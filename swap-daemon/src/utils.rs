use std::collections::HashMap;

use musig2::secp::Scalar;
use secp256k1::PublicKey;

use crate::types::{Address, ParticipantId, Participants, SwapSession, TxRole};

/// Retrieves the ID of the current participant marked as "me" from a list of participants.
///
/// # Parameters
///
/// * `participants` - A reference to a `Participants` collection, which is an iterable structure (e.g., a map or sequence)
///   that stores participant data. Each entry consists of a participant ID (key, of type `u8`) and information about
///   the participant (value).
///
/// # Returns
///
/// * A reference to the `u8` value that represents the ID of the participant marked as "me".
///
/// # Panics
///
/// * This function will panic if no participant is marked as "me" in the provided `participants` collection.
///
pub fn get_my_id(participants: &Participants) -> &u8 {
    participants
        .iter()
        .find(|(_, p)| p.is_me)
        .map(|(id, _)| id)
        .expect("No participant marked as me")
}

/// Checks if all lock transactions for the participants in a swap session have been broadcasted.
///
/// This function iterates over the keys (participant IDs) of the `participants` in the given `SwapSession`
/// and verifies that each participant's ID is present in the `lock_txs_broadcast` set.
/// If all participant IDs are found in `lock_txs_broadcast`, the function returns `true`.
/// Otherwise, it returns `false`.
///
/// # Parameters
///
/// * `session` - A reference to a `SwapSession` containing the participant data and the set of broadcasted lock transaction IDs.
///
/// # Returns
///
/// * `true` if all participants' lock transactions have been broadcasted.
/// * `false` otherwise.
///
pub fn all_lock_txs_broadcast(session: &SwapSession) -> bool {
    session
        .participants
        .keys()
        .all(|id| session.lock_txs_broadcast.contains(id))
}

/// Checks if all lock transactions for the participants in a swap session have been confirmed.
///
/// # Parameters
/// * `session` - A reference to the `SwapSession` containing the participants and their confirmed lock transactions.
///
/// # Returns
/// * `true` if all participant IDs in the `session` are present in the set of `confirmed_lock_txs`.
/// * `false` otherwise.
///
pub fn all_lock_txs_confirmed(session: &SwapSession) -> bool {
    session
        .participants
        .keys()
        .all(|id| session.confirmed_lock_txs.contains(id))
}

/// Retrieves a list of TCP addresses for all participants except the current user.
///
/// This function iterates over the participants, filters out the one marked as `is_me`,
/// and collects the TCP addresses of the remaining participants into a vector.
///
/// # Parameters
/// - `participants`: A reference to a `Participants` collection (e.g., a `HashMap` or similar data structure) containing
///   participant information. Each participant is expected to have an `is_me` field to determine
///   if they are the current user and a `tcp_address` field representing their TCP address.
///
/// # Returns
/// - A `Vec<Address>` containing the TCP addresses of all participants except the one marked as the current user.
///
pub fn get_other_addresses(participants: &Participants) -> Vec<Address> {
    participants
        .values()
        .filter(|p| !p.is_me)
        .map(|p| p.tcp_address.clone())
        .collect()
}

/// Checks if all adaptor points have been received for all participants in the swap session.
///
/// # Parameters
///
/// * `session` - A reference to the `SwapSession` object containing session details,
///               including participants and their respective adaptor points.
///
/// # Returns
///
/// * `true` if adaptor points have been received for all participants in the session.
/// * `false` if at least one participant does not have an associated adaptor point.
///
pub fn all_adaptor_points_received(session: &SwapSession) -> bool {
    session
        .participants
        .keys()
        .all(|id| session.adaptor_points.contains_key(id))
}

/// Checks if the current user is the leader of the swap session.
///
/// # Parameters
///
/// * `session` - A reference to a `SwapSession` instance containing the session
///   details, including the participants and the leader.
///
/// # Returns
///
/// * `true` if the current user's ID matches the ID of the leader in the session.
/// * `false` otherwise.
///
pub fn leader_is_me(session: &SwapSession) -> bool {
    let my_id = get_my_id(&session.participants);
    session.leader == Some(*my_id)
}

