use crate::{
    networking::broadcast,
    transport::tcp::{TcpTransport, TcpConnector},
    types::{Envelope, ParticipantId, SwapSession, WireMessage},
    utils::{get_my_id, get_other_addresses},
};
use rand::{rngs::OsRng, RngCore};
use tracing::error;

/// Initiates the leader election process by generating and broadcasting a leader commitment.
///
/// This function is responsible for starting the leader election by generating a random nonce,
/// computing its commitment, and broadcasting it to other participants in the session.
/// The nonce and its commitment are stored locally for future validation.
///
/// # Parameters
///
/// * `session` - A mutable reference to the `SwapSession` object that represents the current session.
///               This object holds participant data, leader election state, and connection information.
///
/// # Errors
///
/// If the broadcast fails, an error is logged with details of the failure.
///
/// # Panics
///
/// This function does not explicitly handle panics. Ensure that the session is properly initialized
/// and contains valid participant data before invoking this function.
///
/// # Notes
///
/// - The leader election process assumes a reliable broadcasting mechanism and trusted cryptographic operations.
pub async fn start_leader_election(session: &mut SwapSession) {
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let commitment = sha256(&nonce);

    let my_id = *get_my_id(&session.participants);
    session.leader_nonces.insert(my_id, nonce);
    session.leader_commitments.insert(my_id, commitment);

    let addresses = get_other_addresses(&session.participants);
    let envelope = Envelope::new(session.id, my_id, WireMessage::LeaderElectionCommitment(commitment));

    if let Err(e) = broadcast::<TcpTransport, TcpConnector>(&addresses, &envelope, &session.connection_pool).await {
        error!("broadcast failed: {e}");
    }
}

/// Handles the reception of a leader's commitment for a participant in the swap session.
///
/// This asynchronous function accepts a commitment from a leader for a swap participant
/// and stores it in the `leader_commitments` map within the provided `SwapSession`.
///
/// # Parameters
/// - `session`: A mutable reference to the `SwapSession`, which contains the details
///   of the ongoing swap, including a map to store leader commitments.
/// - `commitment`: A 32-byte array representing the leader's cryptographic commitment.
/// - `participant_id`: A reference to the `ParticipantId`, which identifies the participant
///   associated with the commitment.
///
/// # Notes
/// - This function does not verify the validity of the commitment. It assumes that the passed
///   commitment is valid and relevant to the participant identified by `participant_id`.
/// - Make sure to call this function in a context where the `session` is properly mutable.
///
/// # Errors
/// This function does not return errors or perform error handling.
pub async fn received_leader_commitment(
    session: &mut SwapSession,
    commitment: [u8; 32],
    participant_id: &ParticipantId,
) {
    session
        .leader_commitments
        .insert(*participant_id, commitment);
}

/// Broadcasts the leader nonce to all participants in the swap session.
///
/// This asynchronous function generates a message containing the leader
/// nonce and broadcasts it to all participants except the sender using
/// the provided connection pool. If the broadcast fails, an error is logged.
///
/// # Parameters
/// * `session` - A reference to the `SwapSession` that contains information
///   about the current swap session, including the participants and connection pool.
/// * `nonce` - A 32-byte array representing the leader nonce to be broadcasted.
///
/// # Errors
/// If the `broadcast` function encounters an error during execution, the error
/// is caught and logged using the `error!` macro.
///
pub async fn broadcast_leader_nonce(session: &SwapSession, nonce: [u8; 32]) {
    let addresses = get_other_addresses(&session.participants);
    let wire_message = WireMessage::LeaderElectionNonce(nonce);
    let envelope = Envelope::new(
        session.id,
        get_my_id(&session.participants).clone(),
        wire_message,
    );

    if let Err(e) = broadcast::<TcpTransport, TcpConnector>(&addresses, &envelope, &session.connection_pool).await {
        error!("broadcast failed: {e}");
    }
}

/// Handles the reception of a leader's nonce in a swap session, validates it against the stored commitment,
/// and stores the validated nonce in the session.
///
/// # Parameters
///
/// * `session` - A mutable reference to the `SwapSession`, which contains the state of the swap protocol.
/// * `nonce` - A 32-byte array representing the nonce provided by the leader.
/// * `participant_id` - A reference to the `ParticipantId`, which uniquely identifies the participant whose nonce is being received.
///
/// # Panics
///
/// This function will panic under the following conditions:
/// - If there is no commitment available for the given `participant_id`.
/// - If the provided `nonce` does not match the stored commitment.
///
pub fn received_leader_nonce(
    session: &mut SwapSession,
    nonce: [u8; 32],
    participant_id: &ParticipantId,
) {
    let commitment = session
        .leader_commitments
        .get(participant_id)
        .expect("No commitment for this participant");

    if sha256(&nonce) != *commitment {
        panic!("Nonce invalid with commitment");
    }

    session.leader_nonces.insert(*participant_id, nonce);
}

