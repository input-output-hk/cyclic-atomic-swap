#!/usr/bin/env python3
"""
Bitcoin Auto-Mining Service

Simplified autominer: creates two wallets (test_wallet + mining_wallet),
pre-funds test_wallet with ~8,750 BTC, then mines 1 block per detected
transaction to mining_wallet.
"""
import os
import time
import signal
import logging
from pathlib import Path

import zmq
import requests
from requests.auth import HTTPBasicAuth

try:
    import yaml
except ImportError:
    yaml = None

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger(__name__)

PRE_FUND_BLOCKS = 200      # Blocks mined to test_wallet (≈8,750 BTC at 50→25 BTC)
MATURITY_BLOCKS = 101       # Blocks to mature coinbase outputs
MIN_HEIGHT = PRE_FUND_BLOCKS + MATURITY_BLOCKS  # 301


def load_config(config_path: str = "/config/bitcoin-auto-mining.yaml") -> dict:
    """Load connection settings from YAML + environment variables."""
    config = {
        "zmq_host": "bitcoin-node",
        "zmq_port": 28332,
        "rpc_host": "bitcoin-node",
        "rpc_port": 18443,
        "rpc_user": "rpcuser",
        "rpc_password": "rpcpassword",
        "rpc_cookie": None,
    }

    if yaml and Path(config_path).exists():
        try:
            with open(config_path, "r") as f:
                data = yaml.safe_load(f) or {}
            btc = data.get("bitcoin", {})
            for key in ("zmq_host", "zmq_port", "rpc_host", "rpc_port"):
                if key in btc:
                    config[key] = btc[key]
            logger.info("Loaded configuration from %s", config_path)
        except Exception as e:
            logger.warning("Failed to load YAML config: %s", e)

    env_map = {
        "BITCOIN_ZMQ_HOST": ("zmq_host", str),
        "BITCOIN_ZMQ_PORT": ("zmq_port", int),
        "BITCOIN_RPC_HOST": ("rpc_host", str),
        "BITCOIN_RPC_PORT": ("rpc_port", int),
        "BITCOIN_RPC_USER": ("rpc_user", str),
        "BITCOIN_RPC_PASSWORD": ("rpc_password", str),
        "BITCOIN_RPC_COOKIE": ("rpc_cookie", str),
    }
    for env_var, (key, cast) in env_map.items():
        val = os.getenv(env_var)
        if val:
            config[key] = cast(val)

    return config


