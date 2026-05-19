// 20-party completed swap regression test.
//
// Ring: P1(BTC) → P2(ADA) → P3(BTC) → P4(ADA) → ... → P19(BTC) → P20(ADA) → P1
// 10 Bitcoin participants (odd IDs) and 10 Cardano participants (even IDs).
// P2 is the testenv genesis wallet (pre-funded). All others are funded at test start.
//
// Prerequisites: ./cli/testenv start
// Run: cargo test --features regtest --test 20_party_completed_regression_test -- --nocapture

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use bitcoin::{Address, Network, XOnlyPublicKey};
use swap_daemon::{
    blockchains::cardano_utils,
    protocol::chain_monitor::{cardano_txid, check_bitcoin_confirmed},
    types::{
        BitcoinNetwork, Blockchain, CardanoCollateral, CardanoNetwork, Daemon, DaemonConfig,
        Participant, Participants, SwapKeys, SwapSession, SwapState, TxRole,
    },
};

#[path = "../common/mod.rs"]
mod common;

use tracing::{error, info};

// =============================================================================
// Constants
// =============================================================================

const BITCOIN_RPC_URL: &str = "http://localhost:18443/wallet/mining_wallet";
const ELECTRS_URL: &str = "http://localhost:3002";
const DOLOS_REST_URL: &str = "http://localhost:50051";
const DOLOS_GRPC_URL: &str = "http://localhost:50052";
const P2_CARDANO_ADDR: &str = "addr_test1vq5njz7ktkjn8hzaq067r0m0mqg5cswnem76k74wzrjdnmcvsepkp";

const N: u8 = 20;
const BTC_FUNDING_VALUE: u64 = 500_000_000;
const BTC_LOCK_FEE: u64 = 10_000;
const ADA_FUNDING_VALUE: u64 = 2_000_000_000;
const CARDANO_COLLATERAL_VALUE: u64 = 5_000_000;
const CARDANO_SETUP_FEE: u64 = 600_000;

const STATE_TRANSITION_TIMEOUT_SECS: u64 = 300;
const CHAIN_CONFIRMATION_TIMEOUT_SECS: u64 = 300;
const POLL_INTERVAL_MILLIS: u64 = 500;

// SECRET_KEYS[i] = hex secret key for participant (i+1).
// Odd IDs: Bitcoin participants. Even IDs: Cardano participants.
const SECRET_KEYS: &[&str] = &[
    "584cd549efea713ac5c9576508cb3187fc178eaf4969757c3149bd3c85b10e0e", // P1  Bitcoin
    "e5c410aafd004669d0b9cd8b2a8c18159c1c740270542d22a3dc69e0b7a8e68a", // P2  Cardano (genesis)
    "0101010101010101010101010101010101010101010101010101010101010101", // P3  Bitcoin
    "0202020202020202020202020202020202020202020202020202020202020202", // P4  Cardano
    "0303030303030303030303030303030303030303030303030303030303030303", // P5  Bitcoin
    "0606060606060606060606060606060606060606060606060606060606060606", // P6  Cardano
    "0707070707070707070707070707070707070707070707070707070707070707", // P7  Bitcoin
    "0808080808080808080808080808080808080808080808080808080808080808", // P8  Cardano
    "0909090909090909090909090909090909090909090909090909090909090909", // P9  Bitcoin
    "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a", // P10 Cardano
    "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b", // P11 Bitcoin
    "0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c", // P12 Cardano
    "0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d", // P13 Bitcoin
    "0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e", // P14 Cardano
    "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f", // P15 Bitcoin
    "1010101010101010101010101010101010101010101010101010101010101010", // P16 Cardano
    "1111111111111111111111111111111111111111111111111111111111111111", // P17 Bitcoin
    "1212121212121212121212121212121212121212121212121212121212121212", // P18 Cardano
    "1313131313131313131313131313131313131313131313131313131313131313", // P19 Bitcoin
    "1414141414141414141414141414141414141414141414141414141414141414", // P20 Cardano
];

