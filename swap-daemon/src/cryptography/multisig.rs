use std::collections::HashMap;

use crate::{
    blockchains::{bitcoin_utils::taproot_key_agg_ctx, cardano_utils},
    networking::broadcast,
    types::{
        Blockchain, DaemonConfig, Envelope, MusigRuntime, SwapKeys, SwapSession, TxRole,
        WireMessage,
    },
    utils::{aggregate_adaptor_point, aggregate_adaptor_secret, get_my_id, get_other_addresses},
};
use bitcoin::{
    consensus::encode::{deserialize_hex, serialize_hex},
    Witness,
};
use musig2::{
    secp::{MaybePoint, MaybeScalar, Scalar},
    secp256k1::PublicKey,
    AdaptorSignature, AggNonce, CompactSignature, KeyAggContext, LiftedSignature, PartialSignature,
    PubNonce, SecNonce,
};
use tracing::error;

/// Proceeds the swap session to the second round of a MuSig2 protocol.
///
/// In the second round, this function computes and shares a partial signature
/// for the given transaction role (e.g., Spend or Refund). It transitions the
/// session's state, broadcasts the generated partial signature to other
/// participants, and persists the MuSig2 runtime state.
///
/// # Parameters
/// - `session`: Mutable reference to the current swap session, which tracks the
///    state of the MuSig2 signing protocol and other related session data.
/// - `keys`: Reference to the `SwapKeys` structure that contains the necessary
///    cryptographic keys (e.g., secret key) for signing.
/// - `role`: The transaction role (e.g., `TxRole::Spend(_)`), which determines the
///    context of the signing process and whether an adaptor point is utilized.
///
/// # Behavior
/// 1. Extracts the necessary data from the session to continue the signing protocol
///    (e.g., public keys, secret nonce, message, taproot tweak flag).
/// 2. Computes the aggregate nonce from the Schnorr nonces of all participants.
/// 3. If the role involves spending, computes the aggregate adaptor point from
///    the session's adaptor points.
/// 4. Produces a partial signature in a blocking task using the MuSig2 signing context.
/// 5. Updates the session's partial signatures map with the generated signature
///    and transitions the MuSig2 runtime state to `RoundTwo`.
/// 6. Broadcasts the partial signature to other participants using the session's
///    networking pool.
///
/// # Notes
/// - This function does not proceed if the required MuSig2 state is unavailable in the session,
///   or if key/material deserialization fails during the signing process.
/// - If the broadcast of the partial signature fails, an error is logged.
///
/// # Errors
/// - Logs errors if broadcasting the signature envelope fails.
///
/// # Asynchronous Context
/// This function is asynchronous and should be executed within an async runtime. It performs
/// some CPU-intensive operations (e.g., signing) in a blocking context using `tokio::task::spawn_blocking`.
///
/// # Dependencies
/// - This function depends on the `musig2` library for cryptographic signing operations.
/// - It also requires `tokio` for performing blocking tasks and async network I/O.
///
/// # See Also
/// - `MusigRuntime` for the MuSig2 session states.
/// - `SwapSession` for details on how session states and participants are managed.
/// - `musig2::sign_partial` and `musig2::adaptor::sign_partial` for partial signature creation.
pub async fn transition_to_round_two(
    session: &mut SwapSession,
    keys: &SwapKeys,
    role: TxRole,
) {
    let is_adaptor = matches!(role, TxRole::Spend(_));
    let (all_pubkeys, signer_index, msg, taproot_tweak, sec_nonce_bytes) =
        match session.musig_sessions.get(&role) {
            Some(MusigRuntime::RoundOne { all_pubkeys, signer_index, msg, taproot_tweak, sec_nonce }) => {
                (all_pubkeys.clone(), *signer_index, msg.clone(), *taproot_tweak, sec_nonce.clone())
            }
            _ => return,
        };

    let agg_nonce: AggNonce = session
        .schnorr_nonces
        .values()
        .filter_map(|role_map| role_map.get(&role))
        .map(|s| s.parse::<PubNonce>().unwrap())
        .sum();

    let adaptor_point = if is_adaptor {
        Some(aggregate_adaptor_point(&session.adaptor_points))
    } else {
        None
    };

    let secret_key = keys.secret_key;
    let pubkeys_for_blocking = all_pubkeys.clone();
    let msg_for_blocking = msg.clone();

    let sig_hex = tokio::task::spawn_blocking(move || {
        let pubkeys: Vec<PublicKey> = pubkeys_for_blocking.iter().map(|pk| pk.parse().unwrap()).collect();
        let key_agg_ctx = if taproot_tweak {
            taproot_key_agg_ctx(pubkeys)
        } else {
            KeyAggContext::new(pubkeys).unwrap()
        };
        let sec_nonce = SecNonce::from_bytes(&sec_nonce_bytes).unwrap();
        let our_sig: PartialSignature = if let Some(ap) = adaptor_point {
            musig2::adaptor::sign_partial(&key_agg_ctx, secret_key, sec_nonce, &agg_nonce, ap, &msg_for_blocking).unwrap()
        } else {
            musig2::sign_partial(&key_agg_ctx, secret_key, sec_nonce, &agg_nonce, &msg_for_blocking).unwrap()
        };
        hex::encode(our_sig.serialize())
    }).await.unwrap();

    let my_id = *get_my_id(&session.participants);

    session
        .partial_sigs
        .entry(my_id)
        .or_insert_with(HashMap::new)
        .insert(role, sig_hex.clone());

    session.musig_sessions.insert(
        role,
        MusigRuntime::RoundTwo { all_pubkeys, signer_index, msg, taproot_tweak },
    );

    let addresses = get_other_addresses(&session.participants);
    let envelope = Envelope::new(
        session.id,
        my_id,
        WireMessage::PartialSignature { role, sig: sig_hex },
    );
    if let Err(e) = broadcast(&addresses, &envelope, &session.connection_pool).await {
        error!("broadcast partial sig failed for role {:?}: {e}", role);
    }
}

