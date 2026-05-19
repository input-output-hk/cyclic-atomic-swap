// Regression test: refund txes are rejected in all expected attack-vector scenarios.
//
// Attack vector table (Cardano) — all four cases are tested:
//
//   Case 1 (phase-1 rejects — current_slot < validity_start_interval):
//     current_slot < refund_slot, validity_start_interval = refund_slot
//     → Ledger rejects (tx not yet valid)
//
//   Case 2 (success path — tested by refunded_regression_test):
//     current_slot >= refund_slot, validity_start_interval = refund_slot
//     → Passes phase-1 and phase-2
//
//   Case 3 (phase-2 rejects — script catches fake-early lower bound):
//     current_slot >= 0, validity_start_interval = 0
//     → Plutus script fails: `expect lower >= d.refund_slot` (0 < refund_slot)
//
//   Case 4 (phase-2 rejects — script catches missing lower bound):
//     no validity_start_interval (NegInfinity lower bound)
//     → Plutus script fails: `expect Finite(lower) = ...` (NegInfinity is not Finite)
//
// For Bitcoin the equivalent is nLockTime enforcement: the refund tx uses
// Sequence::ENABLE_RBF_NO_LOCKTIME which makes nLockTime active; Bitcoin Core
// rejects it as "non-final" when the block height hasn't reached the locktime.
//
// Same 4-party cross-chain setup as refunded_regression_test (P1/P3 Bitcoin, P2/P4 Cardano).
// Uses refund_window_secs=3600 so every refund window is safely closed for the entire
// duration of the test:
//   - BTC blocks_per_window = ceil(3600/600) = 6; after mining 1 confirmation block,
//     height = start_block+1, but the smallest BTC locktime is start_block+6.
//   - ADA dist=1 slot = start_slot+3600; the test completes in well under an hour.
//
// Cases 3 and 4 do NOT require advancing Cardano time: the script rejects them
// purely based on the declared lower bound, regardless of the current slot.
//
// Prerequisites:
//   ./cli/testenv start
//
// Run with:
//   cargo test --features regtest --test early_refund_rejection_regression_test -- --nocapture

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::info;
use cardano_serialization_lib::{BigNum, Transaction, TransactionBody};
use swap_daemon::{
    blockchains::cardano_utils::{self, add_collateral_witness},
    protocol::chain_monitor::cardano_txid,
    types::{
        BitcoinNetwork, Blockchain, CardanoCollateral, CardanoNetwork, Daemon, DaemonConfig,
        Participant, SwapKeys, SwapSession, SwapState, TxRole,
    },
};

#[path = "../common/mod.rs"]
mod common;

// =============================================================================
// Fixed keys  (same as refunded_regression_test)
// =============================================================================

const P1_SECRET_KEY: &str =
    "584cd549efea713ac5c9576508cb3187fc178eaf4969757c3149bd3c85b10e0e";

const P2_CARDANO_SECRET_KEY: &str =
    "e5c410aafd004669d0b9cd8b2a8c18159c1c740270542d22a3dc69e0b7a8e68a";
const P2_CARDANO_VERIFYING_KEY: &str =
    "e6ceac21f27c463f9065fafdc62883d7e52f6a376b498b8838ba513e44c74eca";
const P2_CARDANO_ADDR: &str =
    "addr_test1vq5njz7ktkjn8hzaq067r0m0mqg5cswnem76k74wzrjdnmcvsepkp";

const P3_SECRET_KEY: &str =
    "0101010101010101010101010101010101010101010101010101010101010101";

const P4_CARDANO_SECRET_KEY: &str =
    "0202020202020202020202020202020202020202020202020202020202020202";
const P4_CARDANO_VERIFYING_KEY: &str =
    "8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394";

// =============================================================================
// Constants
// =============================================================================

const BITCOIN_RPC_URL: &str = "http://localhost:18443/wallet/mining_wallet";
const ELECTRS_URL: &str = "http://localhost:3002";
const DOLOS_REST_URL: &str = "http://localhost:50051";
const DOLOS_GRPC_URL: &str = "http://localhost:50052";

const P1_TCP_ADDR: &str = "127.0.0.1:9741";
const P2_TCP_ADDR: &str = "127.0.0.1:9742";
const P3_TCP_ADDR: &str = "127.0.0.1:9743";
const P4_TCP_ADDR: &str = "127.0.0.1:9744";

const P1_ADDR: &str = "bcrt1p9k83ux474s8l6643k395qdc402lm4mq52x5ja88xc5agcpzeetyqz55pmg";
const P1_FUNDING_UTXO_VALUE: u64 = 500_000_000;
const P1_BTC_LOCK_FEE: u64 = 10_000;