// =============================================================================
// Key / address helpers
// =============================================================================

fn is_bitcoin(id: u8) -> bool { id % 2 == 1 }

fn target_id(id: u8) -> u8 { if id == N { 1 } else { id + 1 } }

fn tcp_addr(id: u8) -> String { format!("127.0.0.1:{}", 9600 + id as u16) }

fn make_keys_for(id: u8) -> SwapKeys {
    let bytes = hex::decode(SECRET_KEYS[(id - 1) as usize]).unwrap();
    let secret_key = musig2::secp256k1::SecretKey::from_slice(&bytes).unwrap();
    let public_key = musig2::secp256k1::PublicKey::from_secret_key(
        &musig2::secp256k1::Secp256k1::new(),
        &secret_key,
    );
    let ed25519_signing =
        ed25519_dalek::SigningKey::from_bytes(bytes.as_slice().try_into().unwrap());
    let ed25519_verifying = ed25519_signing.verifying_key();
    SwapKeys {
        secret_key,
        public_key,
        cardano_wallet_secret_key: bytes,
        cardano_wallet_public_key: ed25519_verifying.to_bytes().to_vec(),
    }
}

fn btc_taproot_address(id: u8) -> String {
    let keys = make_keys_for(id);
    let xonly =
        XOnlyPublicKey::from_slice(&keys.public_key.serialize()[1..]).unwrap();
    let secp = bitcoin::secp256k1::Secp256k1::new();
    Address::p2tr(&secp, xonly, None, Network::Regtest).to_string()
}

// =============================================================================
// Bitcoin helpers
// =============================================================================

async fn bitcoin_rpc(method: &str, params: serde_json::Value) -> serde_json::Value {
    let client = reqwest::Client::new();
    let body = serde_json::json!({ "jsonrpc": "1.0", "method": method, "params": params });
    client
        .post(BITCOIN_RPC_URL)
        .basic_auth("rpcuser", Some("rpcpassword"))
        .json(&body)
        .send()
        .await
        .expect("Bitcoin RPC request failed")
        .json::<serde_json::Value>()
        .await
        .expect("Bitcoin RPC response was not JSON")["result"]
        .clone()
}

async fn get_dolos_tip_slot() -> u64 {
    let resp: serde_json::Value = reqwest::get(&format!("{DOLOS_REST_URL}/blocks/latest"))
        .await
        .expect("dolos tip query failed")
        .json()
        .await
        .expect("invalid tip JSON from dolos");
    resp["slot"].as_u64().expect("missing slot in dolos tip")
}

async fn get_electrs_utxos(addr: &str) -> Vec<serde_json::Value> {
    reqwest::get(&format!("{ELECTRS_URL}/address/{addr}/utxo"))
        .await
        .expect("electrs UTXO query failed")
        .json()
        .await
        .expect("invalid JSON from electrs")
}

fn utxo_confirmed_with_value(u: &serde_json::Value, value: u64) -> bool {
    u["status"]["confirmed"].as_bool().unwrap_or(false) && u["value"].as_u64() == Some(value)
}

