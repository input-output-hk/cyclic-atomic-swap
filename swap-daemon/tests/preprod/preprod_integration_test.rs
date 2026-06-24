// 4-party swap over Bitcoin Testnet4 + Cardano Pre-production.
//
// Swap cycle: P1 (Bitcoin) → P2 (Cardano) → P3 (Bitcoin) → P4 (Cardano) → P1
//   P1 locks tBTC, claims tADA from P2's Cardano lock
//   P2 locks tADA, claims tBTC from P3's Bitcoin lock
//   P3 locks tBTC, claims tADA from P4's Cardano lock
//   P4 locks tADA, claims tBTC from P1's Bitcoin lock
//
// Keys are hardcoded. UTXOs are fetched automatically from mempool.space
// (Bitcoin Testnet4) and Blockfrost (Cardano Preprod) at test startup.
//
// The largest available UTXO at each address is used for locking.
// The smallest UTXO at P1's and P3's Cardano addresses is used for collateral.
//
// ═══════════════════════════════════════════════════════════════════
// HOW TO FUND THE TEST WALLETS (one-time setup)
// ═══════════════════════════════════════════════════════════════════
//
// Addresses (fixed — derived deterministically from the hardcoded keys above)
// ───────────────────────────────────────────────────────────────────
//   P1 Bitcoin  (lock):       tb1ppz3r506h0wcp7ll52memker76k492tkgh5q9ngfvxd4a4c6h4h4qux0kg5
//   P1 Cardano  (collateral): addr_test1vpmk8u6p7ta4y8y5qyhv6mhky7hwaw25htd6dn4lwjj5a0gzxj2dk
//   P2 Cardano  (lock):       addr_test1vrt72mndspvlhmnqujkj5zc5cxg7u78qsdhj5y8avuumecsukf337
//   P3 Bitcoin  (lock):       tb1ph2069levne8qgz9m5snu3ezaztyua9eml6k6f7qnjkfm0fvnwqkq43newx
//   P3 Cardano  (collateral): addr_test1vzyay6n22vqz72x6njmus4vfnuhxlrjada7t4ugpd0fhr5qgwa9kl
//   P4 Cardano  (lock):       addr_test1vrrg74q558phdy00wrvkx4mkmkst3hmckf7fevqxv8wh3uqgp77ad
//
// Step 1 — Fund Bitcoin Testnet4 addresses (P1 and P3)
// ───────────────────────────────────────────────────────────────────
// Fund both addresses. Any amount ≥ 20 000 sat (0.0002 tBTC) is sufficient.
//
//   P1: tb1ppz3r506h0wcp7ll52memker76k492tkgh5q9ngfvxd4a4c6h4h4qux0kg5
//   P3: tb1ph2069levne8qgz9m5snu3ezaztyua9eml6k6f7qnjkfm0fvnwqkq43newx
//
//   1. Go to https://mempool.space/testnet4/faucet
//   2. Paste the address, complete the CAPTCHA, click "Get coins".
//      (~0.001 tBTC per request, repeat for each address.)
//
// Alternative faucets if mempool.space is drained:
//   • https://coinfaucet.eu/en/btc-testnet4/
//   • https://faucet.testnet4.dev/
//
// Bitcoin faucets all require a browser CAPTCHA and cannot be called
// programmatically, so this step is always manual.
//
// Step 2 — Fund Cardano Preprod lock addresses (P2 and P4)
// ───────────────────────────────────────────────────────────────────
// Fund both addresses. Any amount of ADA is fine (the whole UTXO gets locked).
//
//   P2: addr_test1vrt72mndspvlhmnqujkj5zc5cxg7u78qsdhj5y8avuumecsukf337
//   P4: addr_test1vrrg74q558phdy00wrvkx4mkmkst3hmckf7fevqxv8wh3uqgp77ad
//
//   1. Go to https://faucet.preprod.world.dev.cardano.org/basic-faucet
//   2. Paste the address, leave "API key" blank, click "Request funds".
//      (~1 000 ADA per request, repeat for each address.)
//
// Alternative faucet: https://faucet.triangleplatform.com/cardano/preprod
//   (sends 1 ADA — useful if the IOG faucet is rate-limited)
//
// Step 3 — Fund Cardano Preprod collateral addresses (P1 and P3)
// ───────────────────────────────────────────────────────────────────
// P1 and P3 are Bitcoin participants, but every participant needs a small Cardano
// UTXO to use as Plutus script collateral. Their Cardano addresses are derived
// from the same key bytes as their Bitcoin keys (Ed25519 reuse).
//
//   P1: addr_test1vpmk8u6p7ta4y8y5qyhv6mhky7hwaw25htd6dn4lwjj5a0gzxj2dk
//   P3: addr_test1vzyay6n22vqz72x6njmus4vfnuhxlrjada7t4ugpd0fhr5qgwa9kl
//
//   1. Go to https://faucet.preprod.world.dev.cardano.org/basic-faucet
//   2. Paste the address and click "Request funds". Any amount ≥ 2 ADA is fine.
//
// IMPORTANT: the test picks the SMALLEST UTXO at these addresses for collateral.
// If you top up one of these addresses a second time, do it in a separate
// transaction so the UTXOs remain distinct — do not consolidate them.
//
// Step 4 — Run the test
// ───────────────────────────────────────────────────────────────────
//   cargo test --test preprod_integration_test -- --ignored --nocapture
//
// The test checks all 6 addresses before doing anything else. If any address is
// empty it prints the address and relevant faucet URLs, then fails. If all are
// funded it fetches UTXOs automatically and runs the full swap end-to-end.
//
// After a successful run the lock UTXOs are spent. Re-fund before running again.
// ═══════════════════════════════════════════════════════════════════

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{error, info, warn};
use swap_daemon::{
    blockchains::cardano_utils::CARDANO_TESTNET,
    protocol::chain_monitor::{cardano_txid, check_bitcoin_confirmed},
    types::{
        BitcoinNetwork, Blockchain, CardanoCollateral, CardanoNetwork, Daemon, DaemonConfig,
        Participant, Participants, SwapKeys, SwapSession, SwapState, TxRole,
    },
};

