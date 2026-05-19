// Regression test: four-party cross-chain swap where the leader goes offline
// after all lock txs are broadcast, forcing the three non-leaders to reclaim
// their locked funds via their co-signed refund txs.
//
// Participants (same keys as happypath_regression_test):
//   P1 (Bitcoin, id 1)
//   P2 (Cardano, id 2) — genesis wallet, pre-funded in testenv genesis config
//   P3 (Bitcoin, id 3)
//   P4 (Cardano, id 4) — funded from P2 genesis wallet at test start
//
// Swap cycle: P1(BTC) → P2(ADA) → P3(BTC) → P4(ADA) → P1
//
// Cardano collaterals are owned by the LOCK OWNERS (P2 and P4), because each
// participant signs collateral for its own Plutus refund tx.  This differs from
// the happypath test where collaterals are owned by the CLAIMANTS (P1 and P3).
//
// The test verifies staggered refund locktime ordering: after waiting for each
// distance-D group to refund, it asserts that higher-distance Bitcoin
// participants have not yet refunded (their block-based locktimes are still
// in the future).
//
// Prerequisites:
//   ./cli/testenv start
//
// Run with:
//   cargo test --features regtest,dashboard --test refunded_regression_test -- --nocapture

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::info;
use swap_daemon::{
    blockchains::cardano_utils,
    protocol::chain_monitor::{bitcoin_txid, cardano_txid},
    types::{
        BitcoinNetwork, Blockchain, CardanoCollateral, CardanoNetwork, Daemon, DaemonConfig,
        Participant, SwapKeys, SwapSession, SwapState, TxRole,
    },
};

#[path = "../common/mod.rs"]
mod common;


// =============================================================================
// Fixed keys  (same as happypath_regression_test)
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

const P1_TCP_ADDR: &str = "127.0.0.1:9711";
const P2_TCP_ADDR: &str = "127.0.0.1:9712";
const P3_TCP_ADDR: &str = "127.0.0.1:9713";
const P4_TCP_ADDR: &str = "127.0.0.1:9714";

const P1_ADDR: &str = "bcrt1p9k83ux474s8l6643k395qdc402lm4mq52x5ja88xc5agcpzeetyqz55pmg";
const P1_FUNDING_UTXO_VALUE: u64 = 500_000_000; // sent via sendtoaddress — epoch-independent
const P1_BTC_LOCK_FEE: u64 = 10_000;

const P2_FUNDING_UTXO_VALUE: u64 = 2_000_000_000;
// Collateral for Plutus refund txs — 5M lovelace gives comfortable margin above the ~1.7M fee.
const CARDANO_COLLATERAL_VALUE: u64 = 5_000_000;

const P3_ADDR: &str = "bcrt1p33wm0auhr9kkahzd6l0kqj85af4cswn276hsxg6zpz85xe2r0y8s7hfsm7";
const P3_FUNDING_UTXO_VALUE: u64 = 500_000_000; // sent via sendtoaddress — epoch-independent
const P3_BTC_LOCK_FEE: u64 = 10_000;

const P4_FUNDING_UTXO_VALUE: u64 = 2_000_000_000;

const STATE_TRANSITION_TIMEOUT_SECS: u64 = 240;
const POLL_INTERVAL_MILLIS: u64 = 500;

// =============================================================================
// Key helpers
// =============================================================================

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