/// Checks if all leader commitments have been received in the swap session.
///
/// This function verifies whether every participant in the swap session
/// has a corresponding leader commitment within the session's
/// `leader_commitments` map.
///
/// # Parameters
///
/// * `session` - A reference to the `SwapSession` struct which contains
///   information about the participants and their leader commitments.
///
/// # Returns
///
/// * `true` if all participants' IDs exist as keys in the `leader_commitments` map.
/// * `false` if at least one participant's ID is missing from the `leader_commitments` map.
///
pub fn all_leader_commitments_received(session: &SwapSession) -> bool {
    session
        .participants
        .keys()
        .all(|id| session.leader_commitments.contains_key(id))
}

/// Checks if leader nonces have been received from all participants in a swap session.
///
/// # Parameters
/// - `session`: A reference to a `SwapSession` containing the session's participants
///   and the received leader nonces.
///
/// # Returns
/// - `true` if leader nonces are present for all participant IDs in the session.
/// - `false` otherwise.
///
pub fn all_leader_nonces_received(session: &SwapSession) -> bool {
    session
        .participants
        .keys()
        .all(|id| session.leader_nonces.contains_key(id))
}

/// Checks if all required Schnorr nonces have been received for a given session and transaction role.
///
/// This function iterates through all the participants in the given `SwapSession`
/// (excluding the current participant) and verifies if Schnorr nonces for the specified
/// `TxRole` have been received from each participant. It returns `true` if all nonces
/// are present, and `false` otherwise.
///
/// # Parameters
/// - `session`: A reference to the current `SwapSession`, which contains information
///   about participants, Schnorr nonces, and their roles in the swap.
/// - `role`: The transaction role (`TxRole`) for which Schnorr nonces are being checked.
///
/// # Returns
/// - `true` if all participants (excluding the current user) have provided Schnorr nonces
///   for the given `TxRole`.
/// - `false` if at least one participant has not provided their Schnorr nonce for the given role.
///
/// # Notes
/// - This function relies on the `is_me` property of a participant to exclude the current user
///   from the checks.
/// - The `schnorr_nonces` field is expected to be a mapping of participant IDs to their collections
///   of nonce information, keyed by `TxRole`.
pub fn all_schnorr_nonces_received_for(session: &SwapSession, role: TxRole) -> bool {
    session
        .participants
        .iter()
        .filter(|(_, p)| !p.is_me)
        .all(|(id, _)| {
            session
                .schnorr_nonces
                .get(id)
                .map_or(false, |nonces| nonces.contains_key(&role))
        })
}

/// Checks if all required partial signatures for a given transaction role
/// have been received from all participants in the swap session, except for
/// the current participant.
///
/// # Parameters
/// * `session` - A reference to the `SwapSession`, which contains details about the
///   participants and partial signatures.
/// * `role` - The `TxRole` representing the transaction role to check for partial signatures.
///
/// # Returns
/// * `true` if all participants (excluding the current one) have provided partial
///   signatures for the specified transaction role.
/// * `false` otherwise.
///
/// # Notes
/// - The function iterates over the `participants` in the `SwapSession`, filters out the
///   current participant (one for whom `is_me` is `true`), and checks if each remaining
///   participant has provided a partial signature for the given `role` in the `partial_sigs` map.
/// - If any required signature is missing or `partial_sigs` does not contain the specified
///   transaction role for a participant, the function returns `false`.
///
pub fn all_partial_sigs_received_for(session: &SwapSession, role: TxRole) -> bool {
    session
        .participants
        .iter()
        .filter(|(_, p)| !p.is_me)
        .all(|(id, _)| {
            session
                .partial_sigs
                .get(id)
                .map_or(false, |sigs| sigs.contains_key(&role))
        })
}

/// Checks whether all the required partial signatures for refund transactions
/// and adaptor signatures for spend transactions have been received.
///
/// # Parameters
/// - `session`: A reference to the `SwapSession` containing the participants,
///   partial signatures for refund transactions, and adaptor signatures for spend transactions.
///
/// # Returns
/// - `true` if:
///   - All participants, excluding the current participant (`is_me`), have provided
///     partial signatures for all refund transactions.
///   - Adaptor signatures have been received for all spend transactions.
/// - `false` otherwise.
///
pub fn all_partial_sigs_received_for_all_refund_and_spend_txs(session: &SwapSession) -> bool {
    // check all partial sigs received for all refund txs
    let all_refund_sigs_received = session.participants.keys().all(|id| {
        let role = TxRole::Refund(*id);
        session
            .participants
            .iter()
            .filter(|(_, p)| !p.is_me)
            .all(|(sender_id, _)| {
                session
                    .partial_sigs
                    .get(sender_id)
                    .map_or(false, |sigs| sigs.contains_key(&role))
            })
    });

    // check all adaptor sigs received for all spend txs
    let all_adaptor_sigs_received = session.participants.keys().all(|id| {
        let role = TxRole::Spend(*id);
        session.adaptor_sigs.contains_key(&role)
    });

    all_refund_sigs_received && all_adaptor_sigs_received
}

