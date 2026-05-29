use cans_server::connection_manager::ConnectionManager;
use cans_server::daemon::{Daemon, DaemonArgs};
use cans_server::message::Message;
use cans_server::party::Party;
use cans_server::receipt::Receipt;
use cans_server::swap_description::SwapDescription;
use cans_server::tcp_transport::{TcpConnector, TcpListenerAdaptor, TcpTransport};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::info;
use cans_server::party_confidential::PartyConfidential;
use cans_server::swap_keys::SwapKeys;

async fn config_phase_tcp_cluster(party_size: usize) {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
    info!("Starting config phase test.");

    let connection_manager = Arc::new(ConnectionManager::<TcpTransport>::new());
    let mut ready_receivers = Vec::with_capacity(party_size);
    let (observation_tx, mut observation_rx) = tokio::sync::mpsc::channel::<Receipt>(party_size);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);
    let shutdown_tx_trigger = shutdown_tx.clone();

    let completion_handle = tokio::spawn(async move {
        while let Some(receipt) = observation_rx.recv().await {
            info!(ulid = %receipt.id(), ack = %receipt.ack().len(), nak = %receipt.nak().len(), "Observed completion in test.");
            info!("ALL DAEMONS REPORTED OBSERVED COMPLETION IN TEST");
            let _ = shutdown_tx_trigger.send(());
            break;
        }
    });

    let mut daemon_handles: Vec<JoinHandle<()>> = Vec::with_capacity(party_size);

    for i in 0usize..party_size {
        let at = format!("127.0.0.1:{}", 10000 + i);
        let connection_manager = connection_manager.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let observation_tx = observation_tx.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        ready_receivers.push(ready_rx);
        let handle = tokio::spawn(async move {
            let listener = TcpListenerAdaptor::bind(&at)
                .await
                .expect("Failed to bind TCP listener");
            let args = DaemonArgs {
                party: PartyConfidential::new(at.clone(), SwapKeys::make_new()),
                connection_manager,
                messages_buffer_size: party_size * party_size,
                liveness_tx: Some(ready_tx),
                observer_tx: Some(observation_tx),
                shutdown_rx,
                listener,
            };
            let _ = Daemon::<TcpTransport, TcpConnector>::run(args).await;
        });
        daemon_handles.push(handle);
    }

    // Wait for all daemons to be ready
    for rx in ready_receivers {
        rx.await.expect("Daemon failed to start");
    }

    let mut parties: Vec<Party> = Vec::with_capacity(party_size);
    for i in 0..party_size {
        let party = Party::new(format!("127.0.0.1:{}", 10000 + i));
        parties.push(party);
    }
    let swap_config = SwapDescription::new(42.to_string(), parties);
    let from = swap_config.parties().get(0).unwrap().address().to_string();
    let to = swap_config
        .parties()
        .iter()
        .map(|party| party.address().to_string())
        .collect();
    let content = Message::SwapDescription(swap_config.clone());

    Daemon::<TcpTransport, TcpConnector>::ask(
        connection_manager.clone(),
        party_size,
        1.to_string(),
        from,
        to,
        content,
    )
    .await;

    // Wait for completion or timeout
    let timeout = party_size as u64;
    tokio::select! {
        _ = shutdown_rx.recv() => {
            info!("Test received shutdown signal.");
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(timeout)) => {
            panic!("Test timed out after {} seconds", timeout);
        }
    }

    // Cleanup
    for daemon_handle in daemon_handles {
        daemon_handle.abort(); // Force stop daemons if they haven't stopped yet
    }
    completion_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_config_phase() {
    config_phase_tcp_cluster(3).await;
}