use std::sync::Once;

static TRACING: Once = Once::new();

fn init_tracing() {
    TRACING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .init();
    });
}

// =============================================================================
// Fixed keys — these never change
// =============================================================================

const P1_SECRET_KEY: &str          = "ecf054463ed99efb37c5ef39c82df5f3aac830ff79880343ec10ee944a16206e";
const P2_CARDANO_SECRET_KEY: &str  = "00c7ee4aedec35c3bdc9f044579d814d94a85e6a6dda3bda06b552ba2b36346b";
const P3_SECRET_KEY: &str          = "17da2f914f6bcf542f6a369fd151b1dc25265b32c894ee73cae571a866ca1fdc";
const P4_CARDANO_SECRET_KEY: &str  = "6bdeb94b5e492b2408eed7bc91662ddbd8dfcc6533c20ac1345894d42d3ff091";

// =============================================================================
// Network / timing constants
// =============================================================================

const P1_TCP_ADDR: &str = "127.0.0.1:9701";
const P2_TCP_ADDR: &str = "127.0.0.1:9702";
const P3_TCP_ADDR: &str = "127.0.0.1:9703";
const P4_TCP_ADDR: &str = "127.0.0.1:9704";

const P1_BTC_LOCK_FEE: u64 = 10_000;
const P3_BTC_LOCK_FEE: u64 = 10_000;

fn blockfrost_api_key() -> String {
    std::env::var("BLOCKFROST_API_KEY").expect("BLOCKFROST_API_KEY env var required for preprod tests")
}
const BLOCKFROST_PREPROD_URL: &str  = "https://cardano-preprod.blockfrost.io/api/v0";
const MEMPOOL_TESTNET4_URL: &str    = "https://mempool.space/testnet4/api";