async fn get_electrs_tip() -> u64 {
    reqwest::get(&format!("{ELECTRS_URL}/blocks/tip/height"))
        .await
        .expect("electrs tip query failed")
        .json()
        .await
        .expect("invalid tip JSON")
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

/// Ensure P1 and P3 both have confirmed UTXOs of the expected value.
/// Uses sendtoaddress from the mining wallet so the amount is exact and
/// epoch-independent (no coinbase maturity wait needed).
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

    // Ensure the mining wallet has spendable mature funds.
    let balance = bitcoin_rpc("getbalance", serde_json::json!([])).await;
    if balance.as_f64().unwrap_or(0.0) < 20.0 {
        let addr = bitcoin_rpc("getnewaddress", serde_json::json!([]))
            .await
            .as_str()
            .unwrap()
            .to_string();
        info!("Mining 101 blocks to wallet for spendable funds...");
        bitcoin_rpc("generatetoaddress", serde_json::json!([101, addr])).await;
    }

    if !p1_ok {
        let btc = P1_FUNDING_UTXO_VALUE as f64 / 1e8;
        info!("Sending {btc} BTC to P1...");
        bitcoin_rpc("sendtoaddress", serde_json::json!([P1_ADDR, btc])).await;
    }
    if !p3_ok {
        let btc = P3_FUNDING_UTXO_VALUE as f64 / 1e8;
        info!("Sending {btc} BTC to P3...");
        bitcoin_rpc("sendtoaddress", serde_json::json!([P3_ADDR, btc])).await;
    }

    let throwaway = bitcoin_rpc("getnewaddress", serde_json::json!([]))
        .await
        .as_str()
        .unwrap()
        .to_string();
    info!("Mining 1 block to confirm P1/P3 UTXOs...");
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
            "timed out waiting for Bitcoin UTXOs to confirm in electrs"
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
        .unwrap_or_else(|| panic!("no confirmed Bitcoin UTXO found for {addr}"));
    let txid = utxo["txid"].as_str().unwrap().to_string();
    let vout = utxo["vout"].as_u64().unwrap() as u32;
    info!(
        "{addr} UTXO: {}:{} (block {})",
        txid,
        vout,
        utxo["status"]["block_height"].as_u64().unwrap_or(0),
    );
    (txid, vout)
}

// =============================================================================
// Cardano setup helpers
// =============================================================================

struct FundingInfo {
    p2_txid: String,
    p2_vout: u32,
    /// Owned by P2 — P2 signs when broadcasting its own Cardano refund tx.
    p2_collateral_txid: String,
    p2_collateral_vout: u32,
    p4_txid: String,
    p4_vout: u32,
    /// Owned by P4 — P4 signs when broadcasting its own Cardano refund tx.
    p4_collateral_txid: String,
    p4_collateral_vout: u32,
}

/// Fetch P2's largest UTxO from Dolos, retrying until indexed.
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

