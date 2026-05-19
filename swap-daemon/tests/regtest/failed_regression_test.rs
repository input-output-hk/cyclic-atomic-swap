// Regression test: 3-party swap where the leader goes offline AND the
// non-leaders' signed refund txs are removed from memory, forcing the
// non-leaders to reach the Failed state (spend failed because leader is absent,
// refund fails because the signed tx entry is missing).
//
// Participants:
//   P1 (Bitcoin, id 1)
//   P2 (Cardano, id 2) — genesis wallet, pre-funded in testenv genesis config
//   P3 (Bitcoin, id 3)
//
// Swap cycle: P1(BTC) → P2(ADA) → P3(BTC) → P1
//
// Steps:
//   1. All three daemons run the full signing and funding phase.
//   2. Once all daemons reach AwaitingLockConfirmations (lock txs in mempool,
//      signed refund txs already in session.signed_txs, leader elected), we:
//        a. Abort the leader daemon task.
//        b. Remove TxRole::Refund(id) from signed_txs for each non-leader.
//   3. The auto-miner confirms the lock txs, non-leaders reach AwaitingLeaderSpend.
//   4. The RefundWindow poller fires; broadcast_my_refund_tx finds no signed tx
//      and returns false, so the daemon transitions to Failed.
//
// Prerequisites:
//   ./cli/testenv start
//
// Run with:
//   cargo test --features regtest,dashboard --test failed_regression_test -- --nocapture

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use swap_daemon::{
    blockchains::cardano_utils,
    protocol::chain_monitor::cardano_txid,
    types::{
        BitcoinNetwork, Blockchain, CardanoNetwork, Daemon, DaemonConfig,
        Participant, SwapKeys, SwapSession, SwapState, TxRole,
    },
};

#[path = "../common/mod.rs"]
mod common;

use tracing::info;

// =============================================================================
// Fixed keys (same as happypath_regression_test / refund_regression_test)
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

// =============================================================================
// Constants
// =============================================================================

const BITCOIN_RPC_URL: &str = "http://localhost:18443/wallet/mining_wallet";
const ELECTRS_URL: &str = "http://localhost:3002";
const DOLOS_REST_URL: &str = "http://localhost:50051";
const DOLOS_GRPC_URL: &str = "http://localhost:50052";

// Different ports from happypath (9601-9604) and refund (9711-9714) tests.
const P1_TCP_ADDR: &str = "127.0.0.1:9731";
const P2_TCP_ADDR: &str = "127.0.0.1:9732";
const P3_TCP_ADDR: &str = "127.0.0.1:9733";

const P1_ADDR: &str = "bcrt1p9k83ux474s8l6643k395qdc402lm4mq52x5ja88xc5agcpzeetyqz55pmg";
const P1_FUNDING_UTXO_VALUE: u64 = 500_000_000;
const P1_BTC_LOCK_FEE: u64 = 10_000;

const P2_FUNDING_UTXO_VALUE: u64 = 2_000_000_000;

const P3_ADDR: &str = "bcrt1p33wm0auhr9kkahzd6l0kqj85af4cswn276hsxg6zpz85xe2r0y8s7hfsm7";
const P3_FUNDING_UTXO_VALUE: u64 = 500_000_000;
const P3_BTC_LOCK_FEE: u64 = 10_000;

// 240s covers both the BTC block-based windows and the ADA 60-second window.
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
}