// Optional: IOG-issued API key for the Cardano preprod faucet.
// Without this the faucet call is skipped and you must fund ADA addresses manually.
// Request one at https://developers.cardano.org/docs/get-started/networks/testnets/
const CARDANO_FAUCET_API_KEY: Option<&str> = None;
const CARDANO_FAUCET_URL: &str = "https://faucet.preprod.world.dev.cardano.org/send-money";

const STATE_TRANSITION_TIMEOUT_SECS: u64   = 120;
const CHAIN_CONFIRMATION_TIMEOUT_SECS: u64 = 1800; // 30 min — Bitcoin testnet4 can be slow
const POLL_INTERVAL_MILLIS: u64            = 2_000;

// =============================================================================
// Key construction
// =============================================================================

fn make_bitcoin_keys(secret_key_hex: &str) -> SwapKeys {
    let bytes = hex::decode(secret_key_hex).unwrap();
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

fn make_cardano_keys(cardano_secret_hex: &str) -> SwapKeys {
    let bytes = hex::decode(cardano_secret_hex).unwrap();
    let secp_secret = musig2::secp256k1::SecretKey::from_slice(&bytes).unwrap();
    let secp_public = musig2::secp256k1::PublicKey::from_secret_key(
        &musig2::secp256k1::Secp256k1::new(),
        &secp_secret,
    );
    let ed25519_signing = ed25519_dalek::SigningKey::from_bytes(bytes.as_slice().try_into().unwrap());
    let ed25519_verifying = ed25519_signing.verifying_key();
    SwapKeys {
        secret_key: secp_secret,
        public_key: secp_public,
        cardano_wallet_secret_key: ed25519_signing.to_bytes().to_vec(),
        cardano_wallet_public_key: ed25519_verifying.to_bytes().to_vec(),
    }
}

// =============================================================================
// Address derivation
// =============================================================================

fn bitcoin_testnet4_address(keys: &SwapKeys) -> String {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let xonly = bitcoin::XOnlyPublicKey::from_slice(&keys.public_key.serialize()[1..]).unwrap();
    bitcoin::Address::p2tr(&secp, xonly, None, bitcoin::Network::Testnet)
        .to_string()
}


fn cardano_preprod_address(keys: &SwapKeys) -> String {
    use cardano_serialization_lib::{Credential, EnterpriseAddress, PublicKey};
    let pubkey = PublicKey::from_bytes(&keys.cardano_wallet_public_key).unwrap();
    let credential = Credential::from_keyhash(&pubkey.hash());
    EnterpriseAddress::new(CARDANO_TESTNET, &credential)
        .to_address()
        .to_bech32(None)
        .unwrap()
}

// =============================================================================
// UTXO fetching
// =============================================================================

struct BtcUtxo {
    txid: String,
    vout: u32,
    value: u64,   // satoshis
}

struct AdaUtxo {
    txid: String,
    vout: u32,
    value: u64,   // lovelace
}

/// Fetch all UTXOs for a Bitcoin testnet4 address from mempool.space,
/// returning the one with the largest value.
async fn fetch_largest_btc_utxo(address: &str) -> BtcUtxo {
    let url = format!("{MEMPOOL_TESTNET4_URL}/address/{address}/utxo");
    let utxos: serde_json::Value = reqwest::get(&url).await.unwrap().json().await.unwrap();
    let utxos = utxos.as_array().expect("expected array from mempool.space");
    assert!(!utxos.is_empty(), "no UTXOs at Bitcoin address {address}");

    let best = utxos.iter().max_by_key(|u| u["value"].as_u64().unwrap_or(0)).unwrap();
    let txid  = best["txid"].as_str().unwrap().to_string();
    let vout  = best["vout"].as_u64().unwrap() as u32;
    let value = best["value"].as_u64().unwrap();

    info!("  BTC UTXO: {txid}:{vout}  ({value} sat)");
    BtcUtxo { txid, vout, value }
}

/// Fetch all UTXOs for a Cardano preprod address from Blockfrost,
/// returning the one with the largest lovelace value.
async fn fetch_largest_ada_utxo(address: &str) -> AdaUtxo {
    fetch_ada_utxo(address, false).await
}

/// Fetch the smallest ADA UTXO — used for collateral so it doesn't consume
/// the main lock UTXO.
async fn fetch_smallest_ada_utxo(address: &str) -> AdaUtxo {
    fetch_ada_utxo(address, true).await
}

async fn fetch_ada_utxo(address: &str, smallest: bool) -> AdaUtxo {
    let url = format!("{BLOCKFROST_PREPROD_URL}/addresses/{address}/utxos");
    let client = reqwest::Client::new();
    let utxos: serde_json::Value = client
        .get(&url)
        .header("project_id", blockfrost_api_key())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let utxos = utxos.as_array().expect("expected array from Blockfrost");
    assert!(!utxos.is_empty(), "no UTXOs at Cardano address {address}");

    let lovelace = |u: &serde_json::Value| -> u64 {
        u["amount"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["unit"].as_str() == Some("lovelace"))
            .and_then(|a| a["quantity"].as_str())
            .and_then(|q| q.parse().ok())
            .unwrap_or(0)
    };

    let best = if smallest {
        utxos.iter().min_by_key(|u| lovelace(u)).unwrap()
    } else {
        utxos.iter().max_by_key(|u| lovelace(u)).unwrap()
    };

    let txid  = best["tx_hash"].as_str().unwrap().to_string();
    let vout  = best["output_index"].as_u64().unwrap() as u32;
    let value = lovelace(best);

    info!("  ADA UTXO: {txid}#{vout}  ({value} lovelace)");
    AdaUtxo { txid, vout, value }
}

// =============================================================================
// Pre-flight funding helpers
// =============================================================================

/// Check whether a Bitcoin testnet4 address has any UTXOs.
/// Retries up to 3 times on transient errors before returning false.
async fn btc_has_funds(address: &str) -> bool {
    let url = format!("{MEMPOOL_TESTNET4_URL}/address/{address}/utxo");
    for attempt in 1..=3 {
        match reqwest::get(&url).await {
            Ok(r) if r.status().is_success() => {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    return v.as_array().map(|a| !a.is_empty()).unwrap_or(false);
                }
            }
            Ok(r) => error!("  btc_has_funds attempt {attempt}: status {}", r.status()),
            Err(e) => error!("  btc_has_funds attempt {attempt}: {e}"),
        }
        sleep(Duration::from_secs(5)).await;
    }
    false
}