/// Split P2's genesis UTXO into 5 outputs:
///   [0] P2 lock funding  → P2's address
///   [1] P2 collateral    → P2's address (P2 signs when refunding its own Cardano lock)
///   [2] P4 lock funding  → P4's address
///   [3] P4 collateral    → P4's address (P4 signs when refunding its own Cardano lock)
///   [4] change           → P2's address
async fn setup_cardano_utxos(config: &DaemonConfig) -> FundingInfo {
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
            (p2_verifying.as_slice(), P2_FUNDING_UTXO_VALUE),   // [0] P2 lock funding
            (p2_verifying.as_slice(), CARDANO_COLLATERAL_VALUE), // [1] P2 collateral
            (p4_verifying.as_slice(), P4_FUNDING_UTXO_VALUE),   // [2] P4 lock funding
            (p4_verifying.as_slice(), CARDANO_COLLATERAL_VALUE), // [3] P4 collateral
            (p2_verifying.as_slice(), change),                   // [4] change
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

    info!("setup: waiting for Cardano funding tx to confirm...");
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
// Config and state polling helpers
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

async fn wait_for_state(
    daemon: &tokio::sync::RwLock<Daemon>,
    expected: SwapState,
    label: &str,
) {
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
// The test
// =============================================================================

#[tokio::test]
#[cfg_attr(
    not(feature = "regtest"),
    ignore = "requires local Docker Bitcoin/Cardano regtest network"
)]
async fn four_party_cross_chain_refund_when_leader_absent() {
    common::init_tracing();
    // Swap cycle: P1(BTC) → P2(ADA) → P3(BTC) → P4(ADA) → P1
    // refund_window_secs = 60:
    //   BTC locktime = start_block + distance * ceil(60/600) = start_block + distance
    //   ADA locktime = start_slot  + distance * 60
    //
    // Staggered refund order (ascending distance from leader, following target chain):
    // the leader's direct target opens first, the leader opens last. This incentivises
    // the leader to broadcast the spend tx promptly — if they delay, their own claim
    // target refunds and the leader loses their payout.
    // Bitcoin ordering is enforced by block-based locktimes; we mine blocks one
    // at a time and assert that higher-distance BTC participants have not yet
    // refunded before their window opens.

    let config = make_regtest_config(P1_TCP_ADDR);

    info!("=== Setting up Cardano UTxOs ===");
    let funding = setup_cardano_utxos(&config).await;

    info!("=== Setting up Bitcoin UTxOs ===");
    setup_bitcoin_utxos().await;
    let (p1_btc_txid, p1_btc_vout) = fetch_bitcoin_utxo_for(P1_ADDR, P1_FUNDING_UTXO_VALUE).await;
    let (p3_btc_txid, p3_btc_vout) = fetch_bitcoin_utxo_for(P3_ADDR, P3_FUNDING_UTXO_VALUE).await;

    info!("=== Querying chain tips for session anchors ===");
    // Use Bitcoin Core RPC (not electrs) to avoid the indexer lag: electrs can trail
    // the actual chain tip by one block, which would cause the dist=2 refund window
    // to open at the same time as dist=1 (start_block would be 1 too low).
    let btc_start_block = bitcoin_rpc("getblockcount", serde_json::json!([]))
        .await
        .as_u64()
        .unwrap() as u32;
    let cardano_start_slot = get_dolos_tip_slot().await;
    info!("BTC start block {btc_start_block}, Cardano start slot {cardano_start_slot}");

    // Collaterals keyed by SIGNER (the participant who owns the UTXO and signs
    // the collateral witness when broadcasting their own refund tx).
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

        let mut session = SwapSession::new(1, participants, btc_start_block, cardano_start_slot, 5_000, 2_000_000);
        session.refund_window_secs = 60;
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

    #[cfg(feature = "dashboard")]
    {
        tokio::spawn(swap_daemon::dashboard::serve(
            vec![d1.clone(), d2.clone(), d3.clone(), d4.clone()],
            3030,
        ));
        sleep(Duration::from_millis(200)).await;
        info!("Dashboard: http://localhost:3030");
    }

    sleep(Duration::from_millis(500)).await;

    info!("=== Starting swap sessions ===");
    d1.write().await.start_swap_session(1).await.unwrap();
    d2.write().await.start_swap_session(1).await.unwrap();
    d3.write().await.start_swap_session(1).await.unwrap();
    d4.write().await.start_swap_session(1).await.unwrap();

    info!("=== Waiting for lock confirmations phase ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::AwaitingLockConfirmations, name).await;
    }

    let leader = d1.read().await.sessions.get(&1).unwrap().leader.unwrap();
    info!("Leader elected: participant {leader}");

    // Compute distances from the leader for all 3 non-leaders.
    let participants = d1.read().await.sessions.get(&1).unwrap().participants.clone();
    let non_leaders: Vec<u8> = (1u8..=4).filter(|&id| id != leader).collect();
    let distances: HashMap<u8, u32> = non_leaders
        .iter()
        .map(|&id| (id, swap_daemon::utils::distance_from_leader(&participants, id, leader)))
        .collect();

    // Sort non-leaders by ascending distance to determine expected refund order.
    let mut sorted_non_leaders = non_leaders.clone();
    sorted_non_leaders.sort_by_key(|id| distances[id]);
    info!("Refund order:");
    for &id in &sorted_non_leaders {
        info!("  P{id} ({:?}, distance {})", participants[&id].blockchain, distances[&id]);
    }

    // Index daemons by participant ID for convenient lookup.
    let daemon_map: HashMap<u8, Arc<tokio::sync::RwLock<Daemon>>> = [
        (1u8, d1.clone()), (2, d2.clone()), (3, d3.clone()), (4, d4.clone()),
    ]
    .into_iter()
    .collect();

    // Abort the leader so it can never broadcast its spend tx.
    info!("=== Aborting leader (participant {leader}) ===");
    let leader_task = match leader {
        1 => d1_task,
        2 => d2_task,
        3 => d3_task,
        _ => d4_task,
    };
    leader_task.abort();
    sleep(Duration::from_millis(300)).await;

    // Wait for all non-leaders to stall in AwaitingLeaderSpend (leader is offline).
    info!("=== Waiting for non-leaders → AwaitingLeaderSpend ===");
    for &id in &non_leaders {
        wait_for_state(&daemon_map[&id], SwapState::AwaitingLeaderSpend, &format!("d{id}")).await;
    }

    let throwaway = bitcoin_rpc("getnewaddress", serde_json::json!([]))
        .await
        .as_str()
        .unwrap()
        .to_string();

    // Drive refund windows in order.
    //
    // BTC locktime = start_block + distance (refund_window_secs=60 → blocks_per_window=1).
    // The daemon opens a BTC window when height >= locktime.
    // The auto-miner confirms the lock tx at start_block + 1, opening the dist=1 window.
    // dist=2 needs one more block, dist=3 two more.
    //
    // Structure per iteration:
    //   1. Assert BTC at distances > dist haven't refunded (window still closed).
    //   2. Wait for all participants at this distance to reach Refunded.
    //   3. Mine 1 block to open the next distance's BTC window (skip after last iteration).
    //
    // Cardano windows open by real time (distance * 60s); no extra mining needed for them.
    for dist in 1u32..=3 {
        // Give the daemon a moment to react to the current chain state before asserting.
        sleep(Duration::from_millis(500)).await;

        // Assert BTC participants at higher distances have not yet refunded.
        // At this point height = start_block + dist, so their locktimes are strictly in the future.
        for &id in sorted_non_leaders.iter().filter(|&&id| {
            participants[&id].blockchain == Blockchain::Bitcoin && distances[&id] > dist
        }) {
            let has_refunded = daemon_map[&id]
                .read()
                .await
                .sessions
                .get(&1)
                .unwrap()
                .state_history
                .contains(&SwapState::Refunded);
            assert!(
                !has_refunded,
                "P{id} (BTC, dist={}) should not have refunded before distance {} window opens",
                distances[&id],
                dist + 1
            );
        }

        // Wait for every non-leader at this distance to reach Refunded.
        for &id in sorted_non_leaders.iter().filter(|&&id| distances[&id] == dist) {
            wait_for_state(
                &daemon_map[&id],
                SwapState::Refunded,
                &format!("P{id} ({:?}, dist={dist})", participants[&id].blockchain),
            )
            .await;
        }

        // Mine 1 block to open the next distance's BTC window.
        if dist < 3 {
            info!("=== Mining block to open BTC window for distance {} ===", dist + 1);
            bitcoin_rpc("generatetoaddress", serde_json::json!([1, throwaway])).await;
        }
    }

    // Final assertion: all non-leaders reached Refunded.
    info!("\n=== Final state check ===");
    for &id in &non_leaders {
        let state = daemon_map[&id].read().await.sessions.get(&1).unwrap().state;
        info!("  P{id}: {state:?}");
        assert!(
            daemon_map[&id]
                .read()
                .await
                .sessions
                .get(&1)
                .unwrap()
                .state_history
                .contains(&SwapState::Refunded),
            "P{id} should have reached Refunded state"
        );
    }

    // Mine 1 more block to confirm any BTC refund txs that are still in the mempool.
    info!("\n=== Mining block to confirm pending BTC refund txs ===");
    bitcoin_rpc("generatetoaddress", serde_json::json!([1, throwaway])).await;

    // Verify each non-leader's refund tx is confirmed on-chain.
    info!("=== Verifying on-chain refund confirmations ===");
    for &id in &non_leaders {
        let signed_tx_hex = daemon_map[&id]
            .read()
            .await
            .sessions
            .get(&1)
            .unwrap()
            .signed_txs
            .get(&TxRole::Refund(id))
            .unwrap()
            .clone();

        match participants[&id].blockchain {
            Blockchain::Bitcoin => {
                let txid = bitcoin_txid(&signed_tx_hex);
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                loop {
                    let confirmed = reqwest::get(&format!("{ELECTRS_URL}/tx/{txid}/status"))
                        .await
                        .unwrap()
                        .json::<serde_json::Value>()
                        .await
                        .unwrap()["confirmed"]
                        .as_bool()
                        .unwrap_or(false);
                    if confirmed {
                        info!("  P{id} BTC refund tx {txid} confirmed ✓");
                        break;
                    }
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "P{id} BTC refund tx {txid} not confirmed after 30s"
                    );
                    sleep(Duration::from_secs(1)).await;
                }
            }
            Blockchain::Cardano => {
                let txid = cardano_txid(&signed_tx_hex);
                let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
                loop {
                    let confirmed =
                        reqwest::get(&format!("{DOLOS_REST_URL}/txs/{txid}"))
                            .await
                            .map(|r| r.status().is_success())
                            .unwrap_or(false);
                    if confirmed {
                        info!("  P{id} Cardano refund tx {txid} confirmed ✓");
                        break;
                    }
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "P{id} Cardano refund tx {txid} not confirmed after 60s"
                    );
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    #[cfg(feature = "dashboard")]
    {
        info!("Dashboard showing final state at http://localhost:3030 — keeping alive for 120s");
        sleep(Duration::from_secs(120)).await;
    }

    info!("\n=== 4-party cross-chain refund regression test passed ✓ ===");
    info!(
        "  Leader was participant {leader}. All {} non-leaders ({:?}) successfully refunded.",
        non_leaders.len(),
        non_leaders
    );
}