/// Finalizes the role in a multi-party signature scheme by aggregating partial signatures
/// and producing either an adaptor signature or a final transaction signature.
///
/// # Parameters
///
/// * `session` - Mutable reference to a `SwapSession` object that holds the signing sessions,
///   partial signatures, unsigned transactions, and other runtime data.
/// * `_keys` - Reference to the `SwapKeys` which may represent the cryptographic key material
///   associated with the session. This is unused in this function but passed for completeness.
/// * `role` - The role in the signing session (e.g., spender or receiver), represented by `TxRole`.
/// * `config` - Reference to the daemon configuration, typically used for blockchain-specific
///   operations or settings.
///
/// # Details
///
/// 1. Checks if the role requires generating an adaptor signature or a finalized signature.
/// 2. Retrieves the aggregated public keys, the message to sign, and whether the signing involves
///    a taproot tweak from the session's `musig_sessions`.
/// 3. Aggregates nonces and partial signatures stored in the session for the specified role.
/// 4. If the role is for an adaptor signature:
///    - Aggregates the adaptor points and computes the adaptor signature.
/// 5. If the role is for a final signature:
///    - Produces the final aggregated signature and attaches it to the appropriate unsigned
///      transaction based on the blockchain type (`Bitcoin` or `Cardano`).
/// 6. Updates the session by storing the computed signature (adaptor or final).
///
/// # Concurrency
///
/// The cryptographic operations for aggregating keys or generating signatures are performed in
/// a blocking task using `tokio::task::spawn_blocking` to ensure non-blocking execution for
/// asynchronous tasks.
///
/// # Blockchain-Specific Logic
///
/// 1. **Bitcoin**:
///    - Deserializes the unsigned transaction.
///    - Attaches the final signature to the first input's witness data.
///    - Serializes the transaction back to hexadecimal.
///
/// 2. **Cardano**:
///    - Uses a utility function (`cardano_utils::attach_final_sig`) to attach the aggregated
///      signature to the Cardano transaction.
///
/// # Errors
///
/// - If the role's `musig_sessions` do not contain expected runtime data for the second round,
///   the function exits early without doing any processing.
/// - If cryptographic operations fail (e.g., parsing nonces or partial signatures), the function
///   will panic.
///
/// # Panics
///
/// - Parsing errors for nonces, public keys, or partial signatures (e.g., malformed input data).
/// - Failures in cryptographic operations (e.g., aggregation or adaptor signature generation).
/// - If unsigned transaction data is missing from the session.
///
pub async fn finalize_role(
    session: &mut SwapSession,
    _keys: &SwapKeys,
    role: TxRole,
    config: &DaemonConfig,
) {
    let is_adaptor = matches!(role, TxRole::Spend(_));
    let (all_pubkeys, msg, taproot_tweak) = match session.musig_sessions.get(&role) {
        Some(MusigRuntime::RoundTwo { all_pubkeys, msg, taproot_tweak, .. }) => {
            (all_pubkeys.clone(), msg.clone(), *taproot_tweak)
        }
        _ => return,
    };

    let agg_nonce: AggNonce = session
        .schnorr_nonces
        .values()
        .filter_map(|role_map| role_map.get(&role))
        .map(|s| s.parse::<PubNonce>().unwrap())
        .sum();

    let partial_sigs: Vec<PartialSignature> = session
        .partial_sigs
        .values()
        .filter_map(|role_map| role_map.get(&role))
        .map(|s| PartialSignature::from_slice(&hex::decode(s).unwrap()).unwrap())
        .collect();

    let adaptor_point = if is_adaptor {
        Some(aggregate_adaptor_point(&session.adaptor_points))
    } else {
        None
    };
    
    enum CryptoResult {
        Adaptor(Vec<u8>),
        Final(Vec<u8>),
    }

    let crypto_result = tokio::task::spawn_blocking(move || {
        let pubkeys: Vec<PublicKey> = all_pubkeys.iter().map(|pk| pk.parse().unwrap()).collect();
        let key_agg_ctx = if taproot_tweak {
            taproot_key_agg_ctx(pubkeys)
        } else {
            KeyAggContext::new(pubkeys).unwrap()
        };
        if let Some(ap) = adaptor_point {
            let adaptor_sig: AdaptorSignature = musig2::adaptor::aggregate_partial_signatures(
                &key_agg_ctx, &agg_nonce, ap, partial_sigs, &msg,
            ).unwrap();
            CryptoResult::Adaptor(adaptor_sig.serialize().to_vec())
        } else {
            let final_sig: CompactSignature =
                musig2::aggregate_partial_signatures(&key_agg_ctx, &agg_nonce, partial_sigs, &msg).unwrap();
            CryptoResult::Final(final_sig.serialize().to_vec())
        }
    }).await.unwrap();

    match crypto_result {
        CryptoResult::Adaptor(sig_bytes) => {
            session.adaptor_sigs.insert(role, hex::encode(&sig_bytes));
        }
        CryptoResult::Final(sig_bytes) => {
            let unsigned_tx_hex = session.unsigned_txs.get(&role).unwrap().clone();
            let participant_id = role.participant_id();
            let blockchain = session.participants[&participant_id].blockchain;
            let signed_tx_hex = match blockchain {
                Blockchain::Bitcoin => {
                    let mut tx: bitcoin::Transaction = deserialize_hex(&unsigned_tx_hex).unwrap();
                    tx.input[0].witness = Witness::from_slice(&[&sig_bytes]);
                    serialize_hex(&tx)
                }
                Blockchain::Cardano => {
                    cardano_utils::attach_final_sig(&unsigned_tx_hex, &sig_bytes, config, session.cardano_fee, matches!(role, TxRole::Refund(_))).await
                }
            };
            session.signed_txs.insert(role, signed_tx_hex);
        }
    }
}

