// Participant 1 (Bitcoin)
// address:    bcrt1p9k83ux474s8l6643k395qdc402lm4mq52x5ja88xc5agcpzeetyqz55pmg
// secret_key: 584cd549efea713ac5c9576508cb3187fc178eaf4969757c3149bd3c85b10e0e
// public_key: 02db9d1d372d5205f5ba09286984e3cf4581aef27f76b868a92fcd6ba9a38cfaf7
//
// Participant 2 (Cardano — testenv genesis wallet; pre-funded in genesis config)
// address: addr_test1vq5njz7ktkjn8hzaq067r0m0mqg5cswnem76k74wzrjdnmcvsepkp
// secret_key:    e5c410aafd004669d0b9cd8b2a8c18159c1c740270542d22a3dc69e0b7a8e68a
// verifying_key: e6ceac21f27c463f9065fafdc62883d7e52f6a376b498b8838ba513e44c74eca
//
// Participant 3 (Bitcoin — fresh key, coins mined at test start)
// secret_key: 0101010101010101010101010101010101010101010101010101010101010101
// (taproot address and script derived at runtime from the secret key)
//
// Participant 4 (Cardano — funded from genesis wallet at test start)
// secret_key:    0202020202020202020202020202020202020202020202020202020202020202
// verifying_key: 8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394
//
// Swap cycle: P1 (Bitcoin) → P2 (Cardano) → P3 (Bitcoin) → P4 (Cardano) → P1
//   P1 locks BTC, claims ADA from P2's Cardano lock
//   P2 locks ADA, claims BTC from P3's Bitcoin lock
//   P3 locks BTC, claims ADA from P4's Cardano lock
//   P4 locks ADA, claims BTC from P1's Bitcoin lock
//
// Prerequisites:
//   1. ./cli/testenv start
//   (Bitcoin UTXOs are mined automatically by the test via generatetoaddress + 101 maturity blocks)
//
// Run with:
//   cargo test --features regtest,dashboard --test completed_regression_test -- --nocapture

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
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

use tracing::info;

// =============================================================================
// Fixed keys
// =============================================================================

const P1_SECRET_KEY: &str = "584cd549efea713ac5c9576508cb3187fc178eaf4969757c3149bd3c85b10e0e";

const P2_CARDANO_ADDR: &str = "addr_test1vq5njz7ktkjn8hzaq067r0m0mqg5cswnem76k74wzrjdnmcvsepkp";
const P2_CARDANO_SECRET_KEY: &str = "e5c410aafd004669d0b9cd8b2a8c18159c1c740270542d22a3dc69e0b7a8e68a";
const P2_CARDANO_VERIFYING_KEY: &str = "e6ceac21f27c463f9065fafdc62883d7e52f6a376b498b8838ba513e44c74eca";

const P4_CARDANO_SECRET_KEY: &str = "0202020202020202020202020202020202020202020202020202020202020202";
const P4_CARDANO_VERIFYING_KEY: &str = "8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394";

const P3_SECRET_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";

// =============================================================================
// Constants
// =============================================================================

const BITCOIN_RPC_URL: &str = "http://localhost:18443/wallet/mining_wallet";
const ELECTRS_URL: &str = "http://localhost:3002";
const DOLOS_REST_URL: &str = "http://localhost:50051";
const DOLOS_GRPC_URL: &str = "http://localhost:50052";

const P1_TCP_ADDR: &str = "127.0.0.1:9601";
const P2_TCP_ADDR: &str = "127.0.0.1:9602";
const P3_TCP_ADDR: &str = "127.0.0.1:9603";
const P4_TCP_ADDR: &str = "127.0.0.1:9604";

const P1_ADDR: &str = "bcrt1p9k83ux474s8l6643k395qdc402lm4mq52x5ja88xc5agcpzeetyqz55pmg";
const P1_FUNDING_UTXO_VALUE: u64 = 500_000_000; // sent via sendtoaddress — epoch-independent
const P1_BTC_LOCK_FEE: u64 = 10_000;

const P2_FUNDING_UTXO_VALUE: u64 = 2_000_000_000;

const P3_ADDR: &str = "bcrt1p33wm0auhr9kkahzd6l0kqj85af4cswn276hsxg6zpz85xe2r0y8s7hfsm7";
const P3_FUNDING_UTXO_VALUE: u64 = 500_000_000; // sent via sendtoaddress — epoch-independent
const P3_BTC_LOCK_FEE: u64 = 10_000;

const P4_FUNDING_UTXO_VALUE: u64 = 2_000_000_000;
// Collateral for Cardano Plutus spend txs — one UTXO per Cardano lock.
// Plutus txs require ~1.7M lovelace fee; 5M gives comfortable margin.
const CARDANO_COLLATERAL_VALUE: u64 = 5_000_000;

