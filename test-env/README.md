# Local Bitcoin + Cardano Test Network

A dockerised private network for the [swap-daemon](../swap-daemon) regression
tests. Everything needed is in this directory — no access to any other
repository is required.

> ### ⚠️ The keys in this directory are public
>
> `config/cardano/keys/` and `config/genesis/shelley/` contain private keys, and
> they are committed here in the clear. They exist so that an ephemeral private
> chain can be produced deterministically, and they are worthless outside it:
> the network they belong to is created and destroyed by `./testenv start`.
>
> **Never reuse them, and never fund the addresses they derive on any real
> network** (mainnet, preprod, preview, or any shared testnet).

---

## Requirements

- Docker 20.10+ and Docker Compose v2
- ~10 GB free disk, ~8 GB RAM
- `curl` (used by `./testenv` for readiness checks)

## Usage

```bash
./testenv start     # fresh network (destroys any existing chain state)
./testenv status    # container health + chain tips
./testenv stop      # tear down containers and volumes
```

First start takes 2–5 minutes while images build; later starts take 30–60
seconds. `start` is deliberately destructive — the regression tests mine blocks
and submit transactions, so each needs a clean chain to be reproducible.

## Services

| Service | Port | Purpose |
| --- | --- | --- |
| `bitcoin-node` | 18443 (RPC), 28332/28333 (ZMQ) | Bitcoin regtest node |
| `electrs` | 3002 (REST), 50001 (Electrum) | Bitcoin address/UTxO index |
| `cardano-node` | 3001 | single-pool Cardano private testnet |
| `dolos` | 50051 (REST), 50052 (gRPC) | Cardano index and tx submission |
| `auto-mining` | — | mines a Bitcoin block per transaction seen on ZMQ |

Bitcoin RPC credentials are `rpcuser` / `rpcpassword`; the Cardano network magic
is `42`.

## Notes for anyone changing this

**Dolos must be v1.6.0 or newer.** Earlier versions, including `1.0.0-rc.5`,
pass the transaction validity range to the Plutus script context in *seconds*
where the ledger uses *milliseconds*. The script then sees a timestamp about a
thousand times too small and rejects valid time-locked refunds at phase 2, while
the node itself accepts the very same transaction. Upstream fixed this in
v1.6.0. The config format also changed: network magic moved from `[upstream]` to
`[chain]`, and storage is on schema `v3`.

**The auto-miner mines a block for every transaction it sees.** Tests cannot
assume they alone control the Bitcoin block height — runs have been observed
ending ten blocks ahead while the test mined only three. Assertions about
block-height-derived timelocks should read the real height rather than compute
an expected one.

**`sed`, `grep` and `awk` are absent from the `cardano-node` image.** The
Cardano Dockerfile copies `busybox` from the official image to supply them,
which also keeps the correct architecture and avoids committing a binary here.

**The Cardano genesis timestamp is stamped at startup.** `docker/cardano/entrypoint.sh`
rewrites `systemStart` and the Byron `startTime` to the next 30-second boundary,
then recomputes the genesis hashes in the node config. That is why the datum
deadlines the daemon writes are relative to a network start that differs on
every run.