/// Check whether a Cardano preprod address has any UTXOs via Blockfrost.
/// 404 means the address has never appeared on-chain → no funds.
/// Retries up to 3 times on other transient errors (5xx, network failure).
async fn ada_has_funds(address: &str) -> bool {
    let url = format!("{BLOCKFROST_PREPROD_URL}/addresses/{address}/utxos");
    let client = reqwest::Client::new();
    for attempt in 1..=3 {
        match client.get(&url).header("project_id", blockfrost_api_key()).send().await {
            Ok(r) if r.status().is_success() => {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    return v.as_array().map(|a| !a.is_empty()).unwrap_or(false);
                }
            }
            // 404 = address unknown to Blockfrost = never used = no funds
            Ok(r) if r.status() == 404 => return false,
            Ok(r) => warn!("  ada_has_funds attempt {attempt}: status {}", r.status()),
            Err(e) => error!("  ada_has_funds attempt {attempt}: {e}"),
        }
        sleep(Duration::from_secs(5)).await;
    }
    false
}

/// Request ADA from the Cardano preprod faucet using the IOG API.
/// Requires CARDANO_FAUCET_API_KEY to be set; skips silently if not.
async fn fund_ada_from_faucet(address: &str) {
    let key = match CARDANO_FAUCET_API_KEY {
        Some(k) => k,
        None => {
            info!("  No Cardano faucet API key set — skipping auto-fund for {address}");
            return;
        }
    };
    let url = format!("{CARDANO_FAUCET_URL}/{address}?api_key={key}");
    info!("  Requesting ADA from faucet for {address} …");
    let client = reqwest::Client::new();
    match client.post(&url).send().await {
        Ok(r) => info!("  Faucet response: {}", r.status()),
        Err(e) => info!("  Faucet request failed: {e}"),
    }
}