const STATE_TRANSITION_TIMEOUT_SECS: u64 = 60;
const CHAIN_CONFIRMATION_TIMEOUT_SECS: u64 = 120;
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
    let ed25519_signing = ed25519_dalek::SigningKey::from_bytes(bytes.as_slice().try_into().unwrap());
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
    let ed25519_signing = ed25519_dalek::SigningKey::from_bytes(bytes.as_slice().try_into().unwrap());
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
    resp["slot"].as_u64().expect("missing slot field in dolos tip response")
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

    let deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(120);
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
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    info!("Bitcoin setup done ✓");
}

/// Fetch the oldest confirmed UTXO of the given value at the given address.
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
// Cardano setup
// =============================================================================

struct GenesisUtxo {
    txid: String,
    vout: u32,
    value: u64,
}

struct FundingInfo {
    // Cardano funding UTXOs for lock txs
    p2_txid: String,
    p2_vout: u32,
    p4_txid: String,
    p4_vout: u32,
    // Collateral UTXOs — one per spender, owned by the participant who will sign them.
    // P1 spends P2's Cardano lock; P3 spends P4's Cardano lock.
    p1_collateral_txid: String,
    p1_collateral_vout: u32,
    p3_collateral_txid: String,
    p3_collateral_vout: u32,
}

/// Fetch P2's largest unspent UTXO from Dolos, retrying until indexed.
async fn fetch_p2_genesis_utxo() -> GenesisUtxo {
    let url = &format!("{DOLOS_REST_URL}/addresses/{P2_CARDANO_ADDR}/utxos");
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);
    loop {
        let resp = reqwest::get(url).await;
        let body: serde_json::Value = match resp {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    continue;
                }
            },
            Err(_) => {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                continue;
            }
        };
        let utxos = match body.as_array() {
            Some(a) => a,
            None => {
                // Dolos not yet ready (returns error object instead of array)
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
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
            "P2 genesis wallet has no spendable UTxOs after 300s"
        );
        info!("waiting for dolos to index P2 genesis UTXO...");
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

/// Split P2's genesis UTXO into 5 outputs in a single tx:
///   [0] P2 lock tx funding  (P2_FUNDING_UTXO_VALUE)
///   [1] P2 collateral       (CARDANO_COLLATERAL_VALUE)
///   [2] P4 lock tx funding  (P4_FUNDING_UTXO_VALUE)
///   [3] P4 collateral       (CARDANO_COLLATERAL_VALUE)
///   [4] change              (remainder)
///
/// Separate collateral UTXOs are required because P2 and P4 spend txs may be
/// submitted concurrently — sharing a collateral UTXO would cause a mempool conflict.
async fn setup_cardano_utxos() -> FundingInfo {
    let fee: u64 = 400_000;
    let p2_verifying = hex::decode(P2_CARDANO_VERIFYING_KEY).unwrap();
    let p4_verifying = hex::decode(P4_CARDANO_VERIFYING_KEY).unwrap();
    // Collaterals are funded to the *spender's* address: P1 claims P2's lock,
    // P3 claims P4's lock. Each spender signs their own collateral UTXO.
    let p1_verifying = make_p1_keys().cardano_wallet_public_key;
    let p3_verifying = make_p3_keys().cardano_wallet_public_key;
    let network = swap_daemon::blockchains::cardano_utils::CARDANO_TESTNET;

    let genesis = fetch_p2_genesis_utxo().await;
    let GenesisUtxo { txid: genesis_txid, vout: genesis_vout, value: genesis_value } = genesis;

    let change_amount = genesis_value
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
            (p1_verifying.as_slice(), CARDANO_COLLATERAL_VALUE), // [1] P1 collateral (spender of P2's lock)
            (p4_verifying.as_slice(), P4_FUNDING_UTXO_VALUE),   // [2] P4 lock funding
            (p3_verifying.as_slice(), CARDANO_COLLATERAL_VALUE), // [3] P3 collateral (spender of P4's lock)
            (p2_verifying.as_slice(), change_amount),             // [4] change
        ],
        fee,
        network,
    );

    let signed_hex = cardano_utils::sign_cardano_lock_tx(
        &hex::encode(unsigned_tx.to_bytes()),
        &make_p2_keys(),
    )
    .await;

    let transfer_txid = cardano_txid(&signed_hex);
    info!("setup: submitting Cardano funding tx {}", transfer_txid);
    cardano_utils::submit_cardano_tx(&signed_hex, &make_regtest_config(P1_TCP_ADDR)).await;

    info!("setup: waiting for Cardano funding tx to confirm...");
    timeout(
        Duration::from_secs(300),
        async {
            loop {
                let url = format!("{DOLOS_REST_URL}/txs/{}", transfer_txid);
                if let Ok(resp) = reqwest::get(&url).await {
                    if resp.status().is_success() {
                        info!("setup: Cardano funding tx confirmed ✓");
                        return;
                    }
                }
                sleep(Duration::from_millis(500)).await;
            }
        },
    )
    .await
    .expect("timeout waiting for setup Cardano funding tx");

    FundingInfo {
        p2_txid: transfer_txid.clone(),
        p2_vout: 0,
        p1_collateral_txid: transfer_txid.clone(),
        p1_collateral_vout: 1,
        p4_txid: transfer_txid.clone(),
        p4_vout: 2,
        p3_collateral_txid: transfer_txid.clone(),
        p3_collateral_vout: 3,
    }
}