/// Computes the leader for the current swap session.
///
/// The function determines the leader of the swap session by aggregating all
/// leader nonces, generating a deterministic seed using the SHA-256 hash, and
/// selecting an index based on the seed. The leader is the participant at the
/// computed index.
///
/// # Parameters
/// - `session`: A mutable reference to a `SwapSession` object. This session object
///   contains the data necessary to compute the leader, including `leader_nonces`
///   (a collection of nonces associated with participants) and `participants`
///   (a map of participant identifiers).
///
/// # Panics
/// This function will panic if:
/// - The `participants` collection in `session` is empty.
/// - The index returned by `leader_index` is out of bounds (though this is unlikely
///   if `leader_index` and the other logic are implemented correctly).
///
pub fn compute_leader(session: &mut SwapSession) {
    let mut input = Vec::new();
    for nonce in session.leader_nonces.values() {
        input.extend_from_slice(nonce);
    }

    let seed = sha256(&input);
    let idx = leader_index(&seed, session.participants.len());
    let leader_id = *session.participants.keys().nth(idx).unwrap();
    session.leader = Some(leader_id);
}

/// Computes the SHA-256 hash of the given input data.
///
/// # Parameters
/// - `data`: A byte slice (`&[u8]`) representing the input data to hash.
///
/// # Returns
/// - A 32-byte array (`[u8; 32]`) containing the SHA-256 hash of the input data.
///
/// # Notes
/// - Ensure the `bitcoin` crate is included in your `Cargo.toml` file to use this function.
/// - The return value is the hash as a fixed-length 32-byte array.
///
/// # Panics
/// This function does not panic.
fn sha256(data: &[u8]) -> [u8; 32] {
    use bitcoin::hashes::{sha256, Hash};
    sha256::Hash::hash(data).to_byte_array()
}

/// Computes the leader index from a given seed and total number of elements.
///
/// # Parameters
/// - `seed`: A reference to a 32-byte array (`[u8; 32]`) that serves as the input seed.
///           Only the first 8 bytes of this seed are used in the computation.
/// - `n`: The total number of elements among which the leader index is to be selected.
///
/// # Returns
/// - A `usize` value representing the leader index. This will always be a value in the range `[0, n)`.
///
/// # Panics
/// - This function will panic if the slice operation `seed[0..8]` fails.
///   However, in normal cases with a valid `[u8; 32]` input, this panic should not occur.
///
/// # Notes
/// The function calculates the leader index using the first 8 bytes of the seed
/// interpreted as a big-endian `u64`. The result is determined by taking the modulo
/// of this value with the total number of elements `n`.
fn leader_index(seed: &[u8; 32], n: usize) -> usize {
    let x = u64::from_be_bytes(seed[0..8].try_into().unwrap());
    (x as usize) % n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_session;

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

    #[tokio::test]
    async fn start_leader_election_sets_my_nonce_and_commitment() {
        let mut session = make_session(42);

        start_leader_election(&mut session).await;

        let me_id = *get_my_id(&session.participants);

        assert!(session.leader_nonces.contains_key(&me_id));
        assert!(session.leader_commitments.contains_key(&me_id));

        let nonce = session.leader_nonces.get(&me_id).unwrap();
        let commitment = session.leader_commitments.get(&me_id).unwrap();
        assert_eq!(sha256(nonce), *commitment);
        // state change now happens in handle_session_message
    }

    #[tokio::test]
    async fn received_leader_commitment_sets_commitment_for_participant() {
        let mut session = make_session(42);
        let commitment = [7u8; 32];

        received_leader_commitment(&mut session, commitment, &2).await;

        assert_eq!(session.leader_commitments.get(&2), Some(&commitment));
    }

    #[tokio::test]
    async fn received_leader_commitment_stores_commitment_for_participant() {
        let mut session = make_session(42);
        let commitment = [7u8; 32];

        received_leader_commitment(&mut session, commitment, &2).await;

        assert_eq!(session.leader_commitments.get(&2), Some(&commitment));
        // state change and nonce broadcast now happen in handle_session_message
    }
    #[test]
    fn received_leader_nonce_sets_nonce_for_participant() {
        let mut session = make_session(42);

        let nonce = [5u8; 32];
        let commitment = sha256(&nonce);
        session.leader_commitments.insert(2, commitment);

        received_leader_nonce(&mut session, nonce, &2);

        assert_eq!(session.leader_nonces.get(&2), Some(&nonce));
    }

    #[test]
    fn received_leader_nonce_stores_nonce_for_participant() {
        let mut session = make_session(42);

        let my_nonce = [1u8; 32];
        let my_commitment = sha256(&my_nonce);
        session.leader_nonces.insert(1, my_nonce);
        session.leader_commitments.insert(1, my_commitment);

        let other_nonce = [5u8; 32];
        let other_commitment = sha256(&other_nonce);
        session.leader_commitments.insert(2, other_commitment);

        received_leader_nonce(&mut session, other_nonce, &2);

        assert_eq!(session.leader_nonces.get(&2), Some(&other_nonce));
        // state change and leader computation now happen in handle_session_message
    }

    #[test]
    #[should_panic(expected = "Nonce invalid with commitment")]
    fn received_leader_nonce_panics_if_nonce_does_not_match_commitment() {
        let mut session = make_session(42);
        session.leader_commitments.insert(2, [1u8; 32]);
        received_leader_nonce(&mut session, [2u8; 32], &2);
    }

    #[test]
    fn compute_leader_marks_exactly_one_leader() {
        let mut session = make_session(42);

        session.leader_nonces.insert(1, [1u8; 32]);
        session.leader_nonces.insert(2, [2u8; 32]);

        compute_leader(&mut session);

        assert!(session.leader.is_some());
    }

    #[test]
    fn leader_index_is_within_bounds() {
        let seed = [3u8; 32];
        for n in 1..10 {
            let idx = leader_index(&seed, n);
            assert!(idx < n);
        }
    }
}