const P2_FUNDING_UTXO_VALUE: u64 = 2_000_000_000;
const CARDANO_COLLATERAL_VALUE: u64 = 5_000_000;

const P3_ADDR: &str = "bcrt1p33wm0auhr9kkahzd6l0kqj85af4cswn276hsxg6zpz85xe2r0y8s7hfsm7";
const P3_FUNDING_UTXO_VALUE: u64 = 500_000_000;
const P3_BTC_LOCK_FEE: u64 = 10_000;

const P4_FUNDING_UTXO_VALUE: u64 = 2_000_000_000;

const STATE_TRANSITION_TIMEOUT_SECS: u64 = 240;
const POLL_INTERVAL_MILLIS: u64 = 500;

// =============================================================================
// Key helpers
// =============================================================================

/// Rebuilds a Plutus-witnessed refund tx with a modified (or absent) validity_start_interval.
///
/// The Schnorr redeemer data and script_data_hash are preserved exactly. The Schnorr sig
/// covers blake2b(0x01 || lock_txid) which is independent of the validity range, so it
/// remains valid in the rebuilt tx. Only the collateral Ed25519 witness must be
/// recomputed — call add_collateral_witness after this function.
fn tamper_validity_start_interval(signed_tx_hex: &str, new_start: Option<u64>) -> String {
    let tx = Transaction::from_bytes(hex::decode(signed_tx_hex).unwrap()).unwrap();
    let body = tx.body();
    let mut new_body = TransactionBody::new_tx_body(&body.inputs(), &body.outputs(), &body.fee());
    if let Some(slot) = new_start {
        new_body.set_validity_start_interval_bignum(&BigNum::from(slot));
    }
    if let Some(collateral) = body.collateral() {
        new_body.set_collateral(&collateral);
    }
    if let Some(hash) = body.script_data_hash() {
        new_body.set_script_data_hash(&hash);
    }
    let new_tx = Transaction::new(&new_body, &tx.witness_set(), None);
    hex::encode(new_tx.to_bytes())
}

fn script_address_bech32(network: u8) -> String {
    cardano_utils::script_address(network).to_bech32(None).unwrap()
}

fn make_p1_keys() -> SwapKeys {
    let bytes = hex::decode(P1_SECRET_KEY).unwrap();
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
        cardano_wallet_secret_key: ed25519_signing.to_bytes().to_vec(),
        cardano_wallet_public_key: ed25519_verifying.to_bytes().to_vec(),
    }
}

fn make_p2_keys() -> SwapKeys {
    let cardano_secret = hex::decode(P2_CARDANO_SECRET_KEY).unwrap();
    let cardano_verifying = hex::decode(P2_CARDANO_VERIFYING_KEY).unwrap();
    let secp_secret = musig2::secp256k1::SecretKey::from_slice(&cardano_secret).unwrap();
    let secp_public = musig2::secp256k1::PublicKey::from_secret_key(
        &musig2::secp256k1::Secp256k1::new(),
        &secp_secret,
    );
    SwapKeys {
        secret_key: secp_secret,
        public_key: secp_public,
        cardano_wallet_secret_key: cardano_secret,
        cardano_wallet_public_key: cardano_verifying,
    }
}

fn make_p3_keys() -> SwapKeys {
    let bytes = hex::decode(P3_SECRET_KEY).unwrap();
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
        cardano_wallet_secret_key: ed25519_signing.to_bytes().to_vec(),
        cardano_wallet_public_key: ed25519_verifying.to_bytes().to_vec(),
    }
}

fn make_p4_keys() -> SwapKeys {
    let cardano_secret = hex::decode(P4_CARDANO_SECRET_KEY).unwrap();
    let cardano_verifying = hex::decode(P4_CARDANO_VERIFYING_KEY).unwrap();
    let secp_secret = musig2::secp256k1::SecretKey::from_slice(&cardano_secret).unwrap();
    let secp_public = musig2::secp256k1::PublicKey::from_secret_key(
        &musig2::secp256k1::Secp256k1::new(),
        &secp_secret,
    );
    SwapKeys {
        secret_key: secp_secret,
        public_key: secp_public,
        cardano_wallet_secret_key: cardano_secret,
        cardano_wallet_public_key: cardano_verifying,
    }
}

fn keys_for(id: u8) -> SwapKeys {
    match id {
        1 => make_p1_keys(),
        2 => make_p2_keys(),
        3 => make_p3_keys(),
        4 => make_p4_keys(),
        _ => panic!("unknown participant {id}"),
    }
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
    resp["slot"]
        .as_u64()
        .expect("missing slot field in dolos tip response")
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
    u["status"]["confirmed"].as_bool().unwrap_or(false)
        && u["value"].as_u64() == Some(value)
}