async fn fetch_p2_genesis_utxo() -> (String, u32, u64) {
    let url = format!("{DOLOS_REST_URL}/addresses/{P2_CARDANO_ADDR}/utxos");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let body: serde_json::Value = match reqwest::get(&url).await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => {
                    sleep(Duration::from_millis(500)).await;
                    continue;
                }
            },
            Err(_) => {
                sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        let utxos = match body.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => {
                sleep(Duration::from_millis(500)).await;
                continue;
            }
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

/// Split P2's genesis UTxO:
///   [0] P2 lock funding  (P2_FUNDING_UTXO_VALUE)
///   [1] change           (remainder)
///
/// No collateral UTxO is needed: the refund tx is deliberately corrupted before
/// submission, so add_collateral_witness is never called.
async fn setup_cardano_utxos(config: &DaemonConfig) -> FundingInfo {
    let fee: u64 = 400_000;
    let p2_verifying = hex::decode(P2_CARDANO_VERIFYING_KEY).unwrap();
    let network = cardano_utils::CARDANO_TESTNET;

    let (genesis_txid, genesis_vout, genesis_value) = fetch_p2_genesis_utxo().await;
    let change = genesis_value - P2_FUNDING_UTXO_VALUE - fee;

    let unsigned_tx = cardano_utils::build_multi_output_transfer_tx(
        &genesis_txid,
        genesis_vout,
        &[
            (p2_verifying.as_slice(), P2_FUNDING_UTXO_VALUE), // [0] P2 lock funding
            (p2_verifying.as_slice(), change),                 // [1] change
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
        p2_txid: setup_txid,
        p2_vout: 0,
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
async fn three_party_cross_chain_failed_when_leader_absent_and_refund_tx_missing() {
    common::init_tracing();
    // Swap cycle: P1(BTC) → P2(ADA) → P3(BTC) → P1
    // refund_window_secs = 60:
    //   BTC locktime = start_block + distance * ceil(60/600) = start_block + distance
    //   ADA locktime = start_slot  + distance * 60
    //
    // We corrupt BEFORE the lock txs confirm (at AwaitingLockConfirmations), so
    // there is no race between the RefundWindow poller and the corruption step.
    // By the time the RefundWindow poller first fires, the signed refund tx entry
    // is already gone — broadcast_my_refund_tx returns false and the daemon
    // transitions to Failed.

    let config = make_regtest_config(P1_TCP_ADDR);

    info!("=== Setting up Cardano UTxOs ===");
    let funding = setup_cardano_utxos(&config).await;

    info!("=== Setting up Bitcoin UTxOs ===");
    setup_bitcoin_utxos().await;
    let (p1_btc_txid, p1_btc_vout) =
        fetch_bitcoin_utxo_for(P1_ADDR, P1_FUNDING_UTXO_VALUE).await;
    let (p3_btc_txid, p3_btc_vout) =
        fetch_bitcoin_utxo_for(P3_ADDR, P3_FUNDING_UTXO_VALUE).await;

    info!("=== Querying chain tips for session anchors ===");
    let btc_start_block = bitcoin_rpc("getblockcount", serde_json::json!([]))
        .await
        .as_u64()
        .unwrap() as u32;
    let cardano_start_slot = get_dolos_tip_slot().await;
    info!("BTC start block {btc_start_block}, Cardano start slot {cardano_start_slot}");

    let make_session = |me_id: u8| {
        let p1_keys = make_p1_keys();
        let p2_keys = make_p2_keys();
        let p3_keys = make_p3_keys();
        let mut participants = BTreeMap::new();

        // Swap cycle: P1(BTC) → P2(ADA) → P3(BTC) → P1
        // target_participant = "the participant whose lock I claim".

        // P1 (Bitcoin): locks BTC, claims ADA from P2's Cardano lock
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

        // P2 (Cardano): locks ADA, claims BTC from P3's Bitcoin lock
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

        // P3 (Bitcoin): locks BTC, claims BTC from P1's Bitcoin lock
        participants.insert(
            3,
            Participant {
                id: 3,
                blockchain: Blockchain::Bitcoin,
                tcp_address: P3_TCP_ADDR.to_string(),
                target_participant: 1,
                amount_locking: P3_FUNDING_UTXO_VALUE - P3_BTC_LOCK_FEE,
                amount_claiming: P1_FUNDING_UTXO_VALUE - P1_BTC_LOCK_FEE,
                is_me: me_id == 3,
                secp256k1_public_key: p3_keys.public_key.to_string(),
                cardano_wallet_public_key: p3_keys.cardano_wallet_public_key.clone(),
                funding_utxo_txid: p3_btc_txid.clone(),
                funding_utxo_vout: p3_btc_vout,
            },
        );

        let mut session = SwapSession::new(1, participants, btc_start_block, cardano_start_slot, 5_000, 2_000_000);
        session.refund_window_secs = 60;
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

    info!("=== Starting TCP listeners ===");
    let d1_task = {
        let d = d1.clone();
        tokio::spawn(async move { Daemon::run_shared(d).await.ok(); })
    };
    let d2_task = {
        let d = d2.clone();
        tokio::spawn(async move { Daemon::run_shared(d).await.ok(); })
    };
    let d3_task = {
        let d = d3.clone();
        tokio::spawn(async move { Daemon::run_shared(d).await.ok(); })
    };

    #[cfg(feature = "dashboard")]
    {
        tokio::spawn(swap_daemon::dashboard::serve(
            vec![d1.clone(), d2.clone(), d3.clone()],
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

    // Wait until all three daemons have completed the signing phase and broadcast
    // their lock txs.  At this point:
    //   - session.leader is set (elected during signing)
    //   - session.signed_txs contains signed refund txs for all participants
    //   - lock txs are in the Bitcoin mempool / Cardano mempool but NOT yet confirmed
    info!("=== Waiting for lock confirmations phase (signing complete) ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3")] {
        wait_for_state(d, SwapState::AwaitingLockConfirmations, name).await;
    }

    // Leader is already elected.  Read it from d1 (all daemons agree).
    let leader = d1.read().await.sessions.get(&1).unwrap().leader.unwrap();
    info!("Leader elected: participant {leader}");
    let non_leaders: Vec<u8> = (1u8..=3).filter(|&id| id != leader).collect();

    let daemon_map: std::collections::HashMap<u8, Arc<tokio::sync::RwLock<Daemon>>> = [
        (1u8, d1.clone()),
        (2, d2.clone()),
        (3, d3.clone()),
    ]
    .into_iter()
    .collect();

    // Abort the leader task so it never broadcasts a spend tx.  Non-leaders will
    // stall in AwaitingLeaderSpend once the lock txs confirm.
    info!("=== Aborting leader daemon (participant {leader}) ===");
    let leader_task = match leader {
        1 => d1_task,
        2 => d2_task,
        _ => d3_task,
    };
    leader_task.abort();
    sleep(Duration::from_millis(300)).await;

    // Remove each non-leader's own signed refund tx.  This must happen while lock
    // txs are still unconfirmed (before the RefundWindow poller can fire).
    // broadcast_my_refund_tx returns false when the entry is absent, triggering
    // the Failed transition.
    info!("=== Removing signed refund txs for non-leaders ===");
    for &id in &non_leaders {
        let role = TxRole::Refund(id);
        let mut guard = daemon_map[&id].write().await;
        let session = guard.sessions.get_mut(&1).unwrap();
        let had_tx = session.signed_txs.remove(&role).is_some();
        info!(
            "  P{id}: removed TxRole::Refund({id}) from signed_txs (was present: {had_tx})"
        );
        assert!(had_tx, "signed refund tx for P{id} should exist at AwaitingLockConfirmations");
    }

    // Mine extra BTC blocks to ensure ALL refund windows are open.
    // With refund_window_secs=60 and max distance=2: BTC locktimes are
    // start_block+1 and start_block+2.  The auto-miner confirms lock txs at
    // start_block+1; mining 2 more blocks opens the dist=2 window as well.
    // ADA locktime = start_slot + distance*60 opens by wall-clock time.
    info!("=== Mining blocks to open all BTC refund windows ===");
    let throwaway = bitcoin_rpc("getnewaddress", serde_json::json!([]))
        .await
        .as_str()
        .unwrap()
        .to_string();
    bitcoin_rpc("generatetoaddress", serde_json::json!([3, throwaway])).await;

    // Wait for all non-leaders to reach Failed.
    // (ADA window opens in ≤120s; BTC windows are open from mined blocks above.)
    info!("=== Waiting for non-leaders → Failed ===");
    for &id in &non_leaders {
        wait_for_state(
            &daemon_map[&id],
            SwapState::Failed,
            &format!("P{id} ({:?})", make_session(id).participants[&id].blockchain),
        )
        .await;
    }

    info!("\n=== Final state check ===");
    for &id in &non_leaders {
        let session_guard = daemon_map[&id].read().await;
        let session = session_guard.sessions.get(&1).unwrap();
        info!("  P{id}: state={:?}", session.state);
        assert!(
            session.state_history.contains(&SwapState::Failed),
            "P{id} should have reached Failed"
        );
        assert!(
            !session.state_history.contains(&SwapState::Refunded),
            "P{id} should NOT have reached Refunded (refund tx was missing)"
        );
        assert!(
            !session.state_history.contains(&SwapState::Completed),
            "P{id} should NOT have reached Completed (leader was absent)"
        );
    }

    #[cfg(feature = "dashboard")]
    {
        info!("Dashboard showing final state at http://localhost:3030 — keeping alive for 120s");
        sleep(Duration::from_secs(120)).await;
    }

    info!("\n=== 3-party failed swap regression test passed ✓ ===");
    info!(
        "  Leader was participant {leader}. Non-leaders {:?} all reached Failed.",
        non_leaders
    );
}