/// Ensure a Bitcoin testnet4 address is funded, or panic with clear instructions.
async fn ensure_btc_funded(address: &str, label: &str) {
    if btc_has_funds(address).await {
        info!("  {label} already funded: {address}");
        return;
    }
    panic!(
        "\n\n\
        ╔══ Bitcoin Testnet4 address not funded ══╗\n\
        ║  {label}: {address}\n\
        ╠═══════════════════════════════════════════╣\n\
        ║  Bitcoin Testnet4 faucets require CAPTCHA/OAuth and cannot\n\
        ║  be called programmatically. Please fund this address manually:\n\
        ║\n\
        ║  • https://mempool.space/testnet4/faucet\n\
        ║  • https://coinfaucet.eu/en/btc-testnet4/\n\
        ║  • https://faucet.testnet4.dev/\n\
        ║\n\
        ║  Any amount ≥ 0.0002 tBTC (20 000 sat) is sufficient.\n\
        ╚═══════════════════════════════════════════╝\n"
    );
}

/// Ensure a Cardano preprod address is funded.
/// If it is empty and CARDANO_FAUCET_API_KEY is set, calls the faucet and waits.
/// If the key is not set, panics with instructions.
async fn ensure_ada_funded(address: &str, label: &str) {
    if ada_has_funds(address).await {
        info!("  {label} already funded: {address}");
        return;
    }

    if CARDANO_FAUCET_API_KEY.is_some() {
        fund_ada_from_faucet(address).await;
        info!("  Waiting up to 120 s for ADA to arrive at {address} …");
        let arrived = timeout(Duration::from_secs(120), async {
            loop {
                sleep(Duration::from_secs(15)).await;
                if ada_has_funds(address).await {
                    return;
                }
            }
        })
        .await
        .is_ok();
        if arrived {
            info!("  {label} funded ✓");
            return;
        }
        panic!("Faucet was called but no UTXOs appeared at {label} ({address}) within 120 s");
    }

    panic!(
        "\n\n\
        ╔══ Cardano Preprod address not funded ══╗\n\
        ║  {label}: {address}\n\
        ╠══════════════════════════════════════════╣\n\
        ║  To auto-fund: set CARDANO_FAUCET_API_KEY in the test constants\n\
        ║  (request a key at https://developers.cardano.org/docs/get-started/networks/testnets/)\n\
        ║\n\
        ║  To fund manually: https://faucet.preprod.world.dev.cardano.org/basic-faucet\n\
        ║  Any amount ≥ 5 ADA (5 000 000 lovelace) is sufficient for collateral.\n\
        ╚══════════════════════════════════════════╝\n"
    );
}