async fn setup_bitcoin_utxos() {
    let p1_ok = get_electrs_utxos(P1_ADDR)
        .await
        .iter()
        .any(|u| utxo_confirmed_with_value(u, P1_FUNDING_UTXO_VALUE));
    let p3_ok = get_electrs_utxos(P3_ADDR)
        .await
        .iter()
        .any(|u| utxo_confirmed_with_value(u, P3_FUNDING_UTXO_VALUE));

    if p1_ok && p3_ok {
        info!("P1 and P3 already have confirmed Bitcoin UTXOs — skipping setup");
        return;
    }

    let balance = bitcoin_rpc("getbalance", serde_json::json!([])).await;
    if balance.as_f64().unwrap_or(0.0) < 20.0 {
        let addr = bitcoin_rpc("getnewaddress", serde_json::json!([]))
            .await
            .as_str()
            .unwrap()
            .to_string();
        bitcoin_rpc("generatetoaddress", serde_json::json!([101, addr])).await;
    }

    if !p1_ok {
        let btc = P1_FUNDING_UTXO_VALUE as f64 / 1e8;
        bitcoin_rpc("sendtoaddress", serde_json::json!([P1_ADDR, btc])).await;
    }
    if !p3_ok {
        let btc = P3_FUNDING_UTXO_VALUE as f64 / 1e8;
        bitcoin_rpc("sendtoaddress", serde_json::json!([P3_ADDR, btc])).await;
    }

    let throwaway = bitcoin_rpc("getnewaddress", serde_json::json!([]))
        .await
        .as_str()
        .unwrap()
        .to_string();
    bitcoin_rpc("generatetoaddress", serde_json::json!([1, throwaway])).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let p1_ok = get_electrs_utxos(P1_ADDR)
            .await
            .iter()
            .any(|u| utxo_confirmed_with_value(u, P1_FUNDING_UTXO_VALUE));
        let p3_ok = get_electrs_utxos(P3_ADDR)
            .await
            .iter()
            .any(|u| utxo_confirmed_with_value(u, P3_FUNDING_UTXO_VALUE));
        if p1_ok && p3_ok {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for Bitcoin UTXOs to confirm"
        );
        sleep(Duration::from_millis(500)).await;
    }
    info!("Bitcoin setup done ✓");
}

async fn fetch_bitcoin_utxo_for(addr: &str, value: u64) -> (String, u32) {
    let utxos = get_electrs_utxos(addr).await;
    let utxo = utxos
        .iter()
        .filter(|u| utxo_confirmed_with_value(u, value))
        .min_by_key(|u| u["status"]["block_height"].as_u64().unwrap_or(u64::MAX))
        .unwrap_or_else(|| panic!("no confirmed Bitcoin UTXO for {addr}"));
    let txid = utxo["txid"].as_str().unwrap().to_string();
    let vout = utxo["vout"].as_u64().unwrap() as u32;
    (txid, vout)
}

// =============================================================================
// Cardano setup helpers
// =============================================================================

struct FundingInfo {
    p2_txid: String,
    p2_vout: u32,
    p2_collateral_txid: String,
    p2_collateral_vout: u32,
    p4_txid: String,
    p4_vout: u32,
    p4_collateral_txid: String,
    p4_collateral_vout: u32,
}

async fn fetch_p2_genesis_utxo() -> (String, u32, u64) {
    let url = format!("{DOLOS_REST_URL}/addresses/{P2_CARDANO_ADDR}/utxos");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let body: serde_json::Value = match reqwest::get(&url).await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => { sleep(Duration::from_millis(500)).await; continue; }
            },
            Err(_) => { sleep(Duration::from_millis(500)).await; continue; }
        };
        let utxos = match body.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => { sleep(Duration::from_millis(500)).await; continue; }
        };
        let best = utxos
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
        if let Some(u) = best {
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
            info!("P2 genesis UTxO: {txid}#{vout} ({value} lovelace)");
            return (txid, vout, value);
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "P2 genesis wallet has no UTxOs after 300s"
        );
        sleep(Duration::from_millis(500)).await;
    }
}

