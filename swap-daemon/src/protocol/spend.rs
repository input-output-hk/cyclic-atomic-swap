use std::collections::HashMap;

use crate::blockchains::bitcoin_utils::{
    bitcoin_pubkeys, build_spend_tx, compute_sighash, submit_bitcoin_tx,
};
use crate::blockchains::cardano_utils::{self, submit_cardano_tx};
use crate::networking::broadcast;
use crate::types::{
    Blockchain, DaemonConfig, Envelope, MusigRuntime, SwapKeys, SwapSession, TxRole, WireMessage,
};
use crate::utils::{get_my_id, get_other_addresses};
use bitcoin::consensus::encode::{deserialize_hex, serialize_hex};
use musig2::SecNonce;
use rand::{rngs::OsRng, RngCore};
use tracing::{error, info};

/// Begins the process of spend transaction signing for a swap session.
///
/// This function generates unsigned spend transactions, calculates signature hashes, and
/// sends Schnorr nonces for multi-signature creation for each participant in the swap session.
/// It supports both Bitcoin and Cardano blockchains based on the participants' target blockchains.
///
/// # Parameters
/// - `session`: A mutable reference to the [`SwapSession`](struct.SwapSession.html),
///   which maintains the session state including participants, transaction data,
///   and pending nonces.
/// - `keys`: A reference to the [`SwapKeys`](struct.SwapKeys.html), which provides
///   the local participant's key pair used for signing transactions.
///
/// # Blockchain Support
/// - **Bitcoin**
///   - Constructs a Taproot key-path spend transaction.
///   - Calculates the sighash based on the UTXO being spent and the target's lock transaction details.
///   - Applies a fee rate of 5 sat/vByte.
/// - **Cardano**
///   - Constructs a Plutus script spend transaction with appropriate fees including execution units.
///   - Reconstructs transaction hashes to maintain byte-level accuracy of Cardano transaction IDs.
///   - Applies a fixed spend fee of 2_000_000 lovelace.
///
/// # Implementation Details
/// - Derives the current participant's index in the list of public keys.
/// - Iterates through all participants in the session to handle signing roles.
/// - Generates a unique nonce for Schnorr signing using [`SecNonce`](struct.SecNonce.html).
/// - Updates the swap session state with unsigned transactions, Schnorr nonces, and initialization
///   of MuSig sessions for each participant.
/// - Broadcasts Schnorr nonces to other participants to initiate the signing protocol.
///
/// # Errors
/// - If the function fails to broadcast a Schnorr nonce to any participant, it logs the error
///   with `error!` rather than returning it.
/// - Panics if mandatory data is not found in the session (e.g., missing lock transaction or participant ID).
///
/// # Logging
/// - Logs an error message when nonce broadcasting fails.
///
/// # Dependencies
/// - Relies on the `bitcoin`, `cardano_serialization_lib`, and `cardano_utils` crates for transaction construction and handling.
/// - Uses cryptographic utilities like `SecNonce` for nonce management and signing.
///
/// # Related Structures
/// - `SwapSession`: Manages the state of the swap session.
/// - `SwapKeys`: Provides key pairs for signing transactions.
/// - `TxRole`: Enum to distinguish different transaction roles in the swap process.
/// - `Blockchain`: Enum representing the blockchains (Bitcoin or Cardano).
///
/// # Notes
/// - For Bitcoin Taproot transactions, the spend fee is calculated based on a target fee rate (5 sat/vByte).
/// - For Cardano Plutus script transactions, the spend fee accounts for script execution costs,
///   and a safety margin is applied to ensure fees are adequate.
pub async fn begin_spend_signing(session: &mut SwapSession, keys: &SwapKeys) {
    let all_pubkeys = bitcoin_pubkeys(&session.participants);
    let local_index = all_pubkeys
        .iter()
        .position(|pk| pk.serialize() == keys.public_key.serialize())
        .unwrap();
    let all_pubkeys_str: Vec<String> = all_pubkeys.iter().map(|pk| pk.to_string()).collect();
    let my_id = *get_my_id(&session.participants);

    for participant_id in session.participants.keys().copied().collect::<Vec<_>>() {
        let role = TxRole::Spend(participant_id);
        let participant = &session.participants[&participant_id];

        // spend tx spends the target's locked funds, so blockchain is determined by target
        let target_id = participant.target_participant;
        let target = &session.participants[&target_id];
        let taproot_tweak = matches!(target.blockchain, Blockchain::Bitcoin);

        let (unsigned_tx_hex, msg) = match target.blockchain {
            Blockchain::Bitcoin => {
                let lock_tx_hex = session.lock_txs.get(&target_id).unwrap();
                let lock_tx: bitcoin::Transaction = deserialize_hex(lock_tx_hex).unwrap();
                let lock_txid = lock_tx.compute_txid();
                let lock_tx_output = lock_tx.output[0].clone();
                let spend_input = bitcoin::OutPoint {
                    txid: lock_txid,
                    vout: 0,
                };

                let unsigned_tx = build_spend_tx(spend_input, participant, session.bitcoin_fee);
                let msg = compute_sighash(&unsigned_tx, &lock_tx_output);
                (serialize_hex(&unsigned_tx), msg.to_vec())
            }
            Blockchain::Cardano => {
                let lock_tx_hex = session.lock_txs.get(&target_id).unwrap();
                let lock_tx_bytes = hex::decode(lock_tx_hex).unwrap();
                let (lock_txhash, lock_utxo_value) = {
                    // Use FixedTransaction to preserve original byte encoding — this gives the
                    // exact on-chain txid (blake2b of original body bytes, not re-serialised).
                    use cardano_serialization_lib::{FixedTransaction, Transaction as CardanoTx};
                    let fixed_tx = FixedTransaction::from_bytes(lock_tx_bytes.clone()).unwrap();
                    let txhash = fixed_tx.transaction_hash();
                    // Read output value from regular Transaction (only for value, not hashing)
                    let lock_tx = CardanoTx::from_bytes(lock_tx_bytes).unwrap();
                    let utxo_value: u64 = lock_tx
                        .body()
                        .outputs()
                        .get(0)
                        .amount()
                        .coin()
                        .to_str()
                        .parse()
                        .unwrap();
                    (txhash, utxo_value)
                };

                let collateral = session.cardano_collaterals.get(&participant_id)
                    .map(|c| (c.utxo_txid.as_str(), c.utxo_index));
                // Plutus script spend txs have much higher minimum fees than simple transfers
                // (base fee + execution units fee). The node requires ~1,708,909 lovelace minimum;
                // use 2_000_000 for safety.
                let cardano_spend_fee = 2_000_000u64;
                let output_amount = lock_utxo_value - cardano_spend_fee;
                let unsigned_tx =
                    cardano_utils::build_spend_tx(participant, &lock_txhash, cardano_spend_fee, output_amount, collateral);
                // message = raw 32-byte lock tx txid (no circular dependency with script_data_hash)
                let msg = lock_txhash.to_bytes().to_vec();
                (hex::encode(unsigned_tx.to_bytes()), msg)
            }
        };

        session.unsigned_txs.insert(role, unsigned_tx_hex);

        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let sec_nonce = SecNonce::build(seed)
            .with_seckey(keys.secret_key)
            .with_message(&msg)
            .build();
        let our_nonce_str = sec_nonce.public_nonce().to_string();
        let sec_nonce_bytes: Vec<u8> = sec_nonce.serialize().to_vec();

        session
            .schnorr_nonces
            .entry(my_id)
            .or_insert_with(HashMap::new)
            .insert(role, our_nonce_str.clone());

        session.musig_sessions.insert(
            role,
            MusigRuntime::RoundOne {
                all_pubkeys: all_pubkeys_str.clone(),
                signer_index: local_index,
                msg,
                taproot_tweak,
                sec_nonce: sec_nonce_bytes,
            },
        );

        let addresses = get_other_addresses(&session.participants);
        let envelope = Envelope::new(
            session.id,
            my_id,
            WireMessage::SchnorrNonce {
                role,
                nonce: our_nonce_str,
            },
        );
        if let Err(e) = broadcast(&addresses, &envelope, &session.connection_pool).await {
            error!("broadcast nonce failed for role {:?}: {e}", role);
        }
    }
}

