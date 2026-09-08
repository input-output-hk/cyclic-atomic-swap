use std::collections::BTreeMap;
use std::sync::Once;

use rand::rngs::OsRng;
use rand::RngCore;
use swap_daemon::types::{
    Blockchain, Daemon, DaemonConfig, Participant, Participants, SwapSession, TxRole,
};

static TRACING: Once = Once::new();

pub fn init_tracing() {
    TRACING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .init();
    });
}

pub fn make_keys(sk: musig2::secp256k1::SecretKey) -> swap_daemon::types::SwapKeys {
    let public_key =
        musig2::secp256k1::PublicKey::from_secret_key(&musig2::secp256k1::Secp256k1::new(), &sk);
    let mut ed25519_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut ed25519_bytes);
    let ed25519_signing_key = ed25519_dalek::SigningKey::from_bytes(&ed25519_bytes);
    let ed25519_verifying_key = ed25519_signing_key.verifying_key();

    swap_daemon::types::SwapKeys {
        secret_key: sk,
        public_key,
        cardano_wallet_secret_key: ed25519_signing_key.to_bytes().to_vec(),
        cardano_wallet_public_key: ed25519_verifying_key.to_bytes().to_vec(),
    }
}

pub fn make_participant(id: u8, is_me: bool, public_key: String) -> Participant {
    Participant {
        id,
        blockchain: Blockchain::Bitcoin,
        tcp_address: format!("127.0.0.1:91{id:02}"),
        target_participant: if id == 3 { 1 } else { id + 1 },
        amount_locking: 10_000_000,
        amount_claiming: 10_000_000,
        is_me,
        secp256k1_public_key: public_key,
        cardano_wallet_public_key: vec![],
        funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
            .to_string(),
        funding_utxo_vout: 0,
    }
}

pub fn generate_keypair() -> (musig2::secp256k1::SecretKey, String) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let secret_key = musig2::secp256k1::SecretKey::from_slice(&bytes).unwrap();
    let public_key = musig2::secp256k1::PublicKey::from_secret_key(
        &musig2::secp256k1::Secp256k1::new(),
        &secret_key,
    );
    (secret_key, public_key.to_string())
}

pub fn make_participants(me_id: u8, pk1: String, pk2: String, pk3: String) -> Participants {
    let mut participants = BTreeMap::new();
    participants.insert(1, make_participant(1, me_id == 1, pk1));
    participants.insert(2, make_participant(2, me_id == 2, pk2));
    participants.insert(3, make_participant(3, me_id == 3, pk3));
    participants
}

pub fn make_daemon(
    me_id: u8,
    addr: &str,
    pk1: String,
    pk2: String,
    pk3: String,
    swap_keys: swap_daemon::types::SwapKeys,
) -> Daemon {
    let config = DaemonConfig { validate_utxos: false, ..DaemonConfig::testnet(addr.to_string()) };
    let mut daemon = Daemon::new(swap_keys, config);
    let session = SwapSession::new(1, make_participants(me_id, pk1, pk2, pk3), 0, 0, 5_000, 2_000_000);
    daemon.insert_session(session);
    daemon
}

pub fn own_commitment(daemon: &Daemon, my_id: u8) -> [u8; 32] {
    *daemon
        .sessions
        .get(&1)
        .unwrap()
        .leader_commitments
        .get(&my_id)
        .expect("no commitment found")
}

pub fn own_nonce(daemon: &Daemon, my_id: u8) -> [u8; 32] {
    *daemon
        .sessions
        .get(&1)
        .unwrap()
        .leader_nonces
        .get(&my_id)
        .expect("no nonce found")
}

pub fn schnorr_nonce_for(daemon: &Daemon, sender_id: u8, role: TxRole) -> String {
    daemon
        .sessions
        .get(&1)
        .unwrap()
        .schnorr_nonces
        .get(&sender_id)
        .and_then(|m| m.get(&role))
        .expect("no schnorr nonce found")
        .clone()
}

pub fn partial_sig_for(daemon: &Daemon, sender_id: u8, role: TxRole) -> String {
    daemon
        .sessions
        .get(&1)
        .unwrap()
        .partial_sigs
        .get(&sender_id)
        .and_then(|m| m.get(&role))
        .expect("no partial sig found")
        .clone()
}

pub fn has_signed_refund_tx(daemon: &Daemon, role: TxRole) -> bool {
    daemon
        .sessions
        .get(&1)
        .unwrap()
        .signed_txs
        .contains_key(&role)
}

pub fn leader_id(daemon: &Daemon) -> u8 {
    daemon
        .sessions
        .get(&1)
        .unwrap()
        .leader
        .expect("no leader computed")
}

/// Fetches the Cardano network start time (POSIX seconds) from the indexer's
/// `/genesis` endpoint (Dolos and Blockfrost both expose `system_start`).
///
/// Regtest sessions must set `SwapSession::cardano_system_start_secs` from this
/// before building lock txs: the datum's refund deadline is POSIX milliseconds,
/// derived from the network start, and
/// `swap_daemon::utils::refund_deadline_posix_ms` panics if it is left unset.
pub async fn get_cardano_system_start(dolos_rest_url: &str) -> u64 {
    let url = format!("{dolos_rest_url}/genesis");
    let v: serde_json::Value = reqwest::get(&url)
        .await
        .expect("dolos /genesis request failed")
        .json()
        .await
        .expect("invalid JSON from dolos /genesis");
    let system_start = v["system_start"]
        .as_u64()
        .expect("dolos /genesis has no numeric system_start");
    assert!(system_start > 0, "dolos reported system_start = 0");
    system_start
}