/// Adapts the role of a swap session by utilizing an aggregated adaptor secret
/// and the provided configurations.
///
/// This function aggregates an adaptor secret for the given swap session
/// and calls the `adapt_role_with` function to update the session's state
/// based on the specified role and configuration.
///
/// # Parameters
///
/// * `session` - A mutable reference to the `SwapSession` that represents the
///   current swap session. The session's state may be modified by this function.
/// * `role` - The `TxRole` to adapt to. Represents the transactional role to be played
///   in the swap (e.g., maker or taker).
/// * `config` - A reference to the `DaemonConfig` that provides configuration for the daemon.
///
/// # Errors
///
/// Any errors produced by the `adapt_role_with` function may propagate to the caller.
///
/// # Notes
///
/// - Ensure that the `session` is properly initialized before calling this function.
/// - The function relies on the correctness of the `aggregate_adaptor_secret`
///   and `adapt_role_with` implementations.
///
pub async fn adapt_role(session: &mut SwapSession, role: TxRole, config: &DaemonConfig) {
    let aggregate_secret = aggregate_adaptor_secret(session);
    adapt_role_with(session, role, aggregate_secret, config).await;
}

/// Adapts the role-specific transaction using the aggregate secret and completes the signing process.
///
/// This function takes an unsigned transaction and an adaptor signature associated with the given role
/// (e.g., Refund or Spend), modifies it using the provided aggregate secret, and completes the
/// transaction signing process. The final signed transaction is then stored in the `session`.
///
/// # Parameters
///
/// - `session`: A mutable reference to the [`SwapSession`] structure.
///   Contains the swap data, including participants, signatures, and transactions.
///
/// - `role`: The [`TxRole`] of the transaction being adapted.
///   Determines whether it's a Spend transaction or a Refund transaction and identifies the participant.
///
/// - `aggregate_secret`: The secret scalar used to adapt the adaptor signature into a valid signature.
///
/// - `config`: A reference to the [`DaemonConfig`] structure. Provides configuration details,
///   including blockchain-specific utilities for processing Cardano transactions.
///
/// # Errors
///
/// This function assumes that all intermediary data (e.g., adaptor signatures, unsigned transactions)
/// exists in the `session` and that signature adaptation works as intended. Panics may occur under
/// the following circumstances:
/// - Missing adaptor signature or unsigned transaction for the given role.
/// - Errors during hex decoding or deserialization of adaptor signatures or transactions.
/// - Unsupported blockchain or failure handling the transaction format for a specific blockchain.
///
/// # Notes
///
/// - The function is asynchronous because certain blockchain-specific signing processes (e.g., Cardano)
///   may require asynchronous operations.
/// - The function is tightly coupled with domain-specific logic for transaction signing, swap sessions,
///   and the use of particular cryptographic utilities.
///
/// Like `adapt_role` but uses an explicitly provided aggregate adaptor secret instead of
/// re-deriving it from `session.adaptor_secrets`.  Call this from `extract_secret_and_adapt`
/// where the extracted secret is already the full aggregate (t1 + t2) and must not be
/// re-combined with the caller's own individual secret.
pub async fn adapt_role_with(
    session: &mut SwapSession,
    role: TxRole,
    aggregate_secret: Scalar,
    config: &DaemonConfig,
) {
    let adaptor_sig_hex = session.adaptor_sigs.get(&role).unwrap();
    let adaptor_sig =
        AdaptorSignature::from_bytes(&hex::decode(adaptor_sig_hex).unwrap()).unwrap();

    let valid_sig: musig2::LiftedSignature = adaptor_sig.adapt(aggregate_secret).unwrap();

    let unsigned_tx_hex = session.unsigned_txs.get(&role).unwrap().clone();

    let participant_id = role.participant_id();
    let blockchain = match role {
        TxRole::Spend(_) => {
            let target_id = session.participants[&participant_id].target_participant;
            session.participants[&target_id].blockchain
        }
        TxRole::Refund(_) => session.participants[&participant_id].blockchain,
    };

    let signed_tx_hex = match blockchain {
        Blockchain::Bitcoin => {
            let mut tx: bitcoin::Transaction = deserialize_hex(&unsigned_tx_hex).unwrap();
            tx.input[0].witness = Witness::from_slice(&[valid_sig.serialize()]);
            serialize_hex(&tx)
        }
        Blockchain::Cardano => {
            cardano_utils::attach_final_sig(&unsigned_tx_hex, &valid_sig.serialize(), config, session.cardano_fee, false).await
        }
    };

    session.signed_txs.insert(role, signed_tx_hex);
}


