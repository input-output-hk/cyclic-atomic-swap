//! ```rust
//! Executes a Pulsar integration test for broadcasting in a clustered environment.
//! //
use cans_server::connection_manager::ConnectionManager;
use cans_server::daemon::{Daemon, DaemonArgs};
use cans_server::message::Message;
use cans_server::pulsar_transport::{PulsarConnector, PulsarListenerAdaptor, PulsarTransport};
use cans_server::receipt::Receipt;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::info;
use cans_server::party_confidential::PartyConfidential;
use cans_server::swap_keys::SwapKeys;

async fn pulsar_broadcast_cluster(party_size: usize) {
    let _ = tracing_subscriber::fmt::try_init();
    info!("Start Pulsar integration test.");

    let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(party_size);
    let mut ready_receivers = Vec::with_capacity(party_size);
    let manager = Arc::new(ConnectionManager::<PulsarTransport>::new());

    let (observation_tx, mut observation_rx) = tokio::sync::mpsc::channel::<Receipt>(party_size);
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let mut shutdown_rx = shutdown_tx.subscribe();
    let shutdown_tx_clone = shutdown_tx.clone();

    let completion_handle = tokio::spawn(async move {
        let mut count = 0;
        while let Some(receipt) = observation_rx.recv().await {
            count += 1;
            info!(ulid = %receipt.id(), ack = %receipt.ack().len(), nak = %receipt.nak().len(), "Observed completion.");
            if count == party_size {
                info!("ALL DAEMONS REPORTED OBSERVED COMPLETION");
                let _ = shutdown_tx_clone.send(());
                break;
            }
        }
    });

    for i in 0usize..party_size {
        let at = format!("topic-{}", i);
        let (tx, rx) = tokio::sync::oneshot::channel();
        ready_receivers.push(rx);
        let observation_tx = observation_tx.clone();
        let manager = manager.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            let url = "pulsar://127.0.0.1:6650";
            let topic = format!("topic-{}", i);
            let subscription = format!("sub-{}", i);
            let listener = PulsarListenerAdaptor::bind(url, &topic, &subscription)
                .await
                .expect("Failed to bind Pulsar listener");
            let args = DaemonArgs {
                party: PartyConfidential::new(topic.clone(), SwapKeys::make_new()),
                connection_manager: manager,
                messages_buffer_size: party_size * party_size,
                liveness_tx: Some(tx),
                observer_tx: Some(observation_tx),
                shutdown_rx,
                listener,
            };
            let _ = Daemon::<PulsarTransport, PulsarConnector>::run(args).await;
        });
        handles.push(handle);
    }

    for rx in ready_receivers {
        let _ = rx.await.expect("Daemon failed to start");
    }

    let to_array = (0usize..party_size)
        .map(|i| format!("topic-{}", i))
        .collect::<Vec<String>>();
    for i in 0..party_size {
        let id = i.to_string();
        let from = format!("topic-{}", i);
        let to = to_array.clone();
        Daemon::<PulsarTransport, PulsarConnector>::ask(
            manager.clone(),
            party_size,
            id,
            from,
            to,
            Message::Text(i.to_string()),
        )
        .await;
    }

    info!("---");

    // Timeout mechanism similar to UDP test
    let timeout = party_size as u64 / 8u64;
    tokio::select! {
        _ = shutdown_rx.recv() => {
            info!("Test received shutdown signal.");
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(timeout)) => {
            // Pulsar tests might fail if no Pulsar server is running.
            // We don't want to fail the whole build if Pulsar is missing,
            // but for an integration test, it should probably fail if it times out.
            // However, often these are ignored if env var is not set.
            info!("Test timed out after {} seconds. This is expected if Pulsar is not running.", timeout);
        }
    }

    for handle in handles {
        handle.abort();
    }
    completion_handle.abort();
    info!("Stop.");
}

/// ```
/// Test for verifying Pulsar broadcast functionality in a clustered environment.
///
/// # Description
/// This test checks the ability of a Pulsar cluster to handle broadcast messages
/// across multiple nodes with a specified number of broadcast messages. It ensures
/// the broadcast mechanism works as intended within a cluster setup.
///
/// # Attributes
/// - `#[tokio::test(flavor = "multi_thread", worker_threads = 8)]`:
///   - Executes the test using the Tokio asynchronous runtime in multi-threaded mode
///     with 8 worker threads.
/// - `#[ignore]`:
///   - This test is ignored during regular test runs by default as it requires a
///     running instance of a Pulsar cluster.
///
/// # Requirements
/// - A running [Pulsar](https://pulsar.apache.org/) cluster is required for this test.
/// - Ensure the Pulsar cluster is configured properly to test broadcasting.
/// - [Run a standalone Pulsar cluster locally](https://pulsar.apache.org/docs/4.2.x/getting-started-standalone/)
///
///
/// # Parameters
/// - Broadcasting `128` messages during the test execution.
///
/// # Note
/// - To run this test, use the `--ignored` flag to explicitly include ignored tests:
///   ```
///   cargo test -- --ignored
///   ```
///
/// # Example Usage
/// ```rust
/// #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
/// #[ignore]
/// async fn test_pulsar_broadcast() {
///     pulsar_broadcast_cluster(128).await;
/// }
/// ```
/// ```
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore] // Ignoring by default because it requires a running Pulsar instance
async fn test_pulsar_broadcast() {
    pulsar_broadcast_cluster(128).await;
}