// =============================================================================
// Participant / session construction
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn make_participants(
    p1_keys: &SwapKeys,
    p2_keys: &SwapKeys,
    p3_keys: &SwapKeys,
    p4_keys: &SwapKeys,
    p1_btc: &BtcUtxo,
    p2_ada: &AdaUtxo,
    p3_btc: &BtcUtxo,
    p4_ada: &AdaUtxo,
    me_id: u8,
) -> Participants {
    let mut participants = BTreeMap::new();

    participants.insert(1, Participant {
        id: 1,
        blockchain: Blockchain::Bitcoin,
        tcp_address: P1_TCP_ADDR.to_string(),
        target_participant: 2,
        amount_locking: p1_btc.value - P1_BTC_LOCK_FEE,
        amount_claiming: p2_ada.value,
        is_me: me_id == 1,
        secp256k1_public_key: p1_keys.public_key.to_string(),
        cardano_wallet_public_key: p1_keys.cardano_wallet_public_key.clone(),
        funding_utxo_txid: p1_btc.txid.clone(),
        funding_utxo_vout: p1_btc.vout,
    });

    participants.insert(2, Participant {
        id: 2,
        blockchain: Blockchain::Cardano,
        tcp_address: P2_TCP_ADDR.to_string(),
        target_participant: 3,
        amount_locking: p2_ada.value,
        amount_claiming: p3_btc.value - P3_BTC_LOCK_FEE,
        is_me: me_id == 2,
        secp256k1_public_key: p2_keys.public_key.to_string(),
        cardano_wallet_public_key: p2_keys.cardano_wallet_public_key.clone(),
        funding_utxo_txid: p2_ada.txid.clone(),
        funding_utxo_vout: p2_ada.vout,
    });

    participants.insert(3, Participant {
        id: 3,
        blockchain: Blockchain::Bitcoin,
        tcp_address: P3_TCP_ADDR.to_string(),
        target_participant: 4,
        amount_locking: p3_btc.value - P3_BTC_LOCK_FEE,
        amount_claiming: p4_ada.value,
        is_me: me_id == 3,
        secp256k1_public_key: p3_keys.public_key.to_string(),
        cardano_wallet_public_key: p3_keys.cardano_wallet_public_key.clone(),
        funding_utxo_txid: p3_btc.txid.clone(),
        funding_utxo_vout: p3_btc.vout,
    });

    participants.insert(4, Participant {
        id: 4,
        blockchain: Blockchain::Cardano,
        tcp_address: P4_TCP_ADDR.to_string(),
        target_participant: 1,
        amount_locking: p4_ada.value,
        amount_claiming: p1_btc.value - P1_BTC_LOCK_FEE,
        is_me: me_id == 4,
        secp256k1_public_key: p4_keys.public_key.to_string(),
        cardano_wallet_public_key: p4_keys.cardano_wallet_public_key.clone(),
        funding_utxo_txid: p4_ada.txid.clone(),
        funding_utxo_vout: p4_ada.vout,
    });

    participants
}

fn make_config(tcp_address: &str) -> DaemonConfig {
    DaemonConfig {
        tcp_address: tcp_address.to_string(),
        bitcoin_network: BitcoinNetwork::Testnet4,
        cardano_network: CardanoNetwork::Preprod,
        blockfrost_api_key: blockfrost_api_key().to_string(),
        validate_utxos: true,
    }
}

// =============================================================================
// State polling helpers
// =============================================================================