class AutoMiner:
    """Simplified auto-mining service with two-wallet architecture."""

    def __init__(self, config: dict):
        self.zmq_host = config["zmq_host"]
        self.zmq_port = config["zmq_port"]
        self.rpc_base = f"http://{config['rpc_host']}:{config['rpc_port']}"

        # Authentication: cookie file takes precedence
        cookie = config.get("rpc_cookie")
        if cookie:
            try:
                content = Path(cookie).read_text().strip()
                user, pw = content.split(":", 1)
                self.rpc_auth = HTTPBasicAuth(user, pw)
                logger.info("Using cookie authentication from %s", cookie)
            except Exception as e:
                logger.warning("Cookie auth failed (%s), falling back to user/pass", e)
                self.rpc_auth = HTTPBasicAuth(config["rpc_user"], config["rpc_password"])
        else:
            self.rpc_auth = HTTPBasicAuth(config["rpc_user"], config["rpc_password"])

        self.mining_address: str | None = None
        self.running = True
        self.context: zmq.Context | None = None
        self.socket: zmq.Socket | None = None

    # -- RPC helpers -----------------------------------------------------------

    def rpc(self, method: str, params: list | None = None, wallet: str | None = None):
        """Make a JSON-RPC call. Returns the parsed response dict or None."""
        url = f"{self.rpc_base}/wallet/{wallet}" if wallet else self.rpc_base
        try:
            resp = requests.post(
                url,
                json={"jsonrpc": "1.0", "id": "autominer", "method": method, "params": params or []},
                auth=self.rpc_auth,
                timeout=30,
            )
            resp.raise_for_status()
            return resp.json()
        except requests.RequestException as e:
            logger.error("RPC %s failed: %s", method, e)
            return None

    def create_or_load_wallet(self, name: str):
        """Create wallet if it doesn't exist; handle already-loaded case."""
        result = self.rpc("createwallet", [name])
        if result and result.get("error") is None:
            logger.info("Created wallet: %s", name)
            return
        # Already exists — try loading (may also fail if already loaded)
        self.rpc("loadwallet", [name])
        logger.info("Wallet ready: %s", name)

    # -- Startup phases --------------------------------------------------------

    def prefund_test_wallet(self):
        """Mine blocks to pre-fund test_wallet, then mature via mining_wallet."""
        info = self.rpc("getblockchaininfo")
        if not info:
            raise RuntimeError("Cannot reach Bitcoin node")
        height = info["result"]["blocks"]

        if height >= MIN_HEIGHT:
            logger.info("Blockchain height %d >= %d — skipping pre-funding", height, MIN_HEIGHT)
            return

        # Get a test_wallet address for funding
        addr_resp = self.rpc("getnewaddress", ["prefund"], wallet="test_wallet")
        if not addr_resp or not addr_resp.get("result"):
            raise RuntimeError("Failed to get test_wallet address")
        test_addr = addr_resp["result"]

        logger.info("Pre-funding: mining %d blocks to test_wallet …", PRE_FUND_BLOCKS)
        self.rpc("generatetoaddress", [PRE_FUND_BLOCKS, test_addr], wallet="test_wallet")

        # Mine maturity blocks to mining_wallet so coinbase outputs become spendable
        mining_addr = self._get_mining_address()
        logger.info("Maturing: mining %d blocks to mining_wallet …", MATURITY_BLOCKS)
        self.rpc("generatetoaddress", [MATURITY_BLOCKS, mining_addr], wallet="mining_wallet")

        logger.info("Pre-funding complete")

    def _get_mining_address(self) -> str:
        if self.mining_address:
            return self.mining_address
        resp = self.rpc("getnewaddress", ["mining"], wallet="mining_wallet")
        if not resp or not resp.get("result"):
            raise RuntimeError("Failed to get mining_wallet address")
        self.mining_address = resp["result"]
        return self.mining_address

    # -- ZMQ mining loop -------------------------------------------------------

    def setup_zmq(self):
        self.context = zmq.Context()
        self.socket = self.context.socket(zmq.SUB)
        endpoint = f"tcp://{self.zmq_host}:{self.zmq_port}"
        self.socket.connect(endpoint)
        self.socket.setsockopt_string(zmq.SUBSCRIBE, "rawtx")
        logger.info("ZMQ connected: %s", endpoint)

    def run(self):
        """Main entry: create wallets, pre-fund, enter mining loop."""
        signal.signal(signal.SIGINT, self._signal_handler)
        signal.signal(signal.SIGTERM, self._signal_handler)

        time.sleep(2)  # Brief wait for Bitcoin node readiness

        info = self.rpc("getblockchaininfo")
        if not info:
            logger.error("Cannot connect to Bitcoin node — exiting")
            return
        logger.info("Connected to Bitcoin node (%s)", info["result"]["chain"])

        # Wallet setup
        self.create_or_load_wallet("test_wallet")
        self.create_or_load_wallet("mining_wallet")

        # Pre-fund
        self.prefund_test_wallet()

        # Ensure we have a mining address for the loop
        self._get_mining_address()

        # ZMQ loop
        self.setup_zmq()
        logger.info("Listening for transactions …")

        while self.running:
            try:
                if self.socket.poll(timeout=1000):
                    topic_bytes = self.socket.recv()
                    body = self.socket.recv()
                    _seq = self.socket.recv()

                    try:
                        topic = topic_bytes.decode("ascii")
                    except (UnicodeDecodeError, AttributeError):
                        continue

                    if topic == "rawtx":
                        # Only mine if there are real (user) transactions in the mempool.
                        # Mining a block produces a coinbase tx that also triggers "rawtx",
                        # so without this guard we'd loop forever.
                        mempool = self.rpc("getrawmempool")
                        if mempool and mempool.get("result"):
                            logger.info("Mempool has %d tx, mining 1 block", len(mempool["result"]))
                            self.rpc("generatetoaddress", [1, self.mining_address], wallet="mining_wallet")
            except zmq.ZMQError as e:
                logger.error("ZMQ error: %s", e)
                break
            except Exception as e:
                logger.error("Loop error: %s", e)
                continue

        self.cleanup()

    # -- Lifecycle -------------------------------------------------------------

    def _signal_handler(self, signum, _frame):
        logger.info("Signal %d received — shutting down", signum)
        self.running = False

    def cleanup(self):
        if self.socket:
            self.socket.close()
        if self.context:
            self.context.term()
        logger.info("Shutdown complete")


def main():
    config = load_config()
    AutoMiner(config).run()


if __name__ == "__main__":
    main()