/// Checks if all adaptor secrets have been received for the given swap session.
///
/// This function iterates through all participants in the `SwapSession`
/// and verifies that there is a corresponding entry in the `adaptor_secrets`
/// map for each participant. If all participants have their adaptor secrets
/// recorded, the function returns `true`; otherwise, it returns `false`.
///
/// # Parameters
///
/// * `session` - A reference to the `SwapSession` struct, which contains the
///   list of participants and their corresponding adaptor secrets.
///
/// # Returns
///
/// * `bool` - `true` if all participants in the session have their adaptor
///   secrets recorded, otherwise `false`.
///
pub fn all_adaptor_secrets_received(session: &SwapSession) -> bool {
    session
        .participants
        .keys()
        .all(|id| session.adaptor_secrets.contains_key(id))
}

/// Aggregates multiple adaptor points (public keys) into a single combined public key.
///
/// This function takes a reference to a `HashMap` where each entry represents a participant's
/// ID (`ParticipantId`) and their corresponding adaptor point as a `String`. It attempts to parse
/// each adaptor point string into a `PublicKey`. Successfully parsed public keys are combined
/// iteratively into a single aggregated public key using the `combine` method.
///
/// # Parameters
///
/// * `adaptor_points` - A reference to a `HashMap` mapping `ParticipantId` to their adaptor
///   points represented as strings.
///
/// # Returns
///
/// * A `PublicKey` that represents the aggregate of all valid, parseable adaptor points.
///
/// # Panics
///
/// * The function will panic if:
///   - No valid `PublicKey` can be parsed from the input.
///   - The `combine` method of the `PublicKey` fails during aggregation.
///
pub fn aggregate_adaptor_point(adaptor_points: &HashMap<ParticipantId, String>) -> PublicKey {
    let mut iter = adaptor_points
        .values()
        .filter_map(|p| p.parse::<PublicKey>().ok());

    let first = iter.next().unwrap();
    iter.fold(first, |acc, pk| acc.combine(&pk).unwrap())
}

/// Aggregates adaptor secrets in a swap session.
///
/// This function takes a reference to a `SwapSession` and aggregates all the adaptor secrets
/// it contains. The adaptor secrets are stored as hexadecimal strings in the `adaptor_secrets`
/// map of the provided session. Each secret is decoded, converted into a `Scalar`, and then
/// summed together to produce a single aggregated `Scalar`.
///
/// # Parameters
///
/// * `session` - A reference to a `SwapSession` containing adaptor secrets stored as hexadecimal
///   strings in its `adaptor_secrets` field.
///
/// # Returns
///
/// * `Scalar` - The aggregated value of all adaptor secrets.
///
/// # Panics
///
/// * This function will panic if:
///     - The `adaptor_secrets` map is empty.
///     - Any secret in the map fails to decode from its hexadecimal representation.
///     - Any resulting `Scalar` fails to be created from a decoded byte slice.
///     - The summation of secrets results in an invalid `Scalar`.
///
/// # Notes
///
/// * The function assumes that all hexadecimal strings in the `adaptor_secrets` map are valid
///   and can be decoded into byte arrays.
/// * If the summation of secrets overflows the finite field, `Scalar::unwrap` may panic.
pub fn aggregate_adaptor_secret(session: &SwapSession) -> Scalar {
    let mut iter = session
        .adaptor_secrets
        .values()
        .filter_map(|s| hex::decode(s).ok())
        .filter_map(|b| Scalar::from_slice(&b).ok());

    let first = iter.next().unwrap();
    iter.fold(first, |acc, s| (acc + s).unwrap())
}