async fn wait_for_state(daemon: &tokio::sync::RwLock<Daemon>, expected: SwapState, name: &str) {
    info!("waiting for state: {:?} ({name})", expected);
    timeout(Duration::from_secs(STATE_TRANSITION_TIMEOUT_SECS), async {
        loop {
            let guard = daemon.read().await;
            let session = guard.sessions.get(&1).unwrap();
            if session.state_history.contains(&expected) {
                info!("state reached: {:?} ✓ (current: {:?})", expected, session.state);
                return;
            }
            drop(guard);
            sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for state {:?} ({name})", expected));
}

async fn wait_for_completed(daemon: &tokio::sync::RwLock<Daemon>, name: &str) {
    info!("waiting for {name} to complete...");
    timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
        loop {
            let state = daemon.read().await.sessions.get(&1).unwrap().state;
            if state == SwapState::Completed {
                info!("{name} swap completed ✓");
                return;
            }
            info!("{name} current state: {:?}", state);
            sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for {name} to complete"));
}

async fn check_cardano_confirmed_blockfrost(txid: &str) -> bool {
    let url = format!("{BLOCKFROST_PREPROD_URL}/txs/{txid}");
    let client = reqwest::Client::new();
    matches!(
        client.get(&url).header("project_id", blockfrost_api_key()).send().await,
        Ok(r) if r.status().is_success()
    )
}

// =============================================================================
// The test
// =============================================================================

#[tokio::test]
#[ignore = "requires pre-funded wallets on Bitcoin Testnet4 and Cardano Preprod"]
async fn four_party_swap_preprod() {
    let p1_keys = make_bitcoin_keys(P1_SECRET_KEY);
    let p2_keys = make_cardano_keys(P2_CARDANO_SECRET_KEY);
    let p3_keys = make_bitcoin_keys(P3_SECRET_KEY);
    let p4_keys = make_cardano_keys(P4_CARDANO_SECRET_KEY);

    let p1_btc_addr    = bitcoin_testnet4_address(&p1_keys);
    let p1_ada_addr    = cardano_preprod_address(&p1_keys);  // collateral
    let p2_ada_addr    = cardano_preprod_address(&p2_keys);
    let p3_btc_addr    = bitcoin_testnet4_address(&p3_keys);
    let p3_ada_addr    = cardano_preprod_address(&p3_keys);  // collateral
    let p4_ada_addr    = cardano_preprod_address(&p4_keys);

    info!("=== Addresses ===");
    info!("P1 Bitcoin  (lock):       {p1_btc_addr}");
    info!("P1 Cardano  (collateral): {p1_ada_addr}");
    info!("P2 Cardano  (lock):       {p2_ada_addr}");
    info!("P3 Bitcoin  (lock):       {p3_btc_addr}");
    info!("P3 Cardano  (collateral): {p3_ada_addr}");
    info!("P4 Cardano  (lock):       {p4_ada_addr}");

    info!("=== Pre-flight: checking / funding addresses ===");
    ensure_btc_funded(&p1_btc_addr, "P1 Bitcoin").await;
    ensure_btc_funded(&p3_btc_addr, "P3 Bitcoin").await;
    ensure_ada_funded(&p2_ada_addr, "P2 Cardano (lock)").await;
    ensure_ada_funded(&p4_ada_addr, "P4 Cardano (lock)").await;
    ensure_ada_funded(&p1_ada_addr, "P1 Cardano (collateral)").await;
    ensure_ada_funded(&p3_ada_addr, "P3 Cardano (collateral)").await;

    info!("=== Fetching UTXOs ===");
    let p1_btc = fetch_largest_btc_utxo(&p1_btc_addr).await;
    let p2_ada = fetch_largest_ada_utxo(&p2_ada_addr).await;
    let p1_ada_collateral = fetch_smallest_ada_utxo(&p1_ada_addr).await;
    let p3_btc = fetch_largest_btc_utxo(&p3_btc_addr).await;
    let p4_ada = fetch_largest_ada_utxo(&p4_ada_addr).await;
    let p3_ada_collateral = fetch_smallest_ada_utxo(&p3_ada_addr).await;

    let collaterals = {
        let mut map = std::collections::HashMap::new();
        map.insert(2, CardanoCollateral {
            utxo_txid: p1_ada_collateral.txid.clone(),
            utxo_index: p1_ada_collateral.vout,
        });
        map.insert(4, CardanoCollateral {
            utxo_txid: p3_ada_collateral.txid.clone(),
            utxo_index: p3_ada_collateral.vout,
        });
        map
    };

    info!("=== Initializing daemons ===");
    let make_session = |me_id: u8| {
        let mut session = SwapSession::new(
            1,
            make_participants(
                &p1_keys, &p2_keys, &p3_keys, &p4_keys,
                &p1_btc, &p2_ada, &p3_btc, &p4_ada,
                me_id,
            ),
            0,
            0,
            5_000,
            2_000_000,
        );
        session.cardano_collaterals = collaterals.clone();
        session
    };

    let d1 = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(p1_keys.clone(), make_config(P1_TCP_ADDR));
        daemon.insert_session(make_session(1));
        daemon
    }));
    let d2 = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(p2_keys.clone(), make_config(P2_TCP_ADDR));
        daemon.insert_session(make_session(2));
        daemon
    }));
    let d3 = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(p3_keys.clone(), make_config(P3_TCP_ADDR));
        daemon.insert_session(make_session(3));
        daemon
    }));
    let d4 = Arc::new(tokio::sync::RwLock::new({
        let mut daemon = Daemon::new(p4_keys.clone(), make_config(P4_TCP_ADDR));
        daemon.insert_session(make_session(4));
        daemon
    }));

    info!("=== Starting TCP listeners ===");
    for d in [d1.clone(), d2.clone(), d3.clone(), d4.clone()] {
        tokio::spawn(async move { swap_daemon::types::Daemon::run_shared(d).await.ok(); });
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
    info!("leader elected: participant {leader} ✓");

    info!("=== Waiting for refund + spend tx signing ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::Funding, name).await;
    }

    info!("=== Waiting for lock tx confirmations ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_state(d, SwapState::AwaitingLockConfirmations, name).await;
    }
    timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
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
    })
    .await
    .unwrap_or_else(|_| panic!("timeout waiting for lock tx confirmations"));

    info!("=== Waiting for non-leaders to observe leader's spend tx ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        let my_id = d.read().await.sessions.get(&1).unwrap()
            .participants.values().find(|p| p.is_me).unwrap().id;
        if my_id != leader {
            wait_for_state(d, SwapState::AwaitingLeaderSpend, name).await;
        }
    }

    info!("=== Waiting for swap completion ===");
    for (d, name) in [(&d1, "d1"), (&d2, "d2"), (&d3, "d3"), (&d4, "d4")] {
        wait_for_completed(d, name).await;
    }

    info!("\n=== Swap completed successfully! ===");

    let cardano_spend_p2_hex = d1.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(1))
        .expect("Cardano spend tx (P1 claims P2 lock) must be in d1").clone();
    let btc_spend_p3_hex = d2.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(2))
        .expect("Bitcoin spend tx (P2 claims P3 lock) must be in d2").clone();
    let cardano_spend_p4_hex = d3.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(3))
        .expect("Cardano spend tx (P3 claims P4 lock) must be in d3").clone();
    let btc_spend_p1_hex = d4.read().await.sessions.get(&1).unwrap()
        .signed_txs.get(&TxRole::Spend(4))
        .expect("Bitcoin spend tx (P4 claims P1 lock) must be in d4").clone();

    info!("=== Verifying Cardano spend txs confirmed on Preprod ===");
    for (label, hex) in [
        ("P1 claims P2 lock", &cardano_spend_p2_hex),
        ("P3 claims P4 lock", &cardano_spend_p4_hex),
    ] {
        let txid = cardano_txid(hex);
        info!("Cardano spend ({label}) txid: {txid}");
        timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
            loop {
                if check_cardano_confirmed_blockfrost(&txid).await {
                    info!("Cardano spend tx ({label}) confirmed ✓");
                    return;
                }
                sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timeout: Cardano spend tx ({label}) {txid} not confirmed"));
    }

    info!("=== Verifying Bitcoin spend txs confirmed on Testnet4 ===");
    for (label, hex) in [
        ("P2 claims P3 lock", &btc_spend_p3_hex),
        ("P4 claims P1 lock", &btc_spend_p1_hex),
    ] {
        timeout(Duration::from_secs(CHAIN_CONFIRMATION_TIMEOUT_SECS), async {
            loop {
                if check_bitcoin_confirmed(hex, MEMPOOL_TESTNET4_URL).await {
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
}