// =============================================================================
// Participant and config construction
// =============================================================================

fn make_participants(
    me_id: u8,
    funding: &FundingInfo,
    p1_btc_txid: &str,
    p1_btc_vout: u32,
    p3_btc_txid: &str,
    p3_btc_vout: u32,
) -> Participants {
    let p1_keys = make_p1_keys();
    let p2_keys = make_p2_keys();
    let p3_keys = make_p3_keys();
    let p4_keys = make_p4_keys();
    let mut participants = BTreeMap::new();

    // Swap cycle: P1 (Bitcoin) → P2 (Cardano) → P3 (Bitcoin) → P4 (Cardano) → P1

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
            // P1 claims Cardano ADA — needs a wallet key to receive to
            cardano_wallet_public_key: p2_keys.cardano_wallet_public_key.clone(),
            funding_utxo_txid: p1_btc_txid.to_string(),
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

    // P3 (Bitcoin): locks BTC, claims ADA from P4's Cardano lock
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
            // P3 claims Cardano ADA — needs a wallet key to receive to
            cardano_wallet_public_key: p4_keys.cardano_wallet_public_key.clone(),
            funding_utxo_txid: p3_btc_txid.to_string(),
            funding_utxo_vout: p3_btc_vout,
        },
    );

    // P4 (Cardano): locks ADA, claims BTC from P1's Bitcoin lock
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

    participants
}