async fn setup_cardano_utxos(config: &DaemonConfig) -> FundingInfo {
    use swap_daemon::protocol::chain_monitor::cardano_txid;

    let fee: u64 = 400_000;
    let p2_verifying = hex::decode(P2_CARDANO_VERIFYING_KEY).unwrap();
    let p4_verifying = hex::decode(P4_CARDANO_VERIFYING_KEY).unwrap();
    let network = cardano_utils::CARDANO_TESTNET;

    let (genesis_txid, genesis_vout, genesis_value) = fetch_p2_genesis_utxo().await;
    let change = genesis_value
        - P2_FUNDING_UTXO_VALUE
        - CARDANO_COLLATERAL_VALUE
        - P4_FUNDING_UTXO_VALUE
        - CARDANO_COLLATERAL_VALUE
        - fee;

    let unsigned_tx = cardano_utils::build_multi_output_transfer_tx(
        &genesis_txid,
        genesis_vout,
        &[
            (p2_verifying.as_slice(), P2_FUNDING_UTXO_VALUE),
            (p2_verifying.as_slice(), CARDANO_COLLATERAL_VALUE),
            (p4_verifying.as_slice(), P4_FUNDING_UTXO_VALUE),
            (p4_verifying.as_slice(), CARDANO_COLLATERAL_VALUE),
            (p2_verifying.as_slice(), change),
        ],
        fee,
        network,
    );

    let signed_hex = cardano_utils::sign_cardano_lock_tx(
        &hex::encode(unsigned_tx.to_bytes()),
        &make_p2_keys(),
    )
    .await;

    let setup_txid = cardano_txid(&signed_hex);
    info!("setup: submitting Cardano funding tx {setup_txid}");
    cardano_utils::submit_cardano_tx(&signed_hex, config).await;

    timeout(Duration::from_secs(300), async {
        loop {
            let url = format!("{DOLOS_REST_URL}/txs/{setup_txid}");
            if let Ok(resp) = reqwest::get(&url).await {
                if resp.status().is_success() {
                    info!("setup: Cardano funding tx confirmed ✓");
                    return;
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("timeout waiting for Cardano funding tx");

    FundingInfo {
        p2_txid: setup_txid.clone(),
        p2_vout: 0,
        p2_collateral_txid: setup_txid.clone(),
        p2_collateral_vout: 1,
        p4_txid: setup_txid.clone(),
        p4_vout: 2,
        p4_collateral_txid: setup_txid.clone(),
        p4_collateral_vout: 3,
    }
}

// =============================================================================
// Config and state helpers
// =============================================================================

fn make_regtest_config(tcp_address: &str) -> DaemonConfig {
    DaemonConfig {
        tcp_address: tcp_address.to_string(),
        bitcoin_network: BitcoinNetwork::Custom(ELECTRS_URL.to_string()),
        cardano_network: CardanoNetwork::Custom {
            grpc_url: DOLOS_GRPC_URL.to_string(),
            rest_url: DOLOS_REST_URL.to_string(),
        },
        blockfrost_api_key: "".to_string(),
        validate_utxos: false,
    }
}

async fn wait_for_state(daemon: &tokio::sync::RwLock<Daemon>, expected: SwapState, label: &str) {
    info!("waiting for {label} to reach state: {expected:?}");
    timeout(
        Duration::from_secs(STATE_TRANSITION_TIMEOUT_SECS),
        async {
            loop {
                let guard = daemon.read().await;
                let session = guard.sessions.get(&1).unwrap();
                if session.state_history.contains(&expected) {
                    info!(
                        "state {expected:?} reached for {label} ✓ (current: {:?})",
                        session.state
                    );
                    return;
                }
                drop(guard);
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        },
    )
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for {label} to reach {expected:?}"));
}

// =============================================================================
// Submission helpers that return the rejection reason, not just a bool
// =============================================================================

/// POST the signed tx hex directly to electrs and return the error body.
/// Panics if the submission unexpectedly succeeds.
async fn assert_btc_rejected(tx_hex: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{ELECTRS_URL}/tx"))
        .body(tx_hex.to_string())
        .send()
        .await
        .expect("electrs POST /tx request failed");
    assert!(
        !resp.status().is_success(),
        "BTC refund tx was accepted — expected rejection"
    );
    resp.text().await.unwrap_or_default()
}

/// Submit the signed tx bytes directly to Dolos via gRPC and return the error message.
/// Panics if the submission unexpectedly succeeds.
async fn assert_ada_rejected(tx_hex: &str) -> String {
    use utxorpc::{CardanoSubmitClient, ClientBuilder};
    let tx_bytes = hex::decode(tx_hex).unwrap();
    let mut client: CardanoSubmitClient = ClientBuilder::new()
        .uri(DOLOS_GRPC_URL)
        .unwrap()
        .build()
        .await;
    match client.submit_tx(tx_bytes).await {
        Ok(_) => panic!("Cardano refund tx was accepted — expected rejection"),
        Err(e) => format!("{e:?}"),
    }
}

/// Attempt Cardano tx submission without panicking.  Returns Ok(ref) if the
/// node/Dolos accepted it (including mempool-only acceptance) or Err(msg) on
/// immediate rejection.
async fn try_ada_submit(tx_hex: &str) -> Result<String, String> {
    use utxorpc::{CardanoSubmitClient, ClientBuilder};
    let tx_bytes = hex::decode(tx_hex).unwrap();
    let mut client: CardanoSubmitClient = ClientBuilder::new()
        .uri(DOLOS_GRPC_URL)
        .unwrap()
        .build()
        .await;
    match client.submit_tx(tx_bytes).await {
        Ok(r) => Ok(format!("{r:?}")),
        Err(e) => Err(format!("{e:?}")),
    }
}

/// Returns the current Dolos chain height (number of blocks produced).
async fn get_dolos_block_height() -> u64 {
    let resp: serde_json::Value = reqwest::get(&format!("{DOLOS_REST_URL}/blocks/latest"))
        .await
        .expect("dolos tip query failed")
        .json()
        .await
        .expect("invalid tip JSON from dolos");
    resp["height"]
        .as_u64()
        .expect("missing height field in dolos tip response")
}

/// Wait until the Dolos chain height has increased by at least `n` blocks.
async fn wait_for_n_new_blocks(n: u64) {
    let start = get_dolos_block_height().await;
    info!("waiting for {n} new blocks (current height={start})");
    timeout(Duration::from_secs(60), async {
        loop {
            let h = get_dolos_block_height().await;
            if h >= start + n {
                info!("chain advanced to height={h} ✓");
                return;
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("timed out waiting for new Cardano blocks");
}

/// Assert that a specific output of the given tx is still unspent at the swap
/// script address.  Fails if the UTxO is gone — meaning a tampered refund tx
/// managed to steal the locked funds (a security regression).
async fn assert_lock_utxo_unspent(lock_txid: &str, vout: u32) {
    let script_addr =
        script_address_bech32(cardano_utils::CARDANO_TESTNET);
    let url = format!("{DOLOS_REST_URL}/addresses/{script_addr}/utxos");
    let resp: serde_json::Value = reqwest::get(&url)
        .await
        .expect("dolos UTxO query failed")
        .json()
        .await
        .expect("invalid UTxO JSON from dolos");
    let found = resp
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .any(|u| {
            u["tx_hash"].as_str() == Some(lock_txid)
                && u["output_index"].as_u64() == Some(vout as u64)
        });
    assert!(
        found,
        "security regression: lock UTxO {lock_txid}:{vout} is gone — \
         tampered refund tx was included in a block and the funds were stolen!"
    );
}

// =============================================================================
// The test
// =============================================================================

#[tokio::test]
#[cfg_attr(
    not(feature = "regtest"),
    ignore = "requires local Docker Bitcoin/Cardano regtest network"
)]
async fn refund_txes_rejected_before_locktime() {
    common::init_tracing();

    // refund_window_secs=3600 → BTC blocks_per_window=6 and ADA window opens 1 hour
    // from start_slot.  Both windows are safely closed for the entire test duration.
    const REFUND_WINDOW_SECS: u64 = 3600;

    let config = make_regtest_config(P1_TCP_ADDR);

    info!("=== Setting up Cardano UTxOs ===");
    let funding = setup_cardano_utxos(&config).await;

    info!("=== Setting up Bitcoin UTxOs ===");
    setup_bitcoin_utxos().await;
    let (p1_btc_txid, p1_btc_vout) = fetch_bitcoin_utxo_for(P1_ADDR, P1_FUNDING_UTXO_VALUE).await;
    let (p3_btc_txid, p3_btc_vout) = fetch_bitcoin_utxo_for(P3_ADDR, P3_FUNDING_UTXO_VALUE).await;

    info!("=== Querying chain tips for session anchors ===");
    let btc_start_block = bitcoin_rpc("getblockcount", serde_json::json!([]))
        .await
        .as_u64()
        .unwrap() as u32;
    let cardano_start_slot = get_dolos_tip_slot().await;
    info!("BTC start block {btc_start_block}, Cardano start slot {cardano_start_slot}");

    let collaterals = {
        let mut m = HashMap::new();
        m.insert(
            2u8,
            CardanoCollateral {
                utxo_txid: funding.p2_collateral_txid.clone(),
                utxo_index: funding.p2_collateral_vout,
            },
        );
        m.insert(
            4u8,
            CardanoCollateral {
                utxo_txid: funding.p4_collateral_txid.clone(),
                utxo_index: funding.p4_collateral_vout,
            },
        );
        m
    };

    let make_session = |me_id: u8| {
        let p1_keys = make_p1_keys();
        let p2_keys = make_p2_keys();
        let p3_keys = make_p3_keys();
        let p4_keys = make_p4_keys();
        let mut participants = BTreeMap::new();

        participants.insert(
            1,
            Participant {
                id: 1,
                blockchain: Blockchain::Bitcoin,
                tcp_address: P1_TCP_ADDR.to_string(),
                target_participant: 2,
                amount_locking: P1_FUNDING_UTXO_VALUE - P1_BTC_LOCK_FEE,
                amount_claiming: P2_FUNDING_UTXO_VALUE,
                is_me: me_id == 1,
                secp256k1_public_key: p1_keys.public_key.to_string(),
                cardano_wallet_public_key: p2_keys.cardano_wallet_public_key.clone(),
                funding_utxo_txid: p1_btc_txid.clone(),
                funding_utxo_vout: p1_btc_vout,
            },
        );
        participants.insert(
            2,
            Participant {
                id: 2,
                blockchain: Blockchain::Cardano,
                tcp_address: P2_TCP_ADDR.to_string(),
                target_participant: 3,
                amount_locking: P2_FUNDING_UTXO_VALUE,
                amount_claiming: P3_FUNDING_UTXO_VALUE - P3_BTC_LOCK_FEE,
                is_me: me_id == 2,
                secp256k1_public_key: p2_keys.public_key.to_string(),
                cardano_wallet_public_key: p2_keys.cardano_wallet_public_key.clone(),
                funding_utxo_txid: funding.p2_txid.clone(),
                funding_utxo_vout: funding.p2_vout,
            },
        );
        participants.insert(
            3,
            Participant {
                id: 3,
                blockchain: Blockchain::Bitcoin,
                tcp_address: P3_TCP_ADDR.to_string(),
                target_participant: 4,
                amount_locking: P3_FUNDING_UTXO_VALUE - P3_BTC_LOCK_FEE,
                amount_claiming: P4_FUNDING_UTXO_VALUE,
                is_me: me_id == 3,
                secp256k1_public_key: p3_keys.public_key.to_string(),
                cardano_wallet_public_key: p4_keys.cardano_wallet_public_key.clone(),
                funding_utxo_txid: p3_btc_txid.clone(),
                funding_utxo_vout: p3_btc_vout,
            },
        );
        participants.insert(
            4,
            Participant {
                id: 4,
                blockchain: Blockchain::Cardano,
                tcp_address: P4_TCP_ADDR.to_string(),
                target_participant: 1,
                amount_locking: P4_FUNDING_UTXO_VALUE,
                amount_claiming: P1_FUNDING_UTXO_VALUE - P1_BTC_LOCK_FEE,
                is_me: me_id == 4,
                secp256k1_public_key: p4_keys.public_key.to_string(),
                cardano_wallet_public_key: p4_keys.cardano_wallet_public_key.clone(),
                funding_utxo_txid: funding.p4_txid.clone(),
                funding_utxo_vout: funding.p4_vout,
            },
        );

        let mut session = SwapSession::new(
            1,
            participants,
            btc_start_block,
            cardano_start_slot,
            5_000,
            2_000_000,
        );
        session.refund_window_secs = REFUND_WINDOW_SECS;
        session.cardano_collaterals = collaterals.clone();
        session
    };

    info!("=== Initializing daemons ===");
    let d1: Arc<tokio::sync::RwLock<Daemon>> = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(make_p1_keys(), make_regtest_config(P1_TCP_ADDR));
        daemon.insert_session(make_session(1));
        daemon
    }));
    let d2: Arc<tokio::sync::RwLock<Daemon>> = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(make_p2_keys(), make_regtest_config(P2_TCP_ADDR));
        daemon.insert_session(make_session(2));
        daemon
    }));
    let d3: Arc<tokio::sync::RwLock<Daemon>> = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(make_p3_keys(), make_regtest_config(P3_TCP_ADDR));
        daemon.insert_session(make_session(3));
        daemon
    }));
    let d4: Arc<tokio::sync::RwLock<Daemon>> = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(make_p4_keys(), make_regtest_config(P4_TCP_ADDR));
        daemon.insert_session(make_session(4));
        daemon
    }));

    info!("=== Starting TCP listeners ===");
    let d1_task = { let d = d1.clone(); tokio::spawn(async move { Daemon::run_shared(d).await.ok(); }) };
    let d2_task = { let d = d2.clone(); tokio::spawn(async move { Daemon::run_shared(d).await.ok(); }) };
    let d3_task = { let d = d3.clone(); tokio::spawn(async move { Daemon::run_shared(d).await.ok(); }) };
    let d4_task = { let d = d4.clone(); tokio::spawn(async move { Daemon::run_shared(d).await.ok(); }) };

    sleep(Duration::from_millis(500)).await;

    info!("=== Starting swap sessions ===");
    d1.write().await.start_swap_session(1).await.unwrap();
    d2.write().await.start_swap_session(1).await.unwrap();
    d3.write().await.start_swap_session(1).await.unwrap();
    d4.write().await.start_swap_session(1).await.unwrap();

    info!("=== Waiting for AwaitingLockConfirmations ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::AwaitingLockConfirmations, name).await;
    }

    let leader = d1.read().await.sessions.get(&1).unwrap().leader.unwrap();
    info!("Leader elected: participant {leader}");

    let participants = d1.read().await.sessions.get(&1).unwrap().participants.clone();
    let non_leaders: Vec<u8> = (1u8..=4).filter(|&id| id != leader).collect();

    let daemon_map: HashMap<u8, Arc<tokio::sync::RwLock<Daemon>>> = [
        (1u8, d1.clone()), (2, d2.clone()), (3, d3.clone()), (4, d4.clone()),
    ]
    .into_iter()
    .collect();

    // Abort ALL daemons so none of them can submit spend or refund txes while
    // we run the attack-vector checks.  We have already collected the signed
    // refund txes from the session state; the daemons are no longer needed.
    info!("=== Aborting all daemons ===");
    d1_task.abort();
    d2_task.abort();
    d3_task.abort();
    d4_task.abort();
    sleep(Duration::from_millis(300)).await;

    // Pick one BTC and one ADA non-leader to test.
    let btc_test_id = *non_leaders
        .iter()
        .find(|&&id| participants[&id].blockchain == Blockchain::Bitcoin)
        .expect("no BTC non-leader found");
    let ada_test_id = *non_leaders
        .iter()
        .find(|&&id| participants[&id].blockchain == Blockchain::Cardano)
        .expect("no ADA non-leader found");

    info!(
        "Testing BTC early rejection for P{btc_test_id}, ADA early rejection for P{ada_test_id}"
    );

    // Retrieve the Cardano lock tx hex to wait for its confirmation.
    // The lock tx body hash (txid) is the same whether the tx is signed or not.
    let ada_lock_tx_hex = d1
        .read()
        .await
        .sessions
        .get(&1)
        .unwrap()
        .lock_txs
        .get(&ada_test_id)
        .unwrap()
        .clone();
    let ada_lock_txid = cardano_txid(&ada_lock_tx_hex);

    // Mine 1 BTC block: confirms BTC lock UTxOs so the rejection is specifically
    // non-final (locktime not met), not missing-inputs.
    // BTC locktime for any non-leader = start_block + dist * blocks_per_window
    // where blocks_per_window = ceil(3600/600) = 6, so the smallest locktime is
    // start_block+6. After mining this block height = start_block+1 < start_block+6.
    let throwaway = bitcoin_rpc("getnewaddress", serde_json::json!([]))
        .await
        .as_str()
        .unwrap()
        .to_string();
    info!("=== Mining 1 block to confirm BTC lock UTxOs ===");
    bitcoin_rpc("generatetoaddress", serde_json::json!([1, throwaway])).await;

    // Wait for the Cardano lock tx to be indexed by Dolos so the UTxO exists
    // on-chain and the rejection is specifically validity_start_interval, not
    // missing-inputs.
    info!("=== Waiting for Cardano lock tx {ada_lock_txid} to confirm in Dolos ===");
    timeout(Duration::from_secs(120), async {
        loop {
            let url = format!("{DOLOS_REST_URL}/txs/{ada_lock_txid}");
            if let Ok(resp) = reqwest::get(&url).await {
                if resp.status().is_success() {
                    info!("Cardano lock tx confirmed ✓");
                    return;
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("timeout waiting for Cardano lock tx confirmation");

    // ── Bitcoin early rejection ───────────────────────────────────────────────
    //
    // The refund tx uses Sequence::ENABLE_RBF_NO_LOCKTIME (0xFFFFFFFD) which
    // makes the tx subject to nLockTime enforcement.  Bitcoin Core rejects it
    // as "non-final" when the locktime has not been reached.
    let btc_signed_hex = daemon_map[&btc_test_id]
        .read()
        .await
        .sessions
        .get(&1)
        .unwrap()
        .signed_txs
        .get(&TxRole::Refund(btc_test_id))
        .unwrap()
        .clone();

    info!("=== Asserting BTC refund tx for P{btc_test_id} is rejected (non-final) ===");
    let btc_err = assert_btc_rejected(&btc_signed_hex).await;
    assert!(
        btc_err.contains("non-final"),
        "BTC refund tx rejection reason should be 'non-final', got: {btc_err}"
    );
    info!("BTC early rejection confirmed ✓  reason: {btc_err}");

    // ── Cardano early rejection ───────────────────────────────────────────────
    //
    // The refund tx has validity_start_interval = refund_slot (= start_slot + dist * 3600).
    // Dolos phase-1 rejects the tx because the current slot < validity_start_interval.
    let ada_signed_hex = daemon_map[&ada_test_id]
        .read()
        .await
        .sessions
        .get(&1)
        .unwrap()
        .signed_txs
        .get(&TxRole::Refund(ada_test_id))
        .unwrap()
        .clone();

    let ada_keys = keys_for(ada_test_id);
    let ada_with_collateral = add_collateral_witness(
        &ada_signed_hex,
        &ada_keys.cardano_wallet_secret_key,
        &ada_keys.cardano_wallet_public_key,
    );

    info!("=== Asserting Cardano refund tx for P{ada_test_id} is rejected (validity range) ===");
    let ada_err = assert_ada_rejected(&ada_with_collateral).await;
    assert!(
        ada_err.contains("phase-1"),
        "Cardano refund tx rejection reason should mention 'phase-1', got: {ada_err}"
    );
    info!("Cardano early rejection confirmed ✓  reason: {ada_err}");

    // ── Cases 3 & 4: phase-2 security via block-production UTxO check ────────
    //
    // Dolos (v1.0.0-rc.5) performs phase-1 validation synchronously but defers
    // Plutus script execution (phase-2) to the Cardano node at block production
    // time.  So for phase-2 failures we cannot rely on Dolos returning an Err.
    // Instead we:
    //   1. Submit the tampered tx (may be accepted by the mempool).
    //   2. Wait for the chain to advance by ≥2 blocks so the node has had the
    //      opportunity to include (and then reject) the tx via the script.
    //   3. Assert the lock UTxO is STILL unspent — if it were spent, a tampered
    //      refund had stolen the funds, which is the security regression we care about.

    // ── Case 3: validity_start_interval = 0 ──────────────────────────────────
    // Phase-1 passes (current_slot ≥ 0 always).
    // Plutus validator: `expect lower >= d.refund_slot` → 0 ≥ refund_slot fails.
    let case3_tx = tamper_validity_start_interval(&ada_signed_hex, Some(0));
    let case3_with_collateral = add_collateral_witness(
        &case3_tx,
        &ada_keys.cardano_wallet_secret_key,
        &ada_keys.cardano_wallet_public_key,
    );
    info!("=== Case 3: submitting Cardano refund tx with lower=0 ===");
    match try_ada_submit(&case3_with_collateral).await {
        Err(e) => info!("Case 3 rejected immediately by Dolos: {e}"),
        Ok(_) => info!(
            "Case 3 accepted by Dolos mempool (phase-2 deferred to block production)"
        ),
    }

    // ── Case 4: no validity_start_interval (NegInfinity lower bound) ─────────
    // Phase-1 passes (no lower bound imposed).
    // Plutus validator: `expect Finite(lower) = lower_bound_type` → NegInfinity
    // is not Finite, so the expect fails.
    let case4_tx = tamper_validity_start_interval(&ada_signed_hex, None);
    let case4_with_collateral = add_collateral_witness(
        &case4_tx,
        &ada_keys.cardano_wallet_secret_key,
        &ada_keys.cardano_wallet_public_key,
    );
    info!("=== Case 4: submitting Cardano refund tx with NegInfinity lower ===");
    match try_ada_submit(&case4_with_collateral).await {
        Err(e) => info!("Case 4 rejected immediately by Dolos: {e}"),
        Ok(_) => info!(
            "Case 4 accepted by Dolos mempool (phase-2 deferred to block production)"
        ),
    }

    // Wait for ≥2 new blocks so the Cardano node has processed any tampered txes.
    info!("=== Waiting for 2 new blocks to allow the Cardano node to process tampered txes ===");
    wait_for_n_new_blocks(2).await;

    // Core security assertions: both tampered txes must NOT have spent the lock UTxO.
    info!("=== Asserting lock UTxO for P{ada_test_id} is still unspent ===");
    assert_lock_utxo_unspent(&ada_lock_txid, 0).await;
    info!("Lock UTxO still unspent after Cases 3 & 4 ✓ — tampered refunds cannot steal funds");

    info!("\n=== Early refund rejection regression test passed ✓ ===");
    info!("  BTC P{btc_test_id} Case 1: locktime not reached → non-final ✓");
    info!("  ADA P{ada_test_id} Case 1: validity_start_interval in future → phase-1 rejection ✓");
    info!("  ADA P{ada_test_id} Case 3: lower=0 → lock UTxO unspent after block production ✓");
    info!("  ADA P{ada_test_id} Case 4: NegInfinity lower → lock UTxO unspent after block production ✓");
    info!("  (Case 2: success path tested by refunded_regression_test)");
}
