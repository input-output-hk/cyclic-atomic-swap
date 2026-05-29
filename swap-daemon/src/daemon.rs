use crate::{
    networking::{broadcast, handle_connection},
    transport::tcp::{TcpTransport, TcpConnector},
    protocol::{
        adaptor_nonce::broadcast_adaptor_point,
        chain_monitor::{check_leader_spend_confirmed, check_lock_tx_confirmed, check_refund_window_open, validate_funding_utxos},
        refund::broadcast_my_refund_tx,
        secret::extract_secret_and_adapt,
        spend::broadcast_my_spend_tx,
    },
    types::{
        ChainPollTarget, Daemon, DaemonConfig, DaemonEvent, Envelope, SessionId, SwapKeys,
        SwapSession, SwapState, WireMessage,
    },
    utils::{all_lock_txs_confirmed, get_my_id, get_other_addresses},
};
use std::{collections::HashMap, time::Duration};
use tokio::{net::TcpListener, sync::mpsc};
use tracing::{error, info};

// Have the daemon listen to a socket asynchronously for messages using Tokio.
// It will handle connections on a loop.
impl Daemon {
    /// Creates a new instance of the struct.
    ///
    /// # Parameters
    ///
    /// * `swap_keys` - An instance of `SwapKeys` used to manage key-related operations.
    /// * `config` - An instance of `DaemonConfig` containing the configuration settings.
    ///
    /// # Returns
    ///
    /// Returns a new instance of `Self` with the following fields initialized:
    /// - `sessions`: An empty `HashMap` for tracking active sessions.
    /// - `swap_keys`: The provided `SwapKeys` instance.
    /// - `config`: The provided `DaemonConfig` instance.
    /// - `active_pollers`: An empty `HashMap` for managing active poller states.
    pub fn new(swap_keys: SwapKeys, config: DaemonConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            swap_keys,
            config,
            active_pollers: HashMap::new(),
        }
    }

    /// Inserts a new `SwapSession` into the `sessions` collection.
    ///
    /// # Parameters
    /// - `session`: The `SwapSession` instance to be added. It contains an `id`
    ///   which is used as the key in the `sessions` collection.
    ///
    pub fn insert_session(&mut self, session: SwapSession) {
        self.sessions.insert(session.id, session);
    }

    /// Starts a swap session for the specified session ID.
    ///
    /// This function validates funding UTXOs if the configuration requires it and transitions
    /// the session state accordingly. It also broadcasts the adaptor point for the session
    /// regardless of the current session state to ensure proper synchronization between peers.
    ///
    /// # Parameters
    /// - `session_id`: A `u64` representing the unique identifier of the session to start.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(Box<dyn std::error::Error>)` if:
    ///     - The `session_id` does not exist in the `sessions` map.
    ///
    /// # Notes
    /// - This function should ideally be called exactly once per session, but it handles cases
    ///   where the session state has already advanced due to peer messages. It avoids errors
    ///   when the session is no longer in its `Initialized` state.
    ///   1. You create the daemon
    //    2. You add a session
    //    3. You run the daemon (i.e., make it start listening to a port)
    //    4. You start a session (i.e., start broadcasting details)
    ///
    /// # Errors
    /// - Returns an error if the specified `session_id` is not found in the session map.
    ///
    pub async fn start_swap_session(
        &mut self,
        session_id: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if self.config.validate_utxos && !validate_funding_utxos(session, &self.config).await {
                session.transition_to(SwapState::Failed);
                return Ok(());
            }
            // Always broadcast our adaptor point. In practice start_swap_session is called
            // exactly once, but peer messages may have already advanced the session state
            // (race between TCP delivery and the explicit start call), so we must not error
            // on a non-Initialized state — we still need to send our adaptor point.
            broadcast_adaptor_point(session).await;
            if matches!(session.state, SwapState::Initialized) {
                session.transition_to(SwapState::AwaitingAdaptorPoints);
            }
            Ok(())
        } else {
            Err("Session Id does not exist".into())
        }
    }

    /// Injects a test event into the daemon's event handling system.
    ///
    /// # Parameters
    /// - `event`: The `DaemonEvent` to be injected for testing purposes.
    ///
    pub async fn inject_event_for_test(&mut self, event: DaemonEvent) {
        let (event_tx, _) = mpsc::channel::<DaemonEvent>(256);
        self.handle_event(event, event_tx).await;
    }

    /// Spawns a new asynchronous poller task to periodically send `ChainPoll` events to a channel.
    ///
    /// The poller operates independently and is responsible for continuously emitting `ChainPoll`
    /// events for a specific session and target at fixed intervals until the associated channel is closed.
    /// This function prevents the creation of duplicate pollers for the same `session_id` and `target`
    /// by checking an internal map of active pollers.
    ///
    /// # Parameters
    /// - `session_id`: The unique identifier for the session associated with the poller.
    /// - `target`: The target chain being polled.
    /// - `event_tx`: A sender channel through which the `ChainPoll` events are emitted.
    ///
    /// # Notes
    /// - This function is designed to run indefinitely until one of the termination conditions is met.
    /// - It is the caller's responsibility to manage the lifecycle of the poller through the abort handle
    ///   or other means.
    /// - It doesn't know or care about any state — it just sends events forever until the channel closes.
    /// 
    fn spawn_poller(
        &mut self,
        session_id: SessionId,
        target: ChainPollTarget,
        event_tx: mpsc::Sender<DaemonEvent>,
    ) {
        if self.active_pollers.contains_key(&(session_id, target)) {
            return;
        }

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                if event_tx
                    .send(DaemonEvent::ChainPoll { session_id, target })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        self.active_pollers.insert((session_id, target), handle.abort_handle());
    }

    /// Cancels and removes all pollers associated with a specific session.
    ///
    /// # Parameters
    ///
    /// * `session_id` - The identifier of the session for which all associated pollers need to be canceled.
    ///
    /// # Notes
    ///
    /// - If no pollers exist for the provided `session_id`, the function effectively does nothing.
    ///
    fn cancel_session_pollers(&mut self, session_id: SessionId) {
        let keys: Vec<_> = self.active_pollers.keys()
            .filter(|(sid, _)| *sid == session_id)
            .cloned()
            .collect();
        for key in keys {
            if let Some(handle) = self.active_pollers.remove(&key) {
                handle.abort();
            }
        }
    }

    /// Attempts to spawn pollers for a given session, if needed.
    ///
    /// This function evaluates the current state of a session and determines if chain poller tasks
    /// need to be spawned to track state changes on the blockchain. Pollers are spawned for different
    /// targets based on session conditions such as transaction statuses and swap progression.
    ///
    /// # Parameters
    ///
    /// * `session_id` - The unique identifier of the session for which pollers might be spawned.
    /// * `event_tx` - A channel sender (`mpsc::Sender`) used to transmit daemon events triggered by
    ///   spawned pollers.
    ///
    /// # Behavior
    ///
    /// The function evaluates and spawns pollers for the following scenarios:
    ///
    /// 1. **Lock Transaction Pollers**:
    ///    - Spawns a poller for each participant whose lock transaction has been broadcast but not yet
    ///      confirmed.
    ///    - Excludes participants with pollers already active for their lock transactions.
    ///
    /// 2. **Leader Spend Transaction Poller**:
    ///    - Spawns a poller to monitor the leader's spend transaction when the session is in the
    ///      `AwaitingLeaderSpend` state.
    ///    - Ensures no duplicate pollers are active for this target.
    ///
    /// 3. **Refund Window Poller**:
    ///    - Spawns a poller after the lock transaction of the local participant is confirmed.
    ///    - Polls until the locktime is reached, enabling funds reclamation in case the swap stalls.
    ///    - Excludes this poller for sessions in finalized states (`Completed`, `Refunded`, or `Failed`).
    ///
    /// If a poller for a given target within the session is already active, it will not spawn a new one.
    ///
    /// # Session Lookups and Poller Spawning
    ///
    /// - A session is retrieved from `self.sessions` using the provided `session_id`. If the session is
    ///   not found, no pollers are spawned.
    /// - Newly required poller targets are identified and spawned using `self.spawn_poller`, which
    ///   clones the provided `event_tx` channel for each new poller.
    ///
    /// # Preconditions
    ///
    /// - A valid session matching `session_id` must exist in `self.sessions`.
    /// - `self.spawn_poller` must handle the creation of pollers for each specified `ChainPollTarget`.
    ///
    /// # Notes
    ///
    /// - The logic for determining pollers ensures no redundant pollers are started for the same
    ///   session/target combination.
    /// - Internal fields like `self.active_pollers` track ongoing pollers, and `session.state` along
    ///   with other session details determine which pollers are necessary.
    /// - Notable conditions for spawning pollers:
    ///   - `lock_txs_broadcast.contains` — has this participant broadcast their lock tx?
    ///   - `confirmed_lock_txs.contains` — is it not yet confirmed?
    ///   - `active_pollers.contains` — is there not already a poller running for it?
    /// 
    fn maybe_spawn_pollers(
        &mut self,
        session_id: SessionId,
        event_tx: mpsc::Sender<DaemonEvent>,
    ) {
        let to_spawn: Vec<ChainPollTarget> = if let Some(session) = self.sessions.get(&session_id) {
            let mut targets = vec![];

            // lock tx pollers
            for participant_id in session.participants.keys().copied() {
                let target = ChainPollTarget::LockTx { participant_id };
                if session.lock_txs_broadcast.contains(&participant_id)
                    && !session.confirmed_lock_txs.contains(&participant_id)
                    && !self.active_pollers.contains_key(&(session_id, target))
                {
                    targets.push(target);
                }
            }

            // leader spend tx poller — watch in both states
            let watching_leader_spend = session.state == SwapState::AwaitingLeaderSpend;
            if watching_leader_spend {
                let target = ChainPollTarget::LeaderSpendTx {
                    leader_id: session.leader.unwrap(),
                };
                if !self.active_pollers.contains_key(&(session_id, target)) {
                    targets.push(target);
                }
            }

            // refund window poller — once our own lock is confirmed, poll until the
            // locktime is reached so we can reclaim funds if the swap stalls
            let my_id = *get_my_id(&session.participants);
            let refund_relevant = session.confirmed_lock_txs.contains(&my_id)
                && !matches!(
                    session.state,
                    SwapState::Completed | SwapState::Refunded | SwapState::Failed
                );
            if refund_relevant {
                let target = ChainPollTarget::RefundWindow { participant_id: my_id };
                if !self.active_pollers.contains_key(&(session_id, target)) {
                    targets.push(target);
                }
            }

            targets
        } else {
            vec![]
        };

        for target in to_spawn {
            self.spawn_poller(session_id, target, event_tx.clone());
        }
    }

    /// Handles various `DaemonEvent` types related to peer messaging and blockchain polling.
    ///
    /// # Parameters
    /// - `event`: The `DaemonEvent` instance representing the specific event to handle.
    /// - `event_tx`: A sender for propagating new `DaemonEvent` instances.
    ///
    /// # Behavior
    /// This method processes the `DaemonEvent` in two main categories:
    ///
    /// ## 1. `DaemonEvent::PeerMessage`
    /// - Handles peer-to-peer messages related to a session.
    /// - Decodes the event, retrieves the session by its `session_id`, and processes the event message.
    /// - If the session reaches a terminal state (`SwapState::Completed`, `SwapState::Refunded`,
    ///   or `SwapState::Failed`), the session event pollers are canceled. Otherwise, ensures the
    ///   necessary pollers are spawned to monitor further updates.
    ///
    /// ## 2. `DaemonEvent::ChainPoll`
    /// - Processes blockchain-related polling events. Depending on the `ChainPollTarget`, performs
    ///   operations such as:
    ///     1. Verifying confirmation of lock transactions for a specific participant.
    ///     2. Monitoring and confirming the spending of the leader's transaction.
    ///     3. Ensuring correct refund handling during the refund window.
    /// - Transitions the session's state appropriately based on the detected chain state.
    /// - Stops polling on terminal states (`SwapState::Completed`, `SwapState::Refunded`, etc.).
    /// - Spawns additional pollers when required (e.g., for monitoring refund windows after certain
    ///   conditions are met).
    ///
    /// # State Transitions
    /// - The session can transition between states like `AwaitingLeaderSpend`, `AwaitingSecrets`,
    ///   `Claiming`, `Completed`, `Refunded`, or `Failed` based on event outcomes and blockchain
    ///   state.
    ///
    /// # Logging
    /// - Logs are generated to indicate received messages, state transitions, and errors during
    ///   operations like broadcasting or transaction confirmations.
    ///
    /// # Errors
    /// - In case of invalid session IDs or failed operations (e.g., broadcasting a message), the system
    ///   will log the errors but continue handling other events as applicable.
    /// 
    async fn handle_event(&mut self, event: DaemonEvent, event_tx: mpsc::Sender<DaemonEvent>) {
        match event {
            DaemonEvent::PeerMessage { envelope, from } => {
                info!("Message received from {from}");
                let keys = self.swap_keys.clone();
                let session_id = envelope.session_id;

                let session = self
                    .sessions
                    .get_mut(&session_id)
                    .unwrap_or_else(|| panic!("Session {} does not exist", session_id));

                session
                    .handle_session_message(
                        envelope.msg,
                        envelope.participant_id,
                        keys,
                        &self.config,
                    )
                    .await;

                let is_terminal = matches!(
                    self.sessions[&session_id].state,
                    SwapState::Completed | SwapState::Refunded | SwapState::Failed
                );
                if is_terminal {
                    self.cancel_session_pollers(session_id);
                } else {
                    self.maybe_spawn_pollers(session_id, event_tx.clone());
                }
            }
            DaemonEvent::ChainPoll { session_id, target } => {
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    match target {
                        ChainPollTarget::LockTx { participant_id } => {
                            if !session.confirmed_lock_txs.contains(&participant_id) {
                                check_lock_tx_confirmed(session, participant_id, &self.config)
                                    .await;
                                if session.confirmed_lock_txs.contains(&participant_id) {
                                    self.active_pollers.remove(&(session_id, target));
                                }
                                if all_lock_txs_confirmed(session) {
                                    let my_id: u8 = *get_my_id(&session.participants);
                                    let my_secret =
                                        session.adaptor_secrets.get(&my_id).unwrap().clone();
                                    if session.leader != Some(my_id) {
                                        session.transition_to(SwapState::AwaitingLeaderSpend);
                                        let addresses = get_other_addresses(&session.participants);
                                        let envelope = Envelope::new(
                                            session.id,
                                            my_id,
                                            WireMessage::SecretReveal(my_secret),
                                        );
                                        if let Err(e) = broadcast::<TcpTransport, TcpConnector>(&addresses, &envelope, &session.connection_pool).await {
                                            error!("secret reveal broadcast failed: {e}");
                                        }
                                        // spawn poller for leader's spend tx now that we're watching
                                        self.maybe_spawn_pollers(session_id, event_tx.clone());
                                    } else {
                                        // Leader waits for secrets (which come through peer-messaging)
                                        session.transition_to(SwapState::AwaitingSecrets);
                                    }
                                }
                            }
                        }
                        ChainPollTarget::LeaderSpendTx { leader_id } => {
                            let watching = session.state == SwapState::AwaitingLeaderSpend;
                            if watching {
                                if check_leader_spend_confirmed(session, leader_id, &self.config)
                                    .await
                                {
                                    let extracted = extract_secret_and_adapt(session, leader_id, &self.config)
                                        .await;
                                    if extracted {
                                        session.transition_to(SwapState::Claiming);
                                        broadcast_my_spend_tx(session, &self.swap_keys, &self.config).await;
                                        session.transition_to(SwapState::Completed);
                                        self.cancel_session_pollers(session_id);
                                    }
                                    // else: UTxO index lag or spend tx not yet propagated — keep polling.
                                    // The RefundWindow poller handles the actual refund scenario.
                                }
                            }
                        }
                        ChainPollTarget::RefundWindow { participant_id } => {
                            let is_done = matches!(
                                session.state,
                                SwapState::Completed | SwapState::Refunded | SwapState::Failed
                            );
                            if !is_done && check_refund_window_open(session, participant_id, &self.config).await {
                                let ok = broadcast_my_refund_tx(session, &self.swap_keys, &self.config).await;
                                session.transition_to(if ok { SwapState::Refunded } else { SwapState::Failed });
                                self.cancel_session_pollers(session_id);
                            }
                        }
                    }
                }
                // Spawn any new pollers that may be needed after chain state changed
                // (e.g. RefundWindow poller starts once our own lock is confirmed).
                self.maybe_spawn_pollers(session_id, event_tx.clone());
            }
        }
    }

    /// Runs the main event loop of the daemon.
    ///
    /// This asynchronous function sets up a `TcpListener` to listen for incoming
    /// connections on the configured TCP address. It spawns a background task
    /// that accepts new connections and dispatches them to a connection handler.
    ///
    /// The function uses a `tokio::sync::mpsc` channel to process events
    /// from various parts of the application. It listens for events in a loop
    /// and delegates the handling of each event through the `self.handle_event` method.
    ///
    /// ## Key Components
    /// 
    /// - **TCP Listener**: Binds to the configured TCP address and listens for incoming
    ///   connections. Each accepted connection is passed to the `handle_connection`
    ///   function in a separate task.
    /// - **Event Loop**: Listens for events on the receiver (`event_rx`) end of an
    ///   `mpsc` channel. This loop ensures a centralized handling mechanism for all
    ///   events received.
    /// - **Concurrency**: The function makes use of `tokio::select!` and `tokio::spawn`
    ///   to handle tasks concurrently. For instance, new connections and incoming events
    ///   are processed asynchronously but independently.
    ///
    /// # Errors
    /// 
    /// - Returns an error wrapped in `Box<dyn std::error::Error>` if the listener
    ///   fails to bind to the address, or other unexpected errors occur during operation.
    /// - Logs errors if connection handling or event processing encounters issues.
    ///
    /// 
    /// # Notes
    /// 
    /// - `tokio select!` is very important here as it lets the loop continue after either task.
    ///
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(&self.config.tcp_address).await?;
        info!("Daemon listening on {}", self.config.tcp_address);

        let (event_tx, mut event_rx) = mpsc::channel::<DaemonEvent>(4096);

        let accept_event_tx = event_tx.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        let tx = accept_event_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(TcpTransport::new(socket), addr.to_string(), tx).await {
                                error!("Connection handler error for {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {e}");
                        break;
                    }
                }
            }
        });

        loop {
            match event_rx.recv().await {
                Some(event) => self.handle_event(event, event_tx.clone()).await,
                None => {
                    error!("Event channel closed");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Asynchronously runs the daemon in a shared mode, enabling event handling without
    /// monopolizing the runtime loop. This function is suitable for integration tests that
    /// monitor session state.
    ///
    /// # Parameters
    ///
    /// * `daemon` - An `Arc` wrapped `RwLock` providing shared, thread-safe access to the daemon instance.
    ///
    /// # Returns
    ///
    /// * `Result<(), Box<dyn std::error::Error>>` - Returns `Ok(())` if the daemon runs successfully.
    ///   Returns an error if an unrecoverable issue occurs during execution.
    ///
   pub async fn run_shared(
        daemon: std::sync::Arc<tokio::sync::RwLock<Self>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tcp_address = daemon.read().await.config.tcp_address.clone();
        let listener = TcpListener::bind(&tcp_address).await?;
        info!("Daemon listening on {tcp_address}");

        let (event_tx, mut event_rx) = mpsc::channel::<DaemonEvent>(4096);

        // Accept loop runs in its own task so it is never blocked by event handling.
        // When handle_event awaits outgoing broadcasts, the accept loop keeps draining
        // the OS listen queue — preventing SYN drops under high connection load.
        let accept_event_tx = event_tx.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        let tx = accept_event_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(TcpTransport::new(socket), addr.to_string(), tx).await {
                                error!("Connection handler error for {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {e}");
                        break;
                    }
                }
            }
        });

        // Event processing loop — holds write lock only while handling each event.
        loop {
            match event_rx.recv().await {
                Some(event) => {
                    daemon.write().await.handle_event(event, event_tx.clone()).await;
                }
                None => {
                    error!("Event channel closed");
                    break;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        test_utils::{make_session, make_swap_keys},
        types::{Daemon, DaemonConfig},
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

    #[tokio::test]
    async fn start_swap_session_returns_error_for_missing_session() {
        let swap_keys = make_swap_keys();
        let mut daemon = Daemon::new(
            swap_keys,
            DaemonConfig::testnet("127.0.0.1:9000".to_string()),
        );

        let result = daemon.start_swap_session(999).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn start_swap_session_returns_ok_for_existing_session() {
        let swap_keys = make_swap_keys();
        let mut daemon = Daemon::new(
            swap_keys,
            DaemonConfig::testnet("127.0.0.1:9000".to_string()),
        );
        let session = make_session(42);

        daemon.insert_session(session);

        let result = daemon.start_swap_session(42).await;

        assert!(result.is_ok());
        assert!(daemon.sessions.contains_key(&42));
    }
}
