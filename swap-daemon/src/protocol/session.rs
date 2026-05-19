use std::collections::{BTreeMap, HashMap, HashSet};

use rand::RngCore;
use tracing::{error, info};
use crate::{
    cryptography::multisig::{adapt_role, finalize_role, transition_to_round_two},
    networking::{broadcast, new_connection_pool},
    protocol::{
        leader_election::{
            self, broadcast_leader_nonce, compute_leader, received_leader_commitment,
            received_leader_nonce, start_leader_election,
        },
        lock_funds::{broadcast_my_lock_tx, build_lock_txs},
        refund::begin_refund_signing,
        spend::{begin_spend_signing, broadcast_my_spend_tx},
    },
    types::{
        DaemonConfig, Envelope, ParticipantId, Participants, SessionId, StartBlock,
        SwapKeys, SwapSession, SwapState, TxRole, WireMessage,
    },
    utils::{
        all_adaptor_points_received, all_adaptor_secrets_received, all_leader_commitments_received,
        all_leader_nonces_received, all_partial_sigs_received_for,
        all_partial_sigs_received_for_all_refund_and_spend_txs, all_schnorr_nonces_received_for,
        check_cyclic, get_my_id, get_other_addresses,
    },
};


impl SwapSession {

    /// Creates a new instance of the cyclic swap structure.
    ///
    /// # Parameters
    ///
    /// * `id` - The unique identifier for the session.
    /// * `participants` - A mapping of participant information involved in the swap. Must form a cyclic swap structure.
    ///   If the participants do not form a cyclic structure, the function will panic.
    /// * `start_block` - The starting block in which the swap takes place.
    /// * `start_slot` - The slot at which the swap session begins execution.
    /// * `bitcoin_fee` - The transaction fee required for Bitcoin transactions during the swap.
    /// * `cardano_fee` - The transaction fee required for Cardano transactions during the swap.
    ///
    /// # Returns
    ///
    /// A new instance of the swap structure initialized with the given parameters.
    ///
    /// # Panics
    ///
    /// This function will panic if:
    /// * The participants do not form a cyclic swap (determined by the `check_cyclic` function).
    /// * An internal operation related to secret or adaptor point generation fails.
    ///
    /// # Details
    ///
    /// * Generates a unique secret scalar (`adaptor_secret`) for the current participant.
    ///   - This is serialized and stored in the `adaptor_secrets` map.
    ///   - Uses the secret scalar to compute its corresponding adaptor point, which is stored in the `adaptor_points` map.
    /// * The `my_id` field is derived based on the `is_me` flag within the `participants` data, identifying the current participant.
    /// * The refund window is set to 604,800 seconds (7 days) to allow for transactions to settle in testnets or specific environments.
    /// * Initializes various fields such as `musig_sessions`, `lock_txs`, `state_history`, and other relevant data structures for managing the swap process.
    ///
    /// ```
    pub fn new(id: SessionId, participants: Participants, start_block: StartBlock, start_slot: u64, bitcoin_fee: u64, cardano_fee: u64) -> Self {
        if !check_cyclic(&participants) {
            panic!("Not a cyclic swap.");
        }
        let mut secret_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret_bytes);
        let adaptor_secret = musig2::secp::Scalar::from_slice(&secret_bytes).unwrap();
        let adaptor_point = adaptor_secret.base_point_mul();

        let my_id = participants.values().find(|p| p.is_me).unwrap().id;

        let mut adaptor_secrets = HashMap::new();
        adaptor_secrets.insert(my_id, hex::encode(adaptor_secret.serialize()));

