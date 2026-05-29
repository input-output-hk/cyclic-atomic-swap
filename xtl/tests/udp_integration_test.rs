use cans_server::connection_manager::ConnectionManager;
use cans_server::daemon::{Daemon, DaemonArgs};
use cans_server::message::Message;
use cans_server::receipt::Receipt;
use cans_server::udp_transport::{UdpConnector, UdpListenerAdaptor, UdpTransport};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::info;
use cans_server::party_confidential::PartyConfidential;
use cans_server::swap_keys::SwapKeys;

async fn udp_broadcast_cluster(party_size: usize) {
    // Setup tracing for debugging if needed (usually captured by cargo test)
    let _ = tracing_subscriber::fmt::try_init();

    info!("Starting UDP integration test.");

    let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(party_size);
    let mut ready_receivers = Vec::with_capacity(party_size);
    let manager = Arc::new(ConnectionManager::<UdpTransport>::new());

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
            let listener = UdpListenerAdaptor::bind(&at)
                .await
                .expect("Failed to bind UDP listener");
            let args = DaemonArgs {
                party: PartyConfidential::new(at.clone(), SwapKeys::make_new()),
                connection_manager: manager,
                messages_buffer_size: party_size * party_size,
                liveness_tx: Some(tx),
                observer_tx: Some(observation_tx),
                shutdown_rx,
                listener,
            };
            let _ = Daemon::<UdpTransport, UdpConnector>::run(args).await;
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
        Daemon::<UdpTransport, UdpConnector>::ask(
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

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_udp_broadcast() {
    udp_broadcast_cluster(128).await;
}