/// Checks if the given collection of participants forms a valid single cycle.
///
/// A valid cycle must meet the following criteria:
/// 1. Each participant's target exists in the collection (no missing links).
/// 2. No self-loops are present (a participant cannot target itself).
/// 3. Following the chain starting from any participant should visit all participants
///    exactly once before returning to the starting participant.
///
/// # Parameters
/// * `participants` - A map where the keys are participant IDs and the values are
///   participant objects, each of which has an `id` and a `target_participant`.
///
/// # Returns
/// * `true` if the collection forms a valid single cycle.
/// * `false` otherwise.
///
/// # Notes
/// * If the `participants` map is empty, the function will return `false`, as there can
///   be no cycle.
/// * If a sub-cycle exists (i.e., the chain does not include all participants), the
///   function will return `false`.
///
pub fn check_cyclic(participants: &Participants) -> bool {
    if participants.is_empty() {
        return false;
    }

    // every receiver_participant must exist in the map
    for p in participants.values() {
        if !participants.contains_key(&p.target_participant) {
            return false;
        }
        // no self-loops
        if p.target_participant == p.id {
            return false;
        }
    }

    // follow the chain from the first participant
    // if it forms a single cycle visiting all participants, it's valid
    let start = *participants.keys().next().unwrap();
    let mut current = start;
    let mut visited = 0;

    loop {
        visited += 1;
        current = participants[&current].target_participant;
        if current == start {
            break;
        }
        if visited > participants.len() {
            // caught in a sub-cycle
            return false;
        }
    }

    visited == participants.len()
}

/// Calculates the locktime for a refund transaction in a Bitcoin atomic swap.
///
/// # Parameters
///
/// * `session` - A reference to the current `SwapSession` containing the swap's configuration
///   and session-specific details.
/// * `participant_id` - The identifier of the participant for whom the refund locktime is being calculated.
///
/// # Returns
///
/// * A `u32` value representing the locktime block height for the refund transaction.
///
/// # Panics
///
/// This function will panic if a leader has not yet been elected in the session (`session.leader` is `None`).
///
/// # Notes
// - Uses ceiling division so even sub-600s windows produce at least 1 block of separation.
pub fn refund_locktime_btc(session: &SwapSession, participant_id: ParticipantId) -> u32 {
    let leader_id = session.leader.expect("leader not yet elected");
    let distance = distance_from_leader(&session.participants, participant_id, leader_id);
    let blocks_per_window = ((session.refund_window_secs + 599) / 600) as u32;
    session.start_block + distance * blocks_per_window
}

/// Calculates the refund locktime for a participant in a Cardano swap session.
///
/// # Parameters
///
/// * `session` - A reference to the `SwapSession` structure, which contains details about
///               the swap session, including participants, the leader, start slot,
///               and refund timing configuration.
/// * `participant_id` - The unique identifier of the participant for whom the refund locktime
///                      is being calculated.
///
/// # Returns
///
/// * `u64` - The calculated refund locktime in slot units for the specified participant.
///
/// # Panics
///
/// * This function will panic if the leader of the session has not yet been elected (`session.leader` is `None`).
///
pub fn refund_locktime_cardano(session: &SwapSession, participant_id: ParticipantId) -> u64 {
    let leader_id = session.leader.expect("leader not yet elected");
    let distance = distance_from_leader(&session.participants, participant_id, leader_id) as u64;
    session.start_slot + distance * session.refund_window_secs
}