        let mut adaptor_points = HashMap::new();
        adaptor_points.insert(my_id, adaptor_point.to_string());
        Self {
            id,
            participants,
            start_block,
            start_slot,
            state: SwapState::Initialized,
            state_history: vec![SwapState::Initialized],
            leader_commitments: BTreeMap::new(),
            leader_nonces: BTreeMap::new(),
            leader: None,
            bitcoin_fee,
            cardano_fee,
            refund_window_secs: 604_800, // 7 days; regtest mines ~1 block/s so 6000 opens too fast
            musig_sessions: HashMap::new(),
            schnorr_nonces: HashMap::new(),
            partial_sigs: HashMap::new(),
            lock_txs: HashMap::new(),
            unsigned_txs: HashMap::new(),
            signed_txs: HashMap::new(),
            adaptor_sigs: HashMap::new(),
            lock_txs_broadcast: HashSet::new(),
            confirmed_lock_txs: HashSet::new(),
            adaptor_points,
            adaptor_secrets,

            cardano_collaterals: HashMap::new(),
            connection_pool: new_connection_pool(),
        }
    }

    /// Transitions the current state of the object to a new state and records the state change in the history.
    ///
    /// # Parameters
    /// * `state` - The new state (`SwapState`) to which the object should transition.
    ///
    pub fn transition_to(&mut self, state: SwapState) {
        self.state_history.push(state);
        self.state = state;
    }

    /// Handles incoming session messages for the swap protocol. Depending on the type of
    /// `WireMessage` received, this function updates the internal state machine and progresses
    /// through the swap protocol. Each `WireMessage` represents a specific step or action in
    /// the swap protocol, and is handled accordingly.
    ///
    /// # Parameters
    ///
    /// * `wire_message` - The incoming `WireMessage` that contains protocol-relevant data.
    /// * `participant_id` - The ID of the participant sending the message.
    /// * `keys` - An object containing cryptographic keys used for signing transactions.
    /// * `config` - Configuration settings for the daemon.
    ///
    /// # Behavior
    ///
    /// The behavior of this function is determined by the type of `WireMessage` received.
    /// The possible processing logic includes the following cases:
    ///
    /// 1. **`WireMessage::AdaptorPoint`**:
    ///    - Adds the received adaptor point to the state.
    ///    - Checks if all adaptor points have been received and transitions to
    ///      `SwapState::AwaitingLeaderCommitments` if applicable.
    ///    - Starts leader election if this participant's commitment is missing.
    ///
    /// 2. **`WireMessage::LeaderElectionCommitment`**:
    ///    - Starts leader election on receiving the first leader commitment.
    ///    - Processes and stores the received leader commitment.
    ///    - Broadcasts this participant's nonce upon receiving all commitments.
    ///
    /// 3. **`WireMessage::LeaderElectionNonce`**:
    ///    - Processes the received leader nonce and stores it.
    ///    - When all leader nonces are received, computes which participant is the leader,
    ///      transitions to `SwapState::RefundTxsSigning`, and initializes signing processes.
    ///
    /// 4. **`WireMessage::SchnorrNonce`**:
    ///    - Stores the received Schnorr nonce for this participant and role.
    ///    - Checks if all nonces for a specific role are received, and transitions
    ///      to the next round of signing if ready.
    ///
    /// 5. **`WireMessage::PartialSignature`**:
    ///    - Stores received partial signatures for this participant and role.
    ///    - Checks if all partial signatures for a role are received, which triggers finalization
    ///      for that role.
    ///    - Upon signing all refund and spend transactions, transitions to the funding phase and
    ///      attempts to broadcast this participant's lock transaction.
    ///
    /// 6. **`WireMessage::LockTxBroadcast`**:
    ///    - Tracks lock transactions broadcasted by participants. This enables the tracking
    ///      of lock transaction confirmations.
    ///
    /// 7. **`WireMessage::SecretReveal`**:
    ///    - Stores the adaptor secret revealed by a participant.
    ///    - If all adaptor secrets are received, initiates the spend process for this participant
    ///      if they are the leader or transitions to waiting for the leader's spend transaction.
    ///
    /// 8. **`WireMessage::SpendTxBroadcast`**:
    ///    - Updates the local state when a spend transaction broadcast is detected.
    ///
    /// # Preconditions
    /// - The state of the swap must not already be in a terminal state
    ///   (`SwapState::Completed`, `SwapState::Refunded`, `SwapState::Failed`).
    /// - The function expects cryptographic keys, participant information,
    ///   and configuration to be properly initialized.
    ///
    /// # State Transitions
    ///
    /// This function makes several state transitions during the protocol, including:
    /// - `SwapState::AwaitingAdaptorPoints` → `SwapState::AwaitingLeaderCommitments`
    /// - `SwapState::AwaitingLeaderCommitments` → `SwapState::AwaitingLeaderNonces`
    /// - `SwapState::AwaitingLeaderNonces` → `SwapState::RefundTxsSigning`
    /// - `SwapState::RefundTxsSigning` → `SwapState::Funding`
    /// - `SwapState::Funding` → `SwapState::AwaitingLockConfirmations` or `SwapState::Failed`
    /// - `SwapState::Claiming` → `SwapState::Completed`
    ///
    /// # Notes
    ///
    /// - The function uses helper functions such as `all_adaptor_points_received`,
    ///   `all_leader_commitments_received`, and others to determine protocol progress.
    /// - Logging plays a key role in monitoring the progress of the swap and debugging potential issues.
    ///
    /// # Errors
    /// - This function does not explicitly return errors but relies on logging to capture
    ///   errors that may occur during message handling or cryptographic processes.
    pub async fn handle_session_message(
        &mut self,
        wire_message: WireMessage,
        participant_id: ParticipantId,
        keys: SwapKeys,
        config: &DaemonConfig,
    ) {
        if matches!(self.state, SwapState::Completed | SwapState::Refunded | SwapState::Failed) {
            return;
        }
        match wire_message {
            WireMessage::AdaptorPoint(point) => {
                info!(
                    "received adaptor point from participant {} ({}/{})",
                    participant_id,
                    self.adaptor_points.len() + 1,
                    self.participants.len()
                );
                self.adaptor_points.insert(participant_id, point);
                if all_adaptor_points_received(self)
                    && self.state == SwapState::AwaitingAdaptorPoints
                {
                    info!("all adaptor points received, starting leader election");
                    self.transition_to(SwapState::AwaitingLeaderElectionCommitments);
                    let my_id = *get_my_id(&self.participants);
                    if !self.leader_commitments.contains_key(&my_id) {
                        leader_election::start_leader_election(self).await;
                    }
                }
            }
            WireMessage::LeaderElectionCommitment(commitment) => {
                // Kick off our own election exactly once, on the first commitment we receive.
                let my_id = *get_my_id(&self.participants);
                if !self.leader_commitments.contains_key(&my_id) {
                    start_leader_election(self).await;
                    self.transition_to(SwapState::AwaitingLeaderElectionCommitments);
                }
                received_leader_commitment(self, commitment, &participant_id).await;
                // Broadcast our nonce exactly once, when we first have all commitments.
                if all_leader_commitments_received(self)
                    && self.state != SwapState::AwaitingLeaderElectionNonces
                {
                    let my_nonce = self.leader_nonces.get(&my_id).unwrap();
                    broadcast_leader_nonce(self, *my_nonce).await;
                    self.transition_to(SwapState::AwaitingLeaderElectionNonces);
                }
            }
            WireMessage::LeaderElectionNonce(nonce) => {
                received_leader_nonce(self, nonce, &participant_id);
                if all_leader_nonces_received(self) && self.musig_sessions.is_empty() {
                    compute_leader(self);
                    self.transition_to(SwapState::RefundAndSpendTxsSigning);
                    build_lock_txs(self, config);
                    begin_refund_signing(self, &keys).await;
                    begin_spend_signing(self, &keys).await;
                }
            }
            WireMessage::SchnorrNonce { role, nonce } => {
                info!(
                    "received nonce for role {:?} from participant {}",
                    role, participant_id
                );
                self.schnorr_nonces
                    .entry(participant_id)
                    .or_insert_with(HashMap::new)
                    .insert(role, nonce);

                info!(
                    "total nonces for role {:?}: {}",
                    role,
                    self.schnorr_nonces
                        .values()
                        .filter(|m| m.contains_key(&role))
                        .count()
                );
                // Do we need all schnorr nonces for all refunds txs, or can we just proceed
                // on a per refund tx basis? Now we just proceed on a per tx basis. This may not be enough.
                if all_schnorr_nonces_received_for(self, role) {
                    info!(
                        "all nonces received for role {:?}, transitioning to round two",
                        role
                    );
                    transition_to_round_two(self, &keys, role)
                        .await;
                }
            }
            WireMessage::PartialSignature { role, sig } => {
                info!(
                    "received partial sig for role {:?} from participant {}",
                    role, participant_id
                );

                self.partial_sigs
                    .entry(participant_id)
                    .or_insert_with(HashMap::new)
                    .insert(role, sig);

                info!(
                    "total partial sigs for role {:?}: {}",
                    role,
                    self.partial_sigs
                        .values()
                        .filter(|m| m.contains_key(&role))
                        .count()
                );

                if all_partial_sigs_received_for(&self, role) {
                    info!("all partial sigs received for role {:?}, finalizing", role);
                    finalize_role(self, &keys, role, config).await;
                    info!(
                        "after finalize_role, signed_txs: {:?}",
                        self.signed_txs.keys().collect::<Vec<_>>()
                    );
                    info!(
                        "after finalize_role, adaptor_sigs: {:?}",
                        self.adaptor_sigs.keys().collect::<Vec<_>>()
                    );
                }

                if all_partial_sigs_received_for_all_refund_and_spend_txs(self) {
                    info!("all refund and spend txs signed, transitioning to Funding");
                    self.transition_to(SwapState::Funding);
                    if broadcast_my_lock_tx(self, &keys, config).await {
                        self.transition_to(SwapState::AwaitingLockConfirmations);
                    } else {
                        self.transition_to(SwapState::Failed);
                    }
                }
            }
            WireMessage::LockTxBroadcast => {
                self.lock_txs_broadcast.insert(participant_id);
                // daemon spawns a chain poller for this participant's lock tx
                // via event_tx — handled at daemon level
            }
            WireMessage::SecretReveal(secret) => {
                // secret is already hex-encoded — store directly without re-encoding
                self.adaptor_secrets
                    .insert(participant_id, secret);

                if all_adaptor_secrets_received(self) {
                    let leader_id = self.leader.unwrap();
                    let my_id = *get_my_id(&self.participants);

                    if my_id == leader_id {
                        // leader only adapts and broadcasts their own spend tx
                        let role = TxRole::Spend(my_id);
                        if self.adaptor_sigs.contains_key(&role) {
                            adapt_role(self, role, config).await;
                        }
                        self.transition_to(SwapState::Claiming);
                        broadcast_my_spend_tx(self, &keys, &config).await;

                        let addresses = get_other_addresses(&self.participants);
                        let envelope = Envelope::new(self.id, my_id, WireMessage::SpendTxBroadcast);

                        if let Err(e) = broadcast(&addresses, &envelope, &self.connection_pool).await {
                            error!("spend tx broadcast notification failed: {e}");
                        }

                        self.transition_to(SwapState::Completed);
                    } else {
                        // non-leader — wait for leader's spend tx on chain to extract secret
                        self.transition_to(SwapState::AwaitingLeaderSpend);
                    }
                }
            }

            WireMessage::SpendTxBroadcast => {
                info!("received SpendTxBroadcast from leader");
            }
        }
    }
}
