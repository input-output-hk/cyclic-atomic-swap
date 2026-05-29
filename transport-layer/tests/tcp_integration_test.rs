use cans_server::connection_manager::ConnectionManager;
use cans_server::daemon::{Daemon, DaemonArgs};
use cans_server::message::Message;
use cans_server::receipt::Receipt;
use cans_server::tcp_transport::{TcpConnector, TcpListenerAdaptor, TcpTransport};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::info;
use cans_server::party_confidential::PartyConfidential;
use cans_server::swap_keys::SwapKeys;

async fn tcp_broadcast_cluster(party_size: usize) {
    // Setup tracing for debugging if needed (usually captured by cargo test)
    let _ = tracing_subscriber::fmt::try_init();

    info!("Starting TCP integration test.");

    let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(party_size);
    let mut ready_receivers = Vec::with_capacity(party_size);
    let manager = Arc::new(ConnectionManager::<TcpTransport>::new());

    let (observation_tx, mut observation_rx) =
        tokio::sync::mpsc::channel::<Receipt>(party_size);
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let mut shutdown_rx = shutdown_tx.subscribe();
    let shutdown_tx_clone = shutdown_tx.clone();

    let completion_handle = tokio::spawn(async move {
        let mut count = 0;
        while let Some(receipt) = observation_rx.recv().await {
            count += 1;
            info!(ulid = %receipt.id(), ack = %receipt.ack().len(), nak = %receipt.nak().len(), "Observed completion in test.");
            if count == party_size {
                info!("ALL DAEMONS REPORTED OBSERVED COMPLETION IN TEST");
                let _ = shutdown_tx_clone.send(());
                break;
            }
        }
    });

    for i in 0usize..party_size {
        // Use a different port range than __main to avoid conflicts if main is running
        let at = format!("127.0.0.1:{}", 10000 + i);
        let (tx, rx) = tokio::sync::oneshot::channel();
        ready_receivers.push(rx);
        let observation_tx = observation_tx.clone();
        let manager = manager.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            let listener = TcpListenerAdaptor::bind(&at)
                .await
                .expect("Failed to bind TCP listener");
            let args = DaemonArgs {
                party: PartyConfidential::new(at.clone(), SwapKeys::make_new()),
                connection_manager: manager,
                messages_buffer_size: party_size,
                liveness_tx: Some(tx),
                observer_tx: Some(observation_tx),
                shutdown_rx,
                listener,
            };
            let _ = Daemon::<TcpTransport, TcpConnector>::run(args).await;
        });
        handles.push(handle);
    }

    // Wait for all daemons to be ready
    for rx in ready_receivers {
        rx.await.expect("Daemon failed to start");
    }

    let to_array = (0usize..party_size)
        .map(|i| format!("127.0.0.1:{}", 10000 + i))
        .collect::<Vec<String>>();

    for i in 0..party_size {
        let id = i.to_string();
        let from = format!("127.0.0.1:{}", 10000 + i);
        let to = to_array.clone();
        Daemon::<TcpTransport, TcpConnector>::ask(
            manager.clone(),
            party_size,
            id,
            from,
            to,
            Message::Text(format!("message-{}", i)),
        )
        .await;
    }

    // Wait for completion or timeout
    let timeout = party_size as u64 / 8u64;
    tokio::select! {
        _ = shutdown_rx.recv() => {
            info!("Test received shutdown signal.");
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(timeout)) => {
            panic!("Test timed out after {} seconds", timeout);
        }
    }

    // Cleanup
    for handle in handles {
        handle.abort(); // Force stop daemons if they haven't stopped yet
    }
    completion_handle.abort();
}

/// ```rust
/// Tests the functionality of a TCP broadcast cluster with multiple nodes.
///
/// This asynchronous test utilizes the Tokio framework with a multi-threaded runtime.
/// It spawns a test runtime with a specified number of worker threads and performs the
/// `tcp_broadcast_cluster` operation to ensure it behaves as expected in a concurrent environment.
///
/// # Configuration
/// - `flavor = "multi_thread"`: Runs the test with a multi-threaded Tokio runtime.
/// - `worker_threads = 8`: Allocates 8 worker threads to handle the asynchronous workload.
///
/// # What it tests
/// This test calls the `tcp_broadcast_cluster` function with 128 nodes, verifying
/// that the broadcast functionality of the cluster operates as intended.
///
/// # Panics
/// This test will panic if:
/// - The `tcp_broadcast_cluster` function encounters unexpected behavior or fails to complete.
/// - The system runs out of resources to accommodate the 128-node cluster.
///
/// # Requirements
/// Ensure the Tokio runtime is correctly configured and the `tcp_broadcast_cluster` function
/// is implemented and tested for compatibility within an asynchronous context.
///
/// # Examples
/// ```rs
/// #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
/// async fn test_tcp_broadcast() {
///     tcp_broadcast_cluster(128).await;
/// }
///
/// # Note
/// This test is designed for use in high-concurrency scenarios and may take
/// time to complete depending on system resources and implementation.
/// Tested with worker_threads = 24 and party_size = 1024 on Intel® Core™ Ultra 7 265U × 14 - 64.0 GiB RAM with Linux 6.19.12-200.fc43.x86_64.
/// ```
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_tcp_broadcast() {
    tcp_broadcast_cluster(128).await;
}