/// Computes the adaptor secret from a given adaptor signature and a full signature.
///
/// The adaptor secret is the discrete difference between the scalar parts of the full signature
/// and the adaptor signature. It is verified by reconstructing the adaptor public key and
/// comparing it to the points derived from the full signature and adaptor signature.
///
/// # Parameters
///
/// * `adaptor_signature` - An `AdaptorSignature` consisting of a point and a scalar.
/// * `full_signature` - A `LiftedSignature` consisting of a point and a scalar.
///
/// # Returns
///
/// * `Option<Scalar>` - Returns `Some(Scalar)` containing the adaptor secret if the computation
///   is successful and validated, or `None` if validation fails.
///
/// # Errors
///
/// This function will return `None` in the following cases:
/// * If the points extracted from the adaptor signature or full signature are inconsistent.
/// * If the computed adaptor secret does not match the expected adaptor public key.
///
/// ```
/// let adaptor_signature: AdaptorSignature = ...; // Construct an adaptor signature
/// let full_signature: LiftedSignature = ...; // Construct a valid full signature
/// let adaptor_secret = compute_adaptor_secret(adaptor_signature, full_signature);
///
/// match adaptor_secret {
///     Some(secret) => println!("Adaptor secret successfully computed: {:?}", secret),
///     None => println!("Failed to compute adaptor secret: Validation failed."),
/// }
/// ```
pub fn compute_adaptor_secret(
    adaptor_signature: AdaptorSignature,
    full_signature: LiftedSignature,
) -> Option<Scalar> {
    let (adaptor_signature_point, adaptor_signature_scalar): (MaybePoint, MaybeScalar) =
        adaptor_signature.unzip::<MaybePoint, MaybeScalar>();
    let (full_signature_point, full_signature_scalar): (MaybePoint, MaybeScalar) =
        full_signature.unzip();

    let adaptor_signature_scalar = adaptor_signature_scalar.unwrap();
    let full_signature_scalar = full_signature_scalar.unwrap();

    let adaptor_secret = full_signature_scalar - adaptor_signature_scalar;
    let adaptor_point = adaptor_secret.base_point_mul();

    if adaptor_point == full_signature_point - adaptor_signature_point {
        Some(adaptor_secret.unwrap())
    } else if adaptor_point == full_signature_point + adaptor_signature_point {
        Some(-adaptor_secret.unwrap())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{make_pubkeys, make_seed};
    use musig2::{
        secp::Scalar,
        secp256k1::{Secp256k1, SecretKey},
        AdaptorSignature, FirstRound, KeyAggContext, LiftedSignature, PartialSignature,
        SecNonceSpices, SecondRound,
    };
    use rand::rngs::OsRng;
    use rand::RngCore;

    fn make_secret_key() -> SecretKey {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        SecretKey::from_byte_array(bytes).unwrap()
    }

    #[test]
    fn compute_adaptor_secret_recovers_scalar_one() {
        let adaptor_secret = Scalar::one();
        run_adaptor_secret_test(adaptor_secret);
    }

    #[test]
    fn compute_adaptor_secret_recovers_scalar_two() {
        let adaptor_secret = Scalar::two();
        run_adaptor_secret_test(adaptor_secret);
    }

    #[test]
    fn compute_adaptor_secret_recovers_random_scalar() {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let adaptor_secret = Scalar::from_slice(&bytes).unwrap();
        run_adaptor_secret_test(adaptor_secret);
    }

    #[test]
    fn compute_adaptor_secret_returns_none_for_wrong_signature() {
        
        let secp = Secp256k1::new();
        let sk1 = make_secret_key();
        let sk2 = make_secret_key();
        let pk1 = musig2::secp256k1::PublicKey::from_secret_key(&secp, &sk1);
        let pk2 = musig2::secp256k1::PublicKey::from_secret_key(&secp, &sk2);

        let adaptor_secret = Scalar::one();
        let adaptor_point = adaptor_secret.base_point_mul();
        let msg = b"test message";

        let key_agg_ctx = KeyAggContext::new(vec![pk1, pk2]).unwrap();
        let seed1 = make_seed();
        let seed2 = make_seed();

        let mut first_round_1 = FirstRound::new(
            key_agg_ctx.clone(),
            &seed1,
            0,
            SecNonceSpices::new().with_seckey(sk1).with_message(msg),
        )
        .unwrap();
        let mut first_round_2 = FirstRound::new(
            key_agg_ctx.clone(),
            &seed2,
            1,
            SecNonceSpices::new().with_seckey(sk2).with_message(msg),
        )
        .unwrap();

        let nonce1 = first_round_1.our_public_nonce();
        let nonce2 = first_round_2.our_public_nonce();

        first_round_1.receive_nonce(1, nonce2).unwrap();
        first_round_2.receive_nonce(0, nonce1).unwrap();

        let mut second_round_1: SecondRound<&[u8; 12]> = first_round_1
            .finalize_adaptor(sk1, adaptor_point, msg)
            .unwrap();
        let second_round_2: SecondRound<&[u8; 12]> = first_round_2
            .finalize_adaptor(sk2, adaptor_point, msg)
            .unwrap();

        let partial_sig_2: PartialSignature = second_round_2.our_signature();
        second_round_1.receive_signature(1, partial_sig_2).unwrap();

        let adaptor_sig: AdaptorSignature = second_round_1
            .finalize_adaptor::<AdaptorSignature>()
            .unwrap();

        let wrong_secret = Scalar::two();
        let valid_sig: LiftedSignature = adaptor_sig.adapt(wrong_secret).unwrap();

        let result = compute_adaptor_secret(adaptor_sig, valid_sig);
        assert_ne!(result, Some(adaptor_secret));
    }

    #[test]
    fn aggregate_partial_signatures_errors_on_mismatched_messages() {
        let secp = Secp256k1::new();
        let sk1 = make_secret_key();
        let sk2 = make_secret_key();
        let pk1 = musig2::secp256k1::PublicKey::from_secret_key(&secp, &sk1);
        let pk2 = musig2::secp256k1::PublicKey::from_secret_key(&secp, &sk2);

        let msg_a = b"message that signer 1 signs";
        let msg_b = b"different message signer 2 signs";

        let key_agg_ctx = KeyAggContext::new(vec![pk1, pk2]).unwrap();

        let seed1 = make_seed();
        let seed2 = make_seed();
        let sec_nonce1 = musig2::SecNonce::build(seed1).with_seckey(sk1).build();
        let sec_nonce2 = musig2::SecNonce::build(seed2).with_seckey(sk2).build();

        let agg_nonce = AggNonce::sum([sec_nonce1.public_nonce(), sec_nonce2.public_nonce()]);

        let partial_sig_1: PartialSignature = musig2::sign_partial(&key_agg_ctx, sk1, sec_nonce1, &agg_nonce, msg_a).unwrap();
        let partial_sig_2: PartialSignature = musig2::sign_partial(&key_agg_ctx, sk2, sec_nonce2, &agg_nonce, msg_b).unwrap();

        let result: Result<CompactSignature, _> =
            musig2::aggregate_partial_signatures(&key_agg_ctx, &agg_nonce, [partial_sig_1, partial_sig_2], msg_a);

        assert!(result.is_err(), "aggregation should fail when signers signed different messages");
    }

    // --- helpers shared by the round-trip tests ---

    fn random_scalar() -> Scalar {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Scalar::from_slice(&bytes).unwrap()
    }

    fn run_two_party_round_trip(msg: &[u8]) -> (CompactSignature, musig2::secp256k1::PublicKey) {
        let (keys, pubkeys) = make_pubkeys(2);
        let key_agg_ctx = KeyAggContext::new(pubkeys).unwrap();
        let msg = msg.to_vec();

        let sec_nonces: Vec<SecNonce> = keys
            .iter()
            .map(|k| SecNonce::build(make_seed()).with_seckey(k.secret_key).with_message(&msg).build())
            .collect();
        let agg_nonce = AggNonce::sum(sec_nonces.iter().map(|n| n.public_nonce()));

        let partial_sigs: Vec<PartialSignature> = keys
            .iter()
            .zip(sec_nonces)
            .map(|(k, sn)| musig2::sign_partial(&key_agg_ctx, k.secret_key, sn, &agg_nonce, &msg).unwrap())
            .collect();

        let final_sig: CompactSignature =
            musig2::aggregate_partial_signatures(&key_agg_ctx, &agg_nonce, partial_sigs, &msg).unwrap();
        let agg_pk = key_agg_ctx.aggregated_pubkey::<musig2::secp256k1::PublicKey>();
        (final_sig, agg_pk)
    }

    // --- tests ported from the deleted first_round.rs / second_round.rs ---

    #[test]
    fn partial_sig_is_deterministic_for_same_seed() {
        let (keys, pubkeys) = make_pubkeys(2);
        let key_agg_ctx = KeyAggContext::new(pubkeys).unwrap();
        let msg = b"determinism test";
        let seed0 = make_seed();
        let seed1 = make_seed();

        let pn0 = SecNonce::build(seed0).with_seckey(keys[0].secret_key).with_message(msg).build().public_nonce();
        let pn1 = SecNonce::build(seed1).with_seckey(keys[1].secret_key).with_message(msg).build().public_nonce();
        let agg_nonce = AggNonce::sum([pn0, pn1]);

        // Build the same SecNonce twice from the same seed — both partial sigs must be identical.
        let sn_a = SecNonce::build(seed0).with_seckey(keys[0].secret_key).with_message(msg).build();
        let sn_b = SecNonce::build(seed0).with_seckey(keys[0].secret_key).with_message(msg).build();
        let sig_a: PartialSignature = musig2::sign_partial(&key_agg_ctx, keys[0].secret_key, sn_a, &agg_nonce, msg).unwrap();
        let sig_b: PartialSignature = musig2::sign_partial(&key_agg_ctx, keys[0].secret_key, sn_b, &agg_nonce, msg).unwrap();
        assert_eq!(sig_a.serialize(), sig_b.serialize());
    }

    #[test]
    fn partial_sig_differs_for_different_message() {
        let (keys, pubkeys) = make_pubkeys(2);
        let key_agg_ctx = KeyAggContext::new(pubkeys).unwrap();
        let seed0 = make_seed();
        let seed1 = make_seed();

        // Build nonces without message binding so the same nonces apply to both signs,
        // isolating the message as the only variable.
        let pn0 = SecNonce::build(seed0).with_seckey(keys[0].secret_key).build().public_nonce();
        let pn1 = SecNonce::build(seed1).with_seckey(keys[1].secret_key).build().public_nonce();
        let agg_nonce = AggNonce::sum([pn0, pn1]);

        let sn_a = SecNonce::build(seed0).with_seckey(keys[0].secret_key).build();
        let sn_b = SecNonce::build(seed0).with_seckey(keys[0].secret_key).build();
        let sig_a: PartialSignature = musig2::sign_partial(&key_agg_ctx, keys[0].secret_key, sn_a, &agg_nonce, b"message one").unwrap();
        let sig_b: PartialSignature = musig2::sign_partial(&key_agg_ctx, keys[0].secret_key, sn_b, &agg_nonce, b"message two").unwrap();

        assert_ne!(sig_a.serialize(), sig_b.serialize());
    }

    #[test]
    fn two_party_musig2_round_trip_produces_valid_signature() {
        let msg = b"two party round trip";
        let (sig, agg_pk) = run_two_party_round_trip(msg);
        assert_eq!(sig.serialize().len(), 64);
        musig2::verify_single(agg_pk, sig, msg).expect("final signature should verify");
    }

    #[test]
    fn three_party_musig2_round_trip_produces_valid_signature() {
        let (keys, pubkeys) = make_pubkeys(3);
        let key_agg_ctx = KeyAggContext::new(pubkeys).unwrap();
        let msg = b"three party round trip".to_vec();

        let sec_nonces: Vec<SecNonce> = keys
            .iter()
            .map(|k| SecNonce::build(make_seed()).with_seckey(k.secret_key).with_message(&msg).build())
            .collect();
        let agg_nonce = AggNonce::sum(sec_nonces.iter().map(|n| n.public_nonce()));

        let partial_sigs: Vec<PartialSignature> = keys
            .iter()
            .zip(sec_nonces)
            .map(|(k, sn)| musig2::sign_partial(&key_agg_ctx, k.secret_key, sn, &agg_nonce, &msg).unwrap())
            .collect();

        let final_sig: CompactSignature =
            musig2::aggregate_partial_signatures(&key_agg_ctx, &agg_nonce, partial_sigs, &msg).unwrap();

        assert_eq!(final_sig.serialize().len(), 64);
        let agg_pk = key_agg_ctx.aggregated_pubkey::<musig2::secp256k1::PublicKey>();
        musig2::verify_single(agg_pk, final_sig, &msg).expect("three-party final sig should verify");
    }

    #[test]
    fn two_party_musig2_adaptor_round_trip_and_secret_extraction() {
        let (keys, pubkeys) = make_pubkeys(2);
        let key_agg_ctx = KeyAggContext::new(pubkeys).unwrap();
        let msg: Vec<u8> = {
            let mut b = [0u8; 32];
            OsRng.fill_bytes(&mut b);
            b.to_vec()
        };

        let adaptor_secret_1 = random_scalar();
        let adaptor_secret_2 = random_scalar();
        let aggregate_secret = adaptor_secret_1 + adaptor_secret_2;
        let adaptor_point_1 = adaptor_secret_1.base_point_mul();
        let adaptor_point_2 = adaptor_secret_2.base_point_mul();
        let aggregate_adaptor_point = adaptor_point_1 + adaptor_point_2;

        let seeds: Vec<[u8; 32]> = (0..2).map(|_| make_seed()).collect();
        let sec_nonces: Vec<SecNonce> = seeds
            .iter()
            .zip(keys.iter())
            .map(|(s, k)| SecNonce::build(*s).with_seckey(k.secret_key).with_message(&msg).build())
            .collect();
        let pub_nonces: Vec<PubNonce> = sec_nonces.iter().map(|n| n.public_nonce()).collect();
        let agg_nonce = AggNonce::sum(pub_nonces);

        let partial_sigs: Vec<PartialSignature> = keys
            .iter()
            .zip(sec_nonces)
            .map(|(k, sn)| {
                musig2::adaptor::sign_partial(&key_agg_ctx, k.secret_key, sn, &agg_nonce, aggregate_adaptor_point, &msg).unwrap()
            })
            .collect();

        let adaptor_sig: AdaptorSignature =
            musig2::adaptor::aggregate_partial_signatures(&key_agg_ctx, &agg_nonce, aggregate_adaptor_point, partial_sigs, &msg).unwrap();

        assert_eq!(adaptor_sig.serialize().len(), 65);

        let final_sig: LiftedSignature = adaptor_sig.adapt(aggregate_secret).expect("adapt should succeed");
        let agg_pk = key_agg_ctx.aggregated_pubkey::<musig2::secp256k1::PublicKey>();
        musig2::verify_single(agg_pk, final_sig, &msg).expect("adapted sig should verify");

        let extracted = compute_adaptor_secret(adaptor_sig, final_sig);
        assert_eq!(extracted, Some(aggregate_secret.unwrap()), "extracted secret should equal t1+t2");
    }

    fn run_adaptor_secret_test(adaptor_secret: Scalar) {
        let secp = Secp256k1::new();
        let sk1 = make_secret_key();
        let sk2 = make_secret_key();
        let pk1 = musig2::secp256k1::PublicKey::from_secret_key(&secp, &sk1);
        let pk2 = musig2::secp256k1::PublicKey::from_secret_key(&secp, &sk2);

        let adaptor_point = adaptor_secret.base_point_mul();
        let msg = b"test message";

        let key_agg_ctx = KeyAggContext::new(vec![pk1, pk2]).unwrap();
        let seed1 = make_seed();
        let seed2 = make_seed();

        let mut first_round_1 = FirstRound::new(
            key_agg_ctx.clone(),
            &seed1,
            0,
            SecNonceSpices::new().with_seckey(sk1).with_message(msg),
        )
        .unwrap();
        let mut first_round_2 = FirstRound::new(
            key_agg_ctx.clone(),
            &seed2,
            1,
            SecNonceSpices::new().with_seckey(sk2).with_message(msg),
        )
        .unwrap();

        let nonce1 = first_round_1.our_public_nonce();
        let nonce2 = first_round_2.our_public_nonce();

        first_round_1.receive_nonce(1, nonce2).unwrap();
        first_round_2.receive_nonce(0, nonce1).unwrap();

        let mut second_round_1: SecondRound<&[u8; 12]> = first_round_1
            .finalize_adaptor(sk1, adaptor_point, msg)
            .unwrap();
        let second_round_2: SecondRound<&[u8; 12]> = first_round_2
            .finalize_adaptor(sk2, adaptor_point, msg)
            .unwrap();

        let partial_sig_2: PartialSignature = second_round_2.our_signature();
        second_round_1.receive_signature(1, partial_sig_2).unwrap();

        let adaptor_sig: AdaptorSignature = second_round_1
            .finalize_adaptor::<AdaptorSignature>()
            .unwrap();

        let valid_sig: LiftedSignature = adaptor_sig
            .adapt(adaptor_secret)
            .expect("adaptor secret should be valid");

        let agg_pk = key_agg_ctx.aggregated_pubkey::<musig2::secp256k1::PublicKey>();
        musig2::verify_single(agg_pk, valid_sig, msg).expect("signature should be valid");

        let recovered = compute_adaptor_secret(adaptor_sig, valid_sig);

        assert!(recovered.is_some(), "should recover a secret");
        assert_eq!(
            recovered.unwrap(),
            adaptor_secret,
            "recovered secret should match original"
        );
    }
}