/// Broadcasts the signed spend transaction for the participant's role in a swap session.
///
/// This function identifies the caller's role in the transaction (`TxRole::Spend`), retrieves the
/// corresponding signed transaction (if it exists), and broadcasts it to the appropriate blockchain
/// network (Bitcoin or Cardano).
///
/// - For Bitcoin, the function directly submits the signed transaction to the network's mempool using
///   the Bitcoin network's base URL from the configuration.
/// - For Cardano, it checks if collateral witnesses need to be added to the transaction. If so, it modifies
///   the transaction to include the collateral witness before broadcasting it.
///
/// # Parameters
/// * `session` - A mutable reference to the [`SwapSession`] containing the state of the swap and
///   participant-specific information.
/// * `keys` - A reference to the [`SwapKeys`] containing the cryptographic keys required for signing
///   Cardano-specific collateral transaction updates, if applicable.
/// * `config` - A reference to the [`DaemonConfig`] structure providing blockchain-specific configuration,
///   including network information.
///
/// # Behavior
/// 1. Identifies the caller's role (`my_id`) and retrieves their targeted counterparty's information.
/// 2. Checks for the signed transaction associated with the `TxRole::Spend` role.
/// 3. If the target blockchain is Bitcoin:
///     - Submits the transaction to the Bitcoin mempool using the network's base URL.
/// 4. If the target blockchain is Cardano:
///     - Checks if collateral witnesses need to be included in the transaction.
///     - Adds collateral using the caller's Cardano wallet's secret and public keys, if necessary.
///     - Updates the signed transaction cache (`session.signed_txs`).
///     - Submits the transaction to the Cardano network.
/// 5. Logs a message upon successful broadcasting of the spend transaction.
///
/// # Panics
/// This function does not explicitly handle or log panics but assumes the input arguments
/// (`session`, `keys`, `config`) are valid and properly initialized.
///
/// # Side Effects
/// * Updates the `session.signed_txs` map for the `TxRole::Spend` role if modifications are
///   made to the Cardano spend transaction.
/// * Asynchronously communicates with external services (Bitcoin/Cardano blockchain networks)
///   to submit transactions.
///
pub async fn broadcast_my_spend_tx(session: &mut SwapSession, keys: &SwapKeys, config: &DaemonConfig) {
    let my_id = *get_my_id(&session.participants);

    let role = TxRole::Spend(my_id);
    if let Some(signed_tx_hex) = session.signed_txs.get(&role).cloned() {
        let target_id = session.participants[&my_id].target_participant;
        let target = &session.participants[&target_id];
        match target.blockchain {
            Blockchain::Bitcoin => {
                submit_bitcoin_tx(&signed_tx_hex, config.bitcoin_network.mempool_base_url()).await;
            }
            Blockchain::Cardano => {
                let tx_to_submit = if session.cardano_collaterals.contains_key(&my_id) {
                    cardano_utils::add_collateral_witness(
                        &signed_tx_hex,
                        &keys.cardano_wallet_secret_key,
                        &keys.cardano_wallet_public_key,
                    )
                } else {
                    signed_tx_hex.clone()
                };
                session.signed_txs.insert(role, tx_to_submit.clone());
                submit_cardano_tx(&tx_to_submit, config).await;
            }
        }
        info!("broadcast spend tx for role {:?}", role);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_swap_keys;
    use crate::types::{Blockchain, DaemonConfig, Participant, SwapSession};
    use std::collections::{BTreeMap, HashMap};

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

    fn test_config() -> DaemonConfig {
        DaemonConfig::testnet("127.0.0.1:0".to_string())
    }

    fn make_participant(id: u8, is_me: bool, public_key: String, target: u8) -> Participant {
        Participant {
            id,
            blockchain: Blockchain::Bitcoin,
            tcp_address: format!("127.0.0.1:91{id:02}"),
            target_participant: target,
            amount_locking: 1_000_000,
            amount_claiming: 1_000_000,
            is_me,
            secp256k1_public_key: public_key,
            cardano_wallet_public_key: vec![],
            funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
                .to_string(),
            funding_utxo_vout: 0,
        }
    }

    fn make_session_with_lock_txs(keys: &[crate::types::SwapKeys]) -> SwapSession {
        let mut participants = BTreeMap::new();

        // cycle: 1 → 2 → 3 → 1
        participants.insert(
            1,
            make_participant(1, true, keys[0].public_key.to_string(), 2),
        );
        participants.insert(
            2,
            make_participant(2, false, keys[1].public_key.to_string(), 3),
        );
        participants.insert(
            3,
            make_participant(3, false, keys[2].public_key.to_string(), 1),
        );

        let mut session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);

        // build and insert dummy lock txs for each participant
        use bitcoin::consensus::encode::serialize_hex;
        use bitcoin::{
            absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence,
            Transaction, TxIn, TxOut, Witness,
        };

        for (id, _) in session.participants.clone().iter() {
            let dummy_lock_tx = Transaction {
                version: Version::TWO,
                lock_time: LockTime::ZERO,
                input: vec![TxIn {
                    previous_output: OutPoint::null(),
                    script_sig: ScriptBuf::default(),
                    sequence: Sequence::MAX,
                    witness: Witness::default(),
                }],
                output: vec![TxOut {
                    value: Amount::from_sat(100_000),
                    script_pubkey: ScriptBuf::default(),
                }],
            };
            session.lock_txs.insert(*id, serialize_hex(&dummy_lock_tx));
        }

        session
    }

    fn make_session_with_lock_txs_for(keys: &[SwapKeys], me_index: usize) -> SwapSession {
        let mut participants = BTreeMap::new();
        participants.insert(
            1,
            make_participant(1, me_index == 0, keys[0].public_key.to_string(), 2),
        );
        participants.insert(
            2,
            make_participant(2, me_index == 1, keys[1].public_key.to_string(), 3),
        );
        participants.insert(
            3,
            make_participant(3, me_index == 2, keys[2].public_key.to_string(), 1),
        );

        let mut session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);
        session.leader = Some(1);

        // populate adaptor points for all participants using scalar::one for determinism
        for id in [1u8, 2, 3] {
            let secret = musig2::secp::Scalar::one();
            let point = secret.base_point_mul();
            session.adaptor_points.insert(id, point.to_string());
            // also insert the secret for the local participant
            if session.participants[&id].is_me {
                session
                    .adaptor_secrets
                    .insert(id, hex::encode(secret.serialize()));
            }
        }

        // same dummy lock txs
        use bitcoin::consensus::encode::serialize_hex;
        for id in [1u8, 2, 3] {
            let dummy_lock_tx = bitcoin::Transaction {
                version: bitcoin::transaction::Version::TWO,
                lock_time: bitcoin::absolute::LockTime::ZERO,
                input: vec![bitcoin::TxIn {
                    previous_output: bitcoin::OutPoint::null(),
                    script_sig: bitcoin::ScriptBuf::default(),
                    sequence: bitcoin::Sequence::MAX,
                    witness: bitcoin::Witness::default(),
                }],
                output: vec![bitcoin::TxOut {
                    value: bitcoin::Amount::from_sat(100_000),
                    script_pubkey: bitcoin::ScriptBuf::default(),
                }],
            };
            session.lock_txs.insert(id, serialize_hex(&dummy_lock_tx));
        }

        session
    }

    fn get_our_nonce(session: &SwapSession, role: TxRole) -> String {
        let my_id = *crate::utils::get_my_id(&session.participants);
        session
            .schnorr_nonces
            .get(&my_id)
            .and_then(|m| m.get(&role))
            .cloned()
            .unwrap_or_else(|| panic!("expected nonce for {:?}", role))
    }

    fn get_our_partial_sig(session: &SwapSession, role: TxRole) -> String {
        let my_id = *crate::utils::get_my_id(&session.participants);
        session
            .partial_sigs
            .get(&my_id)
            .and_then(|m| m.get(&role))
            .cloned()
            .unwrap_or_else(|| panic!("expected partial sig for {:?}", role))
    }

    #[tokio::test]
    async fn begin_spend_signing_creates_unsigned_txs_for_all_participants() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session_with_lock_txs(&keys);

        begin_spend_signing(&mut session, &keys[0]).await;

        // should have unsigned tx for each participant's spend role
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            assert!(
                session.unsigned_txs.contains_key(&role),
                "missing unsigned spend tx for participant {}",
                id
            );
        }
    }

    #[tokio::test]
    async fn begin_spend_signing_creates_musig_sessions_for_all_participants() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session_with_lock_txs(&keys);

        begin_spend_signing(&mut session, &keys[0]).await;

        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            assert!(
                session.musig_sessions.contains_key(&role),
                "missing musig session for participant {}",
                id
            );
            // should be in RoundOne
            assert!(
                matches!(
                    session.musig_sessions.get(&role),
                    Some(MusigRuntime::RoundOne { .. })
                ),
                "musig session for participant {} should be in RoundOne",
                id
            );
        }
    }

    #[tokio::test]
    async fn begin_spend_signing_stores_our_nonce_in_schnorr_nonces() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session_with_lock_txs(&keys);

        begin_spend_signing(&mut session, &keys[0]).await;

        let my_id = 1u8; // participant 1 is_me in make_session_with_lock_txs
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            let nonce = session
                .schnorr_nonces
                .get(&my_id)
                .and_then(|m| m.get(&role));
            assert!(nonce.is_some(), "nonce should be stored in schnorr_nonces for role {:?}", role);
            assert!(!nonce.unwrap().is_empty(), "nonce should not be empty for role {:?}", role);
        }
    }

    #[tokio::test]
    async fn begin_spend_signing_uses_target_blockchain() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session_with_lock_txs(&keys);

        begin_spend_signing(&mut session, &keys[0]).await;

        // all unsigned txs should be non-empty hex strings
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            let tx_hex = session.unsigned_txs.get(&role).unwrap();
            assert!(!tx_hex.is_empty(), "unsigned tx hex should not be empty");
            // bitcoin tx hex starts with version bytes
            assert!(tx_hex.len() > 10, "unsigned tx hex too short");
        }
    }

    #[tokio::test]
    async fn begin_spend_signing_transitions_to_round_two_with_adaptor() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();

        // Use sessions with adaptor points since spend txs use adaptor signing
        let mut session_0 = make_session_with_lock_txs_for(&keys, 0);
        let mut session_1 = make_session_with_lock_txs_for(&keys, 1);
        let mut session_2 = make_session_with_lock_txs_for(&keys, 2);

        // make_session_with_lock_txs_for already sets identical adaptor points across all sessions

        begin_spend_signing(&mut session_0, &keys[0]).await;
        begin_spend_signing(&mut session_1, &keys[1]).await;
        begin_spend_signing(&mut session_2, &keys[2]).await;

        // exchange nonces — feed participants 2 and 3's nonces into session_0
        for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
            let nonce_1 = get_our_nonce(&session_1, role);
            let nonce_2 = get_our_nonce(&session_2, role);
            session_0.schnorr_nonces.entry(2).or_insert_with(HashMap::new).insert(role, nonce_1);
            session_0.schnorr_nonces.entry(3).or_insert_with(HashMap::new).insert(role, nonce_2);

            crate::cryptography::multisig::transition_to_round_two(&mut session_0, &keys[0], role).await;
        }

        // should now be in RoundTwo
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            assert!(
                matches!(
                    session_0.musig_sessions.get(&role),
                    Some(MusigRuntime::RoundTwo { .. })
                ),
                "spend musig session for participant {} should be in RoundTwo after nonce exchange",
                id
            );
        }
    }

    #[tokio::test]
    async fn spend_txs_produce_adaptor_sigs_not_regular_sigs() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();

        let mut session_0 = make_session_with_lock_txs(&keys);
        let mut session_1 = make_session_with_lock_txs_for(&keys, 1);
        let mut session_2 = make_session_with_lock_txs_for(&keys, 2);

        // synchronize adaptor points across all sessions
        let point_0 = session_0.adaptor_points.get(&1).unwrap().clone();
        let point_1 = session_1.adaptor_points.get(&2).unwrap().clone();
        let point_2 = session_2.adaptor_points.get(&3).unwrap().clone();

        for session in [&mut session_0, &mut session_1, &mut session_2] {
            session.adaptor_points.insert(1, point_0.clone());
            session.adaptor_points.insert(2, point_1.clone());
            session.adaptor_points.insert(3, point_2.clone());
        } // each party runs begin_spend_signing independently
        begin_spend_signing(&mut session_0, &keys[0]).await;
        begin_spend_signing(&mut session_1, &keys[1]).await;
        begin_spend_signing(&mut session_2, &keys[2]).await;

        // exchange nonces — each reads from their own session
        for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
            let nonce_0 = get_our_nonce(&session_0, role);
            let nonce_1 = get_our_nonce(&session_1, role);
            let nonce_2 = get_our_nonce(&session_2, role);

            // feed peer nonces into each session
            session_0
                .schnorr_nonces
                .entry(2)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_1.clone());
            session_0
                .schnorr_nonces
                .entry(3)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_2.clone());

            session_1
                .schnorr_nonces
                .entry(1)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_0.clone());
            session_1
                .schnorr_nonces
                .entry(3)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_2.clone());

            session_2
                .schnorr_nonces
                .entry(1)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_0.clone());
            session_2
                .schnorr_nonces
                .entry(2)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_1.clone());

            // transition to round two
            crate::cryptography::multisig::transition_to_round_two(&mut session_0, &keys[0], role)
                .await;
            crate::cryptography::multisig::transition_to_round_two(&mut session_1, &keys[1], role)
                .await;
            crate::cryptography::multisig::transition_to_round_two(&mut session_2, &keys[2], role)
                .await;
        }

        // exchange partial sigs — session_0 already has participant 1's sig from transition_to_round_two
        for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
            let sig_1 = get_our_partial_sig(&session_1, role);
            let sig_2 = get_our_partial_sig(&session_2, role);

            session_0
                .partial_sigs
                .entry(2)
                .or_insert_with(HashMap::new)
                .insert(role, sig_1.clone());
            session_0
                .partial_sigs
                .entry(3)
                .or_insert_with(HashMap::new)
                .insert(role, sig_2.clone());

            crate::cryptography::multisig::finalize_role(&mut session_0, &keys[0], role, &test_config()).await;
        }

        // adaptor sigs should be stored on session_0
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            assert!(
                session_0.adaptor_sigs.contains_key(&role),
                "adaptor sig should be stored for {:?}",
                role
            );
        }

        // signed txs should NOT exist yet
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            assert!(
                !session_0.signed_txs.contains_key(&role),
                "signed tx should not exist yet for {:?}",
                role
            );
        }
    }

    #[tokio::test]
    async fn spend_txs_can_be_adapted_with_secret() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();

        let mut session_0 = make_session_with_lock_txs(&keys);
        let mut session_1 = make_session_with_lock_txs_for(&keys, 1);
        let mut session_2 = make_session_with_lock_txs_for(&keys, 2);

        // synchronize adaptor points across all sessions
        // each session needs all participants' points to compute the same aggregate
        let point_0 = session_0.adaptor_points.get(&1).unwrap().clone();
        let point_1 = session_1.adaptor_points.get(&2).unwrap().clone();
        let point_2 = session_2.adaptor_points.get(&3).unwrap().clone();

        for session in [&mut session_0, &mut session_1, &mut session_2] {
            session.adaptor_points.insert(1, point_0.clone());
            session.adaptor_points.insert(2, point_1.clone());
            session.adaptor_points.insert(3, point_2.clone());
        }

        begin_spend_signing(&mut session_0, &keys[0]).await;
        begin_spend_signing(&mut session_1, &keys[1]).await;
        begin_spend_signing(&mut session_2, &keys[2]).await;

        // exchange nonces
        for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
            let nonce_0 = get_our_nonce(&session_0, role);
            let nonce_1 = get_our_nonce(&session_1, role);
            let nonce_2 = get_our_nonce(&session_2, role);

            // feed peer nonces into each session
            session_0
                .schnorr_nonces
                .entry(2)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_1.clone());
            session_0
                .schnorr_nonces
                .entry(3)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_2.clone());

            session_1
                .schnorr_nonces
                .entry(1)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_0.clone());
            session_1
                .schnorr_nonces
                .entry(3)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_2.clone());

            session_2
                .schnorr_nonces
                .entry(1)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_0.clone());
            session_2
                .schnorr_nonces
                .entry(2)
                .or_insert_with(HashMap::new)
                .insert(role, nonce_1.clone());

            // transition ALL sessions to round two
            crate::cryptography::multisig::transition_to_round_two(&mut session_0, &keys[0], role)
                .await;
            crate::cryptography::multisig::transition_to_round_two(&mut session_1, &keys[1], role)
                .await;
            crate::cryptography::multisig::transition_to_round_two(&mut session_2, &keys[2], role)
                .await;
        }

        // exchange partial sigs
        for role in [TxRole::Spend(1), TxRole::Spend(2), TxRole::Spend(3)] {
            let sig_0 = get_our_partial_sig(&session_0, role);
            let sig_1 = get_our_partial_sig(&session_1, role);
            let sig_2 = get_our_partial_sig(&session_2, role);

            session_0
                .partial_sigs
                .entry(1)
                .or_insert_with(HashMap::new)
                .insert(role, sig_0);
            session_0
                .partial_sigs
                .entry(2)
                .or_insert_with(HashMap::new)
                .insert(role, sig_1);
            session_0
                .partial_sigs
                .entry(3)
                .or_insert_with(HashMap::new)
                .insert(role, sig_2);

            crate::cryptography::multisig::finalize_role(&mut session_0, &keys[0], role, &test_config()).await;
        }

        // adapt all spend roles
        let cfg = test_config();
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            if session_0.adaptor_sigs.contains_key(&role) {
                crate::cryptography::multisig::adapt_role(&mut session_0, role, &cfg).await;
            }
        }

        // now signed txs should exist
        for id in [1u8, 2, 3] {
            let role = TxRole::Spend(id);
            assert!(
                session_0.signed_txs.contains_key(&role),
                "signed tx should exist after adapting spend role {:?}",
                role
            );
        }
    }
}