/// Fund each Bitcoin participant's taproot address and return confirmed UTXO refs.
async fn setup_bitcoin_utxos() -> HashMap<u8, (String, u32)> {
    let btc_ids: Vec<u8> = (1..=N).filter(|&i| is_bitcoin(i)).collect();
    let addresses: Vec<(u8, String)> =
        btc_ids.iter().map(|&id| (id, btc_taproot_address(id))).collect();

    let unfunded: Vec<(u8, String)> = {
        let mut v = Vec::new();
        for (id, addr) in &addresses {
            let ok = get_electrs_utxos(addr)
                .await
                .iter()
                .any(|u| utxo_confirmed_with_value(u, BTC_FUNDING_VALUE));
            if !ok {
                v.push((*id, addr.clone()));
            }
        }
        v
    };

    if !unfunded.is_empty() {
        let needed = (unfunded.len() as f64) * (BTC_FUNDING_VALUE as f64 / 1e8);
        let balance = bitcoin_rpc("getbalance", serde_json::json!([])).await;
        if balance.as_f64().unwrap_or(0.0) < needed + 1.0 {
            let addr = bitcoin_rpc("getnewaddress", serde_json::json!([]))
                .await
                .as_str()
                .unwrap()
                .to_string();
            info!("Mining 101 blocks for spendable funds...");
            bitcoin_rpc("generatetoaddress", serde_json::json!([101, addr])).await;
        }
        let btc = BTC_FUNDING_VALUE as f64 / 1e8;
        for (id, addr) in &unfunded {
            info!("Sending {btc} BTC to P{id}...");
            bitcoin_rpc("sendtoaddress", serde_json::json!([addr, btc])).await;
        }
        let throwaway = bitcoin_rpc("getnewaddress", serde_json::json!([]))
            .await
            .as_str()
            .unwrap()
            .to_string();
        info!("Mining 1 block to confirm Bitcoin UTXOs...");
        bitcoin_rpc("generatetoaddress", serde_json::json!([1, throwaway])).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        loop {
            let mut confirmed = true;
            for (_, addr) in &unfunded {
                let ok = get_electrs_utxos(addr)
                    .await
                    .iter()
                    .any(|u| utxo_confirmed_with_value(u, BTC_FUNDING_VALUE));
                if !ok {
                    confirmed = false;
                    break;
                }
            }
            if confirmed {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for Bitcoin UTXOs to confirm"
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        info!("Bitcoin setup done ✓");
    } else {
        info!("All Bitcoin UTXOs already confirmed — skipping setup");
    }

    let mut result = HashMap::new();
    for (id, addr) in &addresses {
        let utxos = get_electrs_utxos(addr).await;
        let utxo = utxos
            .iter()
            .filter(|u| utxo_confirmed_with_value(u, BTC_FUNDING_VALUE))
            .min_by_key(|u| u["status"]["block_height"].as_u64().unwrap_or(u64::MAX))
            .unwrap_or_else(|| panic!("no confirmed BTC UTXO for P{id}"));
        let txid = utxo["txid"].as_str().unwrap().to_string();
        let vout = utxo["vout"].as_u64().unwrap() as u32;
        info!("P{id} BTC UTXO: {txid}:{vout}");
        result.insert(*id, (txid, vout));
    }
    result
}

// =============================================================================
// Cardano setup
// =============================================================================

struct GenesisUtxo {
    txid: String,
    vout: u32,
    value: u64,
}

async fn fetch_p2_genesis_utxo() -> GenesisUtxo {
    let url = format!("{DOLOS_REST_URL}/addresses/{P2_CARDANO_ADDR}/utxos");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let body: serde_json::Value = match reqwest::get(&url).await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
            },
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        let utxos = match body.as_array() {
            Some(a) => a,
            None => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        let candidate = utxos
            .iter()
            .filter(|u| !u["tx_hash"].as_str().unwrap_or("").is_empty())
            .max_by_key(|u| {
                u["amount"]
                    .as_array()
                    .and_then(|a| a.iter().find(|e| e["unit"].as_str() == Some("lovelace")))
                    .and_then(|e| e["quantity"].as_str())
                    .and_then(|q| q.parse::<u64>().ok())
                    .unwrap_or(0)
            });
        if let Some(u) = candidate {
            let txid = u["tx_hash"].as_str().unwrap().to_string();
            let vout = u["output_index"].as_u64().unwrap() as u32;
            let value: u64 = u["amount"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["unit"].as_str() == Some("lovelace"))
                .unwrap()["quantity"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            info!("P2 genesis UTXO: {}#{} ({} lovelace)", txid, vout, value);
            return GenesisUtxo { txid, vout, value };
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "P2 genesis wallet has no UTxOs after 300s"
        );
        info!("waiting for dolos to index P2 genesis UTXO...");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Submit a signed Cardano tx directly to the node socket via `docker exec cardano-cli`.
/// Used for the setup tx because Dolos's TxSubmission2 handler buffers until it
/// accumulates nReq=3 txids; a lone setup tx never fills that batch and stalls.
async fn submit_setup_tx_via_cardano_cli(signed_hex: &str) {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    // cardano-cli expects a text-envelope JSON file.
    let envelope = format!(
        r#"{{"type":"Witnessed Tx ConwayEra","description":"","cborHex":"{}"}}"#,
        signed_hex
    );
    let script = "cat > /tmp/setup_tx.json && \
        cardano-cli latest transaction submit \
          --socket-path /ipc/node.socket \
          --testnet-magic 42 \
          --tx-file /tmp/setup_tx.json";
    let mut child = Command::new("docker")
        .args(["exec", "-i", "cardano-node", "sh", "-c", script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn docker exec");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(envelope.as_bytes()).await;
    }
    let out = child.wait_with_output().await.expect("docker exec failed");
    if out.status.success() {
        info!("setup: cardano-cli submission ✓");
    } else {
        error!(
            "setup: cardano-cli submission error: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Build the Cardano funding tx and return the setup txid.
///
/// Output layout (21 outputs total):
///   [0..9]  : ADA funding for P2, P4, ..., P20 (Cardano lock funding)
///   [10..19]: collateral for P1, P3, ..., P19  (owned by each Bitcoin participant)
///   [20]    : change back to P2
async fn setup_cardano_utxos() -> String {
    let genesis = fetch_p2_genesis_utxo().await;
    let GenesisUtxo { txid: genesis_txid, vout: genesis_vout, value: genesis_value } = genesis;

    let cardano_ids: Vec<u8> = (1..=N).filter(|&i| !is_bitcoin(i)).collect();
    let bitcoin_ids: Vec<u8> = (1..=N).filter(|&i| is_bitcoin(i)).collect();

    let total_funding = cardano_ids.len() as u64 * ADA_FUNDING_VALUE;
    let total_collateral = bitcoin_ids.len() as u64 * CARDANO_COLLATERAL_VALUE;
    let change_amount = genesis_value - total_funding - total_collateral - CARDANO_SETUP_FEE;

    let p2_keys = make_keys_for(2);
    let network = cardano_utils::CARDANO_TESTNET;

    let mut outputs: Vec<(Vec<u8>, u64)> = Vec::new();
    for &id in &cardano_ids {
        outputs.push((make_keys_for(id).cardano_wallet_public_key, ADA_FUNDING_VALUE));
    }
    for &id in &bitcoin_ids {
        outputs.push((make_keys_for(id).cardano_wallet_public_key, CARDANO_COLLATERAL_VALUE));
    }
    outputs.push((p2_keys.cardano_wallet_public_key.clone(), change_amount));

    let outputs_refs: Vec<(&[u8], u64)> =
        outputs.iter().map(|(k, v)| (k.as_slice(), *v)).collect();

    let unsigned_tx = cardano_utils::build_multi_output_transfer_tx(
        &genesis_txid,
        genesis_vout,
        &outputs_refs,
        CARDANO_SETUP_FEE,
        network,
    );

    let signed_hex =
        cardano_utils::sign_cardano_lock_tx(&hex::encode(unsigned_tx.to_bytes()), &p2_keys)
            .await;

    let setup_txid = cardano_txid(&signed_hex);
    info!("setup: submitting Cardano funding tx {}", setup_txid);

    // Submit directly via cardano-cli inside the Docker container.
    // Dolos gRPC relays tx to the node, but its TxSubmission2 handler waits for
    // a full batch of nReq=3 txids before responding to the node's RequestTxIds.
    // A single setup tx never fills that batch, so it stalls.  Going directly to
    // the node socket via cardano-cli bypasses this Dolos bug.
    submit_setup_tx_via_cardano_cli(&signed_hex).await;

    info!("setup: waiting for Cardano funding tx to confirm...");
    timeout(Duration::from_secs(300), async {
        loop {
            let url = format!("{DOLOS_REST_URL}/addresses/{P2_CARDANO_ADDR}/utxos");
            if let Ok(resp) = reqwest::get(&url).await {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(utxos) = body.as_array() {
                        if utxos.iter().any(|u| u["tx_hash"].as_str() == Some(&setup_txid)) {
                            info!("setup: Cardano funding tx confirmed ✓");
                            return;
                        }
                    }
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("timeout waiting for setup Cardano funding tx");

    setup_txid
}

// =============================================================================
// Participant and config construction
// =============================================================================

/// Cardano participant funding vout: P2→0, P4→1, ..., P20→9.
fn cardano_funding_vout(id: u8) -> u32 { ((id - 2) / 2) as u32 }

fn make_participants(
    me_id: u8,
    btc_utxos: &HashMap<u8, (String, u32)>,
    cardano_setup_txid: &str,
) -> Participants {
    let mut participants = BTreeMap::new();
    for id in 1..=N {
        let keys = make_keys_for(id);
        let target = target_id(id);
        let blockchain =
            if is_bitcoin(id) { Blockchain::Bitcoin } else { Blockchain::Cardano };

        let (amount_locking, amount_claiming) = if is_bitcoin(id) {
            (BTC_FUNDING_VALUE - BTC_LOCK_FEE, ADA_FUNDING_VALUE)
        } else {
            (ADA_FUNDING_VALUE, BTC_FUNDING_VALUE - BTC_LOCK_FEE)
        };

        // Bitcoin participants claim ADA from the target Cardano participant;
        // send claimed ADA to that target's address (simplification for testing).
        // Cardano participants use their own wallet key.
        let cardano_wallet_public_key = if is_bitcoin(id) {
            make_keys_for(target).cardano_wallet_public_key
        } else {
            keys.cardano_wallet_public_key.clone()
        };

        let (funding_utxo_txid, funding_utxo_vout) = if is_bitcoin(id) {
            let (txid, vout) = btc_utxos[&id].clone();
            (txid, vout)
        } else {
            (cardano_setup_txid.to_string(), cardano_funding_vout(id))
        };

        participants.insert(
            id,
            Participant {
                id,
                blockchain,
                tcp_address: tcp_addr(id),
                target_participant: target,
                amount_locking,
                amount_claiming,
                is_me: me_id == id,
                secp256k1_public_key: keys.public_key.to_string(),
                cardano_wallet_public_key,
                funding_utxo_txid,
                funding_utxo_vout,
            },
        );
    }
    participants
}

/// Collateral vout: P1→10, P3→11, ..., P19→19.
fn make_collaterals(setup_txid: &str) -> HashMap<u8, CardanoCollateral> {
    let mut map = HashMap::new();
    for (k, id) in (1..=N).filter(|&i| is_bitcoin(i)).enumerate() {
        map.insert(id, CardanoCollateral {
            utxo_txid: setup_txid.to_string(),
            utxo_index: (k + 10) as u32,
        });
    }
    map
}

fn make_regtest_config(tcp_address: &str) -> DaemonConfig {
    DaemonConfig {
        tcp_address: tcp_address.to_string(),
        bitcoin_network: BitcoinNetwork::Custom(ELECTRS_URL.to_string()),
        cardano_network: CardanoNetwork::Custom {
            grpc_url: DOLOS_GRPC_URL.to_string(),
            rest_url: DOLOS_REST_URL.to_string(),
        },
        blockfrost_api_key: "".to_string(),
        validate_utxos: true,
    }
}

// =============================================================================
// State polling
// =============================================================================

async fn wait_for_state(
    daemon: &tokio::sync::RwLock<Daemon>,
    expected: SwapState,
    name: &str,
) {
    info!("waiting for state: {:?} ({})", expected, name);
    timeout(Duration::from_secs(STATE_TRANSITION_TIMEOUT_SECS), async {
        loop {
            let guard = daemon.read().await;
            let session = guard.sessions.get(&1).unwrap();
            if session.state_history.contains(&expected) {
                info!("state reached: {:?} ✓ ({})", expected, name);
                return;
            }
            drop(guard);
            sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for {:?} ({})", expected, name));
}

async fn wait_for_completed(daemon: &tokio::sync::RwLock<Daemon>, name: &str) {
    timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
        loop {
            let state = daemon.read().await.sessions.get(&1).unwrap().state.clone();
            if state == SwapState::Completed {
                info!("{name} completed ✓");
                return;
            }
            info!("{name} state: {:?}", state);
            sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for {name} to complete"));
}

// =============================================================================
// Test
// =============================================================================

#[tokio::test]
#[cfg_attr(
    not(feature = "regtest"),
    ignore = "requires local Docker Bitcoin/Cardano regtest network"
)]
async fn twenty_party_swap_over_tcp() {
    common::init_tracing();
    info!("=== Setting up Cardano UTXOs ===");
    let cardano_setup_txid = setup_cardano_utxos().await;
    let collaterals = make_collaterals(&cardano_setup_txid);

    info!("=== Setting up Bitcoin UTXOs ===");
    let btc_utxos = setup_bitcoin_utxos().await;

    info!("=== Querying chain tips ===");
    let btc_start_block = bitcoin_rpc("getblockcount", serde_json::json!([]))
        .await
        .as_u64()
        .unwrap() as u32;
    let cardano_start_slot = get_dolos_tip_slot().await;
    info!("btc_start_block={btc_start_block}, cardano_start_slot={cardano_start_slot}");

    info!("=== Initializing {} daemons ===", N);
    let make_session = |me_id: u8| {
        let mut session = SwapSession::new(
            1,
            make_participants(me_id, &btc_utxos, &cardano_setup_txid),
            btc_start_block,
            cardano_start_slot,
            5_000,
            2_000_000,
        );
        session.cardano_collaterals = collaterals.clone();
        session
    };

    let daemons: Vec<Arc<tokio::sync::RwLock<Daemon>>> = (1..=N)
        .map(|id| {
            let addr = tcp_addr(id);
            let mut daemon = Daemon::new(make_keys_for(id), make_regtest_config(&addr));
            daemon.insert_session(make_session(id));
            Arc::new(tokio::sync::RwLock::new(daemon))
        })
        .collect();

    info!("=== Starting TCP listeners ===");
    for d in &daemons {
        let d = d.clone();
        tokio::spawn(async move { swap_daemon::types::Daemon::run_shared(d).await.ok(); });
    }

    #[cfg(feature = "dashboard")]
    {
        tokio::spawn(swap_daemon::dashboard::serve(daemons.clone(), 3030));
        sleep(Duration::from_millis(200)).await;
        info!("Dashboard: http://localhost:3030");
    }

    sleep(Duration::from_millis(1000)).await;

    info!("=== Starting swap sessions ===");
    for (i, d) in daemons.iter().enumerate() {
        d.write().await.start_swap_session(1).await.unwrap();
        info!("P{} session started", i + 1);
        // Throttle session starts so the first participant's broadcasts can drain
        // before the next one opens another 19 connections to the same listeners.
        sleep(Duration::from_millis(200)).await;
    }

    info!("=== Waiting for leader election ===");
    for (i, d) in daemons.iter().enumerate() {
        wait_for_state(d, SwapState::AwaitingLeaderElectionCommitments, &format!("d{}", i + 1)).await;
    }
    for (i, d) in daemons.iter().enumerate() {
        wait_for_state(d, SwapState::AwaitingLeaderElectionNonces, &format!("d{}", i + 1)).await;
    }
    for (i, d) in daemons.iter().enumerate() {
        wait_for_state(d, SwapState::RefundAndSpendTxsSigning, &format!("d{}", i + 1)).await;
    }
    let leader = daemons[0].read().await.sessions.get(&1).unwrap().leader.unwrap();
    info!("leader elected: participant {} ✓", leader);

    info!("=== Waiting for refund + spend tx signing ===");
    for (i, d) in daemons.iter().enumerate() {
        wait_for_state(d, SwapState::Funding, &format!("d{}", i + 1)).await;
    }

    info!("=== Waiting for lock tx confirmations ===");
    for (i, d) in daemons.iter().enumerate() {
        wait_for_state(d, SwapState::AwaitingLockConfirmations, &format!("d{}", i + 1)).await;
    }
    timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
        loop {
            let guard = daemons[0].read().await;
            let s = guard.sessions.get(&1).unwrap();
            let confirmed = s.confirmed_lock_txs.len();
            let total = s.participants.len();
            if confirmed == total {
                info!("all {total} lock txs confirmed ✓");
                return;
            }
            drop(guard);
            info!("{confirmed}/{total} lock txs confirmed");
            sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for lock tx confirmations"));

    info!("=== Waiting for non-leaders to observe leader's spend tx ===");
    for (i, d) in daemons.iter().enumerate() {
        let my_id = d
            .read()
            .await
            .sessions
            .get(&1)
            .unwrap()
            .participants
            .values()
            .find(|p| p.is_me)
            .unwrap()
            .id;
        if my_id != leader {
            wait_for_state(d, SwapState::AwaitingLeaderSpend, &format!("d{}", i + 1)).await;
        }
    }

    info!("=== Waiting for swap completion ===");
    for (i, d) in daemons.iter().enumerate() {
        wait_for_completed(d, &format!("d{}", i + 1)).await;
    }

    info!("\n=== Swap completed successfully! ===");

    // Cardano spend txs: Bitcoin participants (odd IDs) claiming Cardano locks.
    info!("=== Verifying Cardano spend txs confirmed on-chain ===");
    for id in (1..=N).filter(|&i| is_bitcoin(i)) {
        let hex = daemons[(id - 1) as usize]
            .read()
            .await
            .sessions
            .get(&1)
            .unwrap()
            .signed_txs
            .get(&TxRole::Spend(id))
            .unwrap_or_else(|| panic!("Cardano spend tx for P{id} not found"))
            .clone();
        let txid = cardano_txid(&hex);
        info!("P{id} Cardano spend txid: {txid}");
        timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
            loop {
                let url = format!("{DOLOS_REST_URL}/txs/{txid}");
                if let Ok(resp) = reqwest::get(&url).await {
                    if resp.status().is_success() {
                        info!("P{id} Cardano spend tx confirmed ✓");
                        return;
                    }
                }
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timeout: P{id} Cardano spend tx {txid} not confirmed"));
    }

    // Bitcoin spend txs: Cardano participants (even IDs) claiming Bitcoin locks.
    info!("=== Verifying Bitcoin spend txs confirmed on-chain ===");
    for id in (1..=N).filter(|&i| !is_bitcoin(i)) {
        let hex = daemons[(id - 1) as usize]
            .read()
            .await
            .sessions
            .get(&1)
            .unwrap()
            .signed_txs
            .get(&TxRole::Spend(id))
            .unwrap_or_else(|| panic!("Bitcoin spend tx for P{id} not found"))
            .clone();
        timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
            loop {
                if check_bitcoin_confirmed(&hex, ELECTRS_URL).await {
                    info!("P{id} Bitcoin spend tx confirmed ✓");
                    return;
                }
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timeout: P{id} Bitcoin spend tx not confirmed"));
    }

    info!("=== All spend txs confirmed on-chain ✓ ===");

    #[cfg(feature = "dashboard")]
    {
        info!("Dashboard showing final state at http://localhost:3030 — keeping alive for 120s");
        sleep(Duration::from_secs(120)).await;
    }
}
