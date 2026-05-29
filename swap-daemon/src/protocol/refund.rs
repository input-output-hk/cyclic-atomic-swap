use std::collections::HashMap;

use blake2::{digest::consts::U32, Blake2b, Digest};
use crate::blockchains::bitcoin_utils::{bitcoin_pubkeys, compute_sighash, submit_bitcoin_tx};
use crate::blockchains::{bitcoin_utils, cardano_utils};
use crate::blockchains::cardano_utils::submit_cardano_tx;
use crate::networking::broadcast;
use crate::transport::tcp::{TcpTransport, TcpConnector};
use crate::types::{
    Blockchain, DaemonConfig, Envelope, MusigRuntime, SwapKeys, SwapSession, TxRole, WireMessage,
};
use crate::utils::{get_my_id, get_other_addresses, refund_locktime_btc, refund_locktime_cardano};
use bitcoin::consensus::encode::{deserialize_hex, serialize_hex};
use cardano_serialization_lib::FixedTransaction;
use musig2::SecNonce;
use rand::{rngs::OsRng, RngCore};
use tracing::{error, info};

/// Initiates the refund signing process for a swap session.
///
/// This function facilitates the generation and sharing of Schnorr nonces
/// for participants involved in the multisig signing process of refund
/// transactions. Each participant needs to sign the refund transaction for every
/// participant to enable refunding during the agreed time window.
///
/// # Parameters
///
/// * `session` - Mutable reference to the `SwapSession` containing the swap details.
/// * `keys` - Reference to the `SwapKeys` of the local participant containing
///    their cryptographic key pair.
///
/// # Notes
///
/// - Refund transactions vary based on the blockchain type (Bitcoin or Cardano) due to differences
///   in transaction structures and signing requirements.
/// - Handles cryptographic operations such as computing sighashes, generating nonces, and initializing
///   multi-signature (MuSig) sessions.
/// - Ensures secure selective broadcasting of Schnorr nonces to other participants.
///
/// # Error Handling
///
/// If broadcasting the Schnorr nonce to participants fails, an error is logged with details
/// about the failure and the associated signing role.
///
pub async fn begin_refund_signing(session: &mut SwapSession, keys: &SwapKeys) {
    let all_pubkeys = bitcoin_pubkeys(&session.participants);
    let local_index = all_pubkeys
        .iter()
        .position(|pk| pk.serialize() == keys.public_key.serialize())
        .unwrap();
    let all_pubkeys_str: Vec<String> = all_pubkeys.iter().map(|pk| pk.to_string()).collect();
    let my_id = *get_my_id(&session.participants);

    // A participant signs the refund tx for every participant's refund tx.
    // Each participant needs everyone to sign their refund tx in order to approve the refund windows.
    for participant_id in session.participants.keys().copied().collect::<Vec<_>>() {
        let role = TxRole::Refund(participant_id);
        let participant = &session.participants[&participant_id];
        let taproot_tweak = matches!(participant.blockchain, Blockchain::Bitcoin);

        let (unsigned_tx_hex, msg) = match participant.blockchain {
            Blockchain::Bitcoin => {
                let locktime = refund_locktime_btc(session, participant_id);
                let lock_tx_hex = session.lock_txs.get(&participant_id).unwrap();
                let lock_tx: bitcoin::Transaction = deserialize_hex(lock_tx_hex).unwrap();
                let lock_txid = lock_tx.compute_txid();
                let lock_tx_output = lock_tx.output[0].clone();
                let refund_input = bitcoin::OutPoint {
                    txid: lock_txid,
                    vout: 0,
                };

                let unsigned_tx =
                    bitcoin_utils::build_refund_tx(refund_input, participant, locktime, session.bitcoin_fee);
                let msg = compute_sighash(&unsigned_tx, &lock_tx_output);
                (serialize_hex(&unsigned_tx), msg.to_vec())
            }
            Blockchain::Cardano => {
                let refund_slot = refund_locktime_cardano(session, participant_id);
                let lock_tx_hex = session.lock_txs.get(&participant_id).unwrap();
                let lock_tx_bytes = hex::decode(lock_tx_hex).unwrap();
                // FixedTransaction preserves the original byte encoding so we get
                // the exact on-chain txid (blake2b of the original body bytes).
                let lock_txhash = FixedTransaction::from_bytes(lock_tx_bytes).unwrap()
                    .transaction_hash();

                let collateral = session.cardano_collaterals.get(&participant_id)
                    .map(|c| (c.utxo_txid.as_str(), c.utxo_index));
                let unsigned_tx = cardano_utils::build_refund_tx(
                    participant,
                    &lock_txhash,
                    refund_slot,
                    session.cardano_fee,
                    collateral,
                );
                // The Plutus validator's Refund path verifies:
                //   schnorr(agg_pubkey, blake2b(0x01 || txid), sig)
                // 0x01 prefix is the domain tag — prevents a refund sig from being
                // replayed via the Spend redeemer (which signs txid with no prefix).
                // refund_slot is already committed to by txid: the lock tx body
                // contains the datum inline, so any change to refund_slot in the
                // datum would produce a different txid and invalidate all signatures.
                let mut preimage = vec![1u8];
                preimage.extend_from_slice(&lock_txhash.to_bytes());
                let msg = Blake2b::<U32>::digest(&preimage).to_vec();
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
        let sec_nonce_bytes = sec_nonce.serialize().to_vec();

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
        if let Err(e) = broadcast::<TcpTransport, TcpConnector>(&addresses, &envelope, &session.connection_pool).await {
            error!("broadcast nonce failed for role {:?}: {e}", role);
        }
    }
}

/// Broadcasts the signed refund transaction associated with the current session and participant.
///
/// # Parameters
/// * `session` - A mutable reference to the `SwapSession` containing details about the ongoing swap,
///   including transaction and participant data.
/// * `keys` - A reference to the `SwapKeys` containing cryptographic keys used for signing transactions
///   and collateral management.
/// * `config` - A reference to the `DaemonConfig` containing the network configuration and blockchain-specific
///   details, such as base URLs for mempool and other parameters.
///
/// # Returns
/// * `bool` - Returns `true` if the refund transaction was successfully broadcasted, otherwise returns `false`.
///
/// # Logging
/// * Logs a success message if the refund transaction is successfully broadcasted.
/// * Logs an error message if a timeout occurs but no signed refund transaction is found for the participant.
///
/// # Notes
/// * If no signed refund transaction is found in the `session.signed_txs`, this function will return `false`.
/// * For Cardano, if a collateral witness needs to be added, it modifies the existing transaction
///   and updates it in the `signed_txs` map.
///
pub async fn broadcast_my_refund_tx(session: &mut SwapSession, keys: &SwapKeys, config: &DaemonConfig) -> bool {
    let my_id = *get_my_id(&session.participants);
    let role = TxRole::Refund(my_id);

    if let Some(signed_tx_hex) = session.signed_txs.get(&role).cloned() {
        let my_blockchain = session.participants[&my_id].blockchain;
        let ok = match my_blockchain {
            Blockchain::Bitcoin => {
                submit_bitcoin_tx(&signed_tx_hex, config.bitcoin_network.mempool_base_url()).await
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
                submit_cardano_tx(&tx_to_submit, config).await
            }
        };
        info!("broadcast refund tx for role {:?}", role);
        ok
    } else {
        error!("timeout fired for session {} but no signed refund tx found for {:?}", session.id, role);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_swap_keys;
    use crate::types::{Blockchain, Participant, SwapSession, TxRole};
    use bitcoin::consensus::encode::serialize_hex;
    use bitcoin::{
        absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence,
        Transaction, TxIn, TxOut, Witness,
    };
    use std::collections::BTreeMap;

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

    fn make_participant(id: u8, is_me: bool, public_key: String, target: u8) -> Participant {
        Participant {
            id,
            blockchain: Blockchain::Bitcoin,
            tcp_address: format!("127.0.0.1:91{id:02}"),
            target_participant: target,
            amount_locking: 100_000,
            amount_claiming: 100_000,
            is_me,
            secp256k1_public_key: public_key,
            cardano_wallet_public_key: vec![],
            funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
                .to_string(),
            funding_utxo_vout: 0,
        }
    }

    fn make_dummy_lock_tx() -> String {
        let tx = Transaction {
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
        serialize_hex(&tx)
    }

    fn make_session(keys: &[crate::types::SwapKeys]) -> SwapSession {
        let mut participants = BTreeMap::new();
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

        let mut session = SwapSession::new(1, participants, 100, 0, 5_000, 2_000_000);
        session.leader = Some(1);

        // insert dummy lock txs for each participant
        for id in [1u8, 2, 3] {
            session.lock_txs.insert(id, make_dummy_lock_tx());
        }

        session
    }

    #[tokio::test]
    async fn begin_refund_signing_creates_unsigned_txs_for_all_participants() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session(&keys);

        begin_refund_signing(&mut session, &keys[0]).await;

        for id in [1u8, 2, 3] {
            assert!(
                session.unsigned_txs.contains_key(&TxRole::Refund(id)),
                "missing unsigned refund tx for participant {}",
                id
            );
        }
    }

    #[tokio::test]
    async fn begin_refund_signing_creates_musig_sessions_in_round_one() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session(&keys);

        begin_refund_signing(&mut session, &keys[0]).await;

        for id in [1u8, 2, 3] {
            let role = TxRole::Refund(id);
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
    async fn begin_refund_signing_unsigned_txs_are_nonempty() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session(&keys);

        begin_refund_signing(&mut session, &keys[0]).await;

        for id in [1u8, 2, 3] {
            let tx_hex = session.unsigned_txs.get(&TxRole::Refund(id)).unwrap();
            assert!(
                !tx_hex.is_empty(),
                "unsigned tx hex should not be empty for participant {}",
                id
            );
        }
    }

    #[tokio::test]
    async fn begin_refund_signing_uses_staggered_locktimes() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session(&keys);

        begin_refund_signing(&mut session, &keys[0]).await;

        // deserialize txs and check locktimes are different
        let mut locktimes: Vec<u32> = Vec::new();
        for id in [1u8, 2, 3] {
            let tx_hex = session.unsigned_txs.get(&TxRole::Refund(id)).unwrap();
            let tx: bitcoin::Transaction =
                bitcoin::consensus::encode::deserialize_hex(tx_hex).unwrap();
            if let bitcoin::absolute::LockTime::Blocks(height) = tx.lock_time {
                locktimes.push(height.to_consensus_u32());
            }
        }

        assert_eq!(locktimes.len(), 3, "should have 3 locktimes");
        // all locktimes should be different
        let unique: std::collections::HashSet<_> = locktimes.iter().collect();
        assert_eq!(unique.len(), 3, "all locktimes should be different");
    }

    #[tokio::test]
    async fn begin_refund_signing_requires_leader_for_locktime() {
        let keys: Vec<_> = (0..3).map(|_| make_swap_keys()).collect();
        let mut session = make_session(&keys);
        session.leader = None; // no leader set

        // should panic since refund_locktime requires a leader
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                begin_refund_signing(&mut session, &keys[0]).await;
            });
        }));

        assert!(result.is_err(), "should panic when no leader is set");
    }
}