/// Calculates the distance (in terms of links) from a specified participant to the leader in a
/// directed graph of participants.
///
/// This function determines refund window ordering:
/// - The leader's refund window opens last — any cheating requires the leader's cooperation,
///   so they bear the cost of delay.
/// - Each party's refund window opens after their claimant's — if your claimant takes both
///   their refund and your output, you can still claim your own output since the refund window
///   of the person you are claiming from has not yet opened. The only party left without either
///   their refund or their claim is the leader.
///
/// # Arguments
///
/// * `participants` - A reference to a `Participants` map, where each key is a `ParticipantId`,
///   and the value contains information about the participant, including its target participant.
/// * `from` - The `ParticipantId` of the participant from which the distance to the leader
///   will be calculated.
/// * `leader_id` - The `ParticipantId` of the leader participant.
///
/// # Returns
///
/// Returns the number of steps required to reach the leader starting from the specified
/// participant (`from`). If the leader cannot be reached or a cycle is detected, the function panics.
///
/// # Panics
///
/// This function will panic if:
/// - The graph contains a cycle that prevents the traversal from reaching the `from` participant starting
///   at the `leader_id`.
/// - The `participants` map is inconsistent or does not allow iteration to complete within bounds.
///
/// # Example
///
/// ```text
/// use std::collections::HashMap;
///
/// #[derive(Clone)]
/// struct Participant {
///     target_participant: ParticipantId,
/// }
///
/// type ParticipantId = u32;
/// type Participants = HashMap<ParticipantId, Participant>;
///
/// let mut participants = Participants::new();
/// participants.insert(1, Participant { target_participant: 2 });
/// participants.insert(2, Participant { target_participant: 3 });
/// participants.insert(3, Participant { target_participant: 1 });
///
/// let result = distance_from_leader(&participants, 2, 1);
/// assert_eq!(result, 2);
/// ```
///
/// # Notes
///
/// - Ensure that the `participants` map accurately represents the directed graph of participant targets.
/// The function assumes that the graph is either acyclic or has a detectable issue if traversal
/// goes beyond the size of the map.
pub fn distance_from_leader(
    participants: &Participants,
    from: ParticipantId,
    leader_id: ParticipantId,
) -> u32 {
    let mut current = leader_id;
    let mut distance = 0;

    loop {
        let target = participants[&current].target_participant;
        distance += 1;
        if target == from {
            return distance;
        }
        current = target;
        if distance > participants.len() as u32 {
            panic!("cycle broken — could not reach participant from leader");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::types::{Blockchain, Participant};

    use super::*;

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

    fn make_participant(id: u8, target: u8) -> Participant {
        Participant {
            id,
            target_participant: target,
            blockchain: Blockchain::Bitcoin,
            tcp_address: format!("127.0.0.1:91{id:02}"),
            amount_locking: 100,
            amount_claiming: 100,
            is_me: id == 1,
            secp256k1_public_key:
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".to_string(),
            cardano_wallet_public_key: vec![],
            funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
                .to_string(),
            funding_utxo_vout: 0,
        }
    }

    fn make_session_with_leader(leader: ParticipantId) -> SwapSession {
        let mut session = SwapSession::new(
            1,
            {
                let mut p = BTreeMap::new();
                p.insert(1, make_participant(1, 2));
                p.insert(2, make_participant(2, 3));
                p.insert(3, make_participant(3, 1));
                p
            },
            100,       // start_block
            5_000_000, // start_slot
            5_000,
            2_000_000,
        );
        session.leader = Some(leader);
        session
    }

    #[test]
    fn valid_cycle_two_participants() {
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, 2));
        participants.insert(2, make_participant(2, 1));
        assert!(check_cyclic(&participants));
    }

    #[test]
    fn valid_cycle_three_participants() {
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, 2));
        participants.insert(2, make_participant(2, 3));
        participants.insert(3, make_participant(3, 1));
        assert!(check_cyclic(&participants));
    }

    #[test]
    fn invalid_sub_cycle() {
        // 1->2->1 but 3->1, so 3 is never in the cycle
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, 2));
        participants.insert(2, make_participant(2, 1));
        participants.insert(3, make_participant(3, 1));
        assert!(!check_cyclic(&participants));
    }

    #[test]
    fn invalid_self_loop() {
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, 1));
        participants.insert(2, make_participant(2, 1));
        assert!(!check_cyclic(&participants));
    }

    #[test]
    fn invalid_missing_receiver() {
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, 2));
        participants.insert(2, make_participant(2, 99)); // 99 doesn't exist
        assert!(!check_cyclic(&participants));
    }

    #[test]
    fn invalid_empty() {
        let participants = BTreeMap::new();
        assert!(!check_cyclic(&participants));
    }

    fn make_four_party_session_with_leader(leader: ParticipantId) -> SwapSession {
        let mut session = SwapSession::new(
            1,
            {
                let mut p = BTreeMap::new();
                p.insert(1, make_participant(1, 2));
                p.insert(2, make_participant(2, 3));
                p.insert(3, make_participant(3, 4));
                p.insert(4, make_participant(4, 1));
                p
            },
            100,       // start_block
            5_000_000, // start_slot
            5_000,
            2_000_000,
        );
        session.leader = Some(leader);
        session
    }

    #[test]
    fn btc_locktimes_non_default_leader() {
        // cycle: 1->2->3->1, leader=2; distances from leader:
        // p3: 2->3 = 1 hop; p1: 2->3->1 = 2 hops; p2/leader: full cycle = 3 hops
        let session = make_session_with_leader(2);
        let blocks_per_window = ((session.refund_window_secs + 599) / 600) as u32;
        assert_eq!(refund_locktime_btc(&session, 3), session.start_block + 1 * blocks_per_window);
        assert_eq!(refund_locktime_btc(&session, 1), session.start_block + 2 * blocks_per_window);
        assert_eq!(refund_locktime_btc(&session, 2), session.start_block + 3 * blocks_per_window);
    }

    #[test]
    fn cardano_locktimes_non_default_leader() {
        let session = make_session_with_leader(2);
        assert_eq!(refund_locktime_cardano(&session, 3), session.start_slot + 1 * session.refund_window_secs);
        assert_eq!(refund_locktime_cardano(&session, 1), session.start_slot + 2 * session.refund_window_secs);
        assert_eq!(refund_locktime_cardano(&session, 2), session.start_slot + 3 * session.refund_window_secs);
    }

    #[test]
    fn four_party_btc_locktimes_are_multiples_of_window() {
        // cycle: 1(leader)->2->3->4->1, distances from leader:
        // p2=1, p3=2, p4=3, p1/leader=4
        let session = make_four_party_session_with_leader(1);
        let blocks_per_window = ((session.refund_window_secs + 599) / 600) as u32;
        assert_eq!(refund_locktime_btc(&session, 2), session.start_block + 1 * blocks_per_window);
        assert_eq!(refund_locktime_btc(&session, 3), session.start_block + 2 * blocks_per_window);
        assert_eq!(refund_locktime_btc(&session, 4), session.start_block + 3 * blocks_per_window);
        assert_eq!(refund_locktime_btc(&session, 1), session.start_block + 4 * blocks_per_window);
    }

    #[test]
    fn four_party_cardano_locktimes_are_multiples_of_window() {
        let session = make_four_party_session_with_leader(1);
        assert_eq!(refund_locktime_cardano(&session, 2), session.start_slot + 1 * session.refund_window_secs);
        assert_eq!(refund_locktime_cardano(&session, 3), session.start_slot + 2 * session.refund_window_secs);
        assert_eq!(refund_locktime_cardano(&session, 4), session.start_slot + 3 * session.refund_window_secs);
        assert_eq!(refund_locktime_cardano(&session, 1), session.start_slot + 4 * session.refund_window_secs);
    }

    #[test]
    fn leader_has_longest_window() {
        // cycle: 1(leader)->2->3->1, distances from leader: p2=1, p3=2, p1/leader=3
        let session = make_session_with_leader(1);
        assert!(refund_locktime_btc(&session, 1) > refund_locktime_btc(&session, 3));
        assert!(refund_locktime_btc(&session, 3) > refund_locktime_btc(&session, 2));
        assert!(refund_locktime_cardano(&session, 1) > refund_locktime_cardano(&session, 3));
        assert!(refund_locktime_cardano(&session, 3) > refund_locktime_cardano(&session, 2));
    }

    #[test]
    fn btc_locktimes_are_multiples_of_window() {
        let session = make_session_with_leader(1);
        let blocks_per_window = ((session.refund_window_secs + 599) / 600) as u32;
        // cycle: 1(leader)->2->3->1, distances from leader:
        // p2: 1->2 = 1 hop; p3: 1->2->3 = 2 hops; p1/leader: full cycle = 3 hops
        assert_eq!(
            refund_locktime_btc(&session, 2),
            session.start_block + 1 * blocks_per_window
        );
        assert_eq!(
            refund_locktime_btc(&session, 3),
            session.start_block + 2 * blocks_per_window
        );
        assert_eq!(
            refund_locktime_btc(&session, 1),
            session.start_block + 3 * blocks_per_window
        );
    }

    #[test]
    fn cardano_locktimes_are_multiples_of_window() {
        let session = make_session_with_leader(1);
        assert_eq!(
            refund_locktime_cardano(&session, 2),
            session.start_slot + 1 * session.refund_window_secs
        );
        assert_eq!(
            refund_locktime_cardano(&session, 3),
            session.start_slot + 2 * session.refund_window_secs
        );
        assert_eq!(
            refund_locktime_cardano(&session, 1),
            session.start_slot + 3 * session.refund_window_secs
        );
    }
}