/// Build per-Cardano-lock collaterals, keyed by the Cardano participant ID.
fn make_collaterals(funding: &FundingInfo) -> std::collections::HashMap<u8, CardanoCollateral> {
    let mut map = std::collections::HashMap::new();
    map.insert(1, CardanoCollateral {
        utxo_txid: funding.p1_collateral_txid.clone(),
        utxo_index: funding.p1_collateral_vout,
    });
    map.insert(3, CardanoCollateral {
        utxo_txid: funding.p3_collateral_txid.clone(),
        utxo_index: funding.p3_collateral_vout,
    });
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
// State polling helpers
// =============================================================================

async fn wait_for_state(
    daemon: &tokio::sync::RwLock<Daemon>,
    expected: SwapState,
    description: &str,
) {
    info!("waiting for state: {:?} ({})", expected, description);
    timeout(
        Duration::from_secs(STATE_TRANSITION_TIMEOUT_SECS),
        async {
            loop {
                let guard = daemon.read().await;
                let session = guard.sessions.get(&1).unwrap();
                if session.state_history.contains(&expected) {
                    info!(
                        "state reached: {:?} ✓ (current: {:?})",
                        expected, session.state
                    );
                    return;
                }
                drop(guard);
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        },
    )
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for state {:?} ({})", expected, description));
}

async fn wait_for_completed(daemon: &tokio::sync::RwLock<Daemon>, name: &str) {
    info!("waiting for {} to complete...", name);
    timeout(
        Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS),
        async {
            loop {
                let state = daemon.read().await.sessions.get(&1).unwrap().state.clone();
                if state == SwapState::Completed {
                    info!("{} swap completed ✓", name);
                    return;
                }
                info!("{} current state: {:?}", name, state);
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        },
    )
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for {} to complete", name));
}

// =============================================================================
// The test
// =============================================================================

#[tokio::test]
#[cfg_attr(
    not(feature = "regtest"),
    ignore = "requires local Docker Bitcoin/Cardano regtest network"
)]
async fn four_party_swap_over_tcp() {
    common::init_tracing();
    info!("=== Setting up Cardano UTXOs ===");
    let funding = setup_cardano_utxos().await;
    let collaterals = make_collaterals(&funding);

    info!("=== Setting up Bitcoin UTXOs ===");
    setup_bitcoin_utxos().await;
    let (p1_btc_txid, p1_btc_vout) =
        fetch_bitcoin_utxo_for(P1_ADDR, P1_FUNDING_UTXO_VALUE).await;
    let (p3_btc_txid, p3_btc_vout) =
        fetch_bitcoin_utxo_for(P3_ADDR, P3_FUNDING_UTXO_VALUE).await;

    info!("=== Querying chain tips for session anchors ===");
    // Use Bitcoin Core RPC (not electrs) to avoid indexer lag skewing start_block.
    let btc_start_block = bitcoin_rpc("getblockcount", serde_json::json!([]))
        .await
        .as_u64()
        .unwrap() as u32;
    let cardano_start_slot = get_dolos_tip_slot().await;
    info!("btc_start_block={btc_start_block}, cardano_start_slot={cardano_start_slot}");

    info!("=== Initializing daemons ===");
    let make_session = |me_id: u8| {
        let mut session = SwapSession::new(
            1,
            make_participants(
                me_id, &funding, &p1_btc_txid, p1_btc_vout,
                &p3_btc_txid, p3_btc_vout,
            ),
            btc_start_block,
            cardano_start_slot,
            5_000,
            2_000_000,
        );
        session.cardano_collaterals = collaterals.clone();
        session
    };

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
    for d in [d1.clone(), d2.clone(), d3.clone(), d4.clone()] {
        tokio::spawn(
            async move { swap_daemon::types::Daemon::run_shared(d).await.ok(); },
        );
    }

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

    info!("=== Waiting for leader election ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::AwaitingLeaderElectionCommitments, name).await;
    }
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::AwaitingLeaderElectionNonces, name).await;
    }
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::RefundAndSpendTxsSigning, name).await;
    }
    let leader = d1.read().await.sessions.get(&1).unwrap().leader.unwrap();
    info!("leader elected: participant {} ✓", leader);

    info!("=== Waiting for refund + spend tx signing ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::Funding, name).await;
    }

    info!("=== Waiting for lock tx confirmations ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::AwaitingLockConfirmations, name).await;
    }
    timeout(
        Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS),
        async {
            loop {
                let guard = d1.read().await;
                let s = guard.sessions.get(&1).unwrap();
                if s.confirmed_lock_txs.len() == s.participants.len() {
                    info!("all {} lock txs confirmed ✓", s.participants.len());
                    return;
                }
                drop(guard);
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        },
    )
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for lock tx confirmations"));

    info!("=== Waiting for non-leaders to observe leader's spend tx ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
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
            wait_for_state(d, SwapState::AwaitingLeaderSpend, name).await;
        }
    }

    info!("=== Waiting for swap completion ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_completed(d, name).await;
    }

    info!("\n=== Swap completed successfully! ===");

    // TxRole::Spend(1) = P1 claims from P2's Cardano lock → Cardano spend tx (held by d1)
    // TxRole::Spend(2) = P2 claims from P3's Bitcoin lock → Bitcoin spend tx (held by d2)
    // TxRole::Spend(3) = P3 claims from P4's Cardano lock → Cardano spend tx (held by d3)
    // TxRole::Spend(4) = P4 claims from P1's Bitcoin lock → Bitcoin spend tx (held by d4)
    let cardano_spend_p2_hex = d1.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(1))
        .expect("Cardano spend tx (P1 claims P2 lock) must be present in d1").clone();
    let btc_spend_p2_hex = d2.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(2))
        .expect("Bitcoin spend tx (P2 claims P3 lock) must be present in d2").clone();
    let cardano_spend_p4_hex = d3.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(3))
        .expect("Cardano spend tx (P3 claims P4 lock) must be present in d3").clone();
    let btc_spend_p4_hex = d4.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(4))
        .expect("Bitcoin spend tx (P4 claims P1 lock) must be present in d4").clone();

    info!("=== Verifying Cardano spend txs confirmed on-chain ===");
    for (label, hex) in [
        ("P1 claims P2 lock", &cardano_spend_p2_hex),
        ("P3 claims P4 lock", &cardano_spend_p4_hex),
    ] {
        let txid = cardano_txid(hex);
        info!("Cardano spend ({label}) txid: {txid}");
        timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
            loop {
                let url = format!("{DOLOS_REST_URL}/txs/{txid}");
                if let Ok(resp) = reqwest::get(&url).await {
                    if resp.status().is_success() {
                        info!("Cardano spend tx ({label}) confirmed ✓");
                        return;
                    }
                }
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timeout: Cardano spend tx ({label}) {txid} not confirmed"));
    }

    info!("=== Verifying Bitcoin spend txs confirmed on-chain ===");
    for (label, hex) in [
        ("P2 claims P3 lock", &btc_spend_p2_hex),
        ("P4 claims P1 lock", &btc_spend_p4_hex),
    ] {
        timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
            loop {
                if check_bitcoin_confirmed(hex, ELECTRS_URL).await {
                    info!("Bitcoin spend tx ({label}) confirmed ✓");
                    return;
                }
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timeout: Bitcoin spend tx ({label}) not confirmed"));
    }

    info!("=== All spend txs confirmed on-chain ✓ ===");

    #[cfg(feature = "dashboard")]
    {
        info!("Dashboard showing final state at http://localhost:3030 — keeping alive for 120s");
        sleep(Duration::from_secs(120)).await;
    }
}
