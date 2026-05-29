use std::sync::Arc;

use crate::transport::connection_pool::ConnectionPool;
use crate::transport::Transport;
use crate::types::{DaemonEvent, Envelope};
use serde_json;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::TcpStream,
    sync::{mpsc, Mutex},
};
use tracing::{error, info};
use crate::transport::tcp_transport::TcpTransport;

pub async fn handle_connection(
    socket: TcpStream,
    from: String,
    event_tx: mpsc::Sender<DaemonEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let reader = BufReader::new(socket);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let envelope = parse_envelope(&line)?;
        event_tx
            .send(DaemonEvent::PeerMessage {
                envelope: envelope,
                from: from.clone(),
            })
            .await?;
    }

    info!("Connection closed by {}", from);
    Ok(())
}

pub fn parse_envelope(line: &str) -> Result<Envelope, Box<dyn std::error::Error + Send + Sync>> {
    let envelope: Envelope = serde_json::from_str(line)?;
    Ok(envelope)
}

pub async fn broadcast(
    addresses: &[String],
    envelope: &Envelope,
    pool: &ConnectionPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload = serde_json::to_vec(envelope)?;
    let mut framed = payload;
    framed.push(b'\n');

    let mut tasks = Vec::new();

    for addr in addresses {
        let addr = addr.clone();
        let pool = pool.clone();
        let framed = framed.clone();

        tasks.push(tokio::spawn(async move {
            // Check pool without holding lock during connect
            let stream_arc = pool.lock().await.get(&addr).cloned();

            let stream_arc = if let Some(s) = stream_arc {
                s
            } else {
                match TcpStream::connect(&addr).await {
                    Ok(stream) => {
                        let s = Arc::new(Mutex::new(stream));
                        let mut guard = pool.lock().await;
                        // Use entry so a racing task's connection wins
                        guard.entry(addr.clone()).or_insert(s).clone()
                    }
                    Err(e) => {
                        error!("failed to connect to {}: {}", addr, e);
                        return;
                    }
                }
            };

            let mut stream = stream_arc.lock().await;
            // if let Err(e) = stream.write_all(&framed).await {

            if let Err(e) = TcpTransport::new(&mut *stream).send(&framed).await {
                error!("failed to write to {}: {}", addr, e);
                pool.lock().await.remove(&addr);
            }
        }));
    }

    for task in tasks {
        let _ = task.await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::connection_pool::new_connection_pool;
    use crate::types::{TxRole, WireMessage};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::{TcpListener, TcpStream},
        sync::mpsc,
        time::{timeout, Duration},
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

    #[test]
    fn parse_envelope_from_json() {
        let envelope = Envelope {
            session_id: 42,
            participant_id: 7,
            msg: WireMessage::SchnorrNonce {
                role: TxRole::Refund(1),
                nonce: "abc123".to_string(),
            },
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let parsed = parse_envelope(&json).unwrap();

        assert_eq!(parsed, envelope);
    }

    #[test]
    fn envelope_json_round_trip_secret_reveal() {
        let envelope = Envelope {
            session_id: 99,
            participant_id: 3,
            msg: WireMessage::SecretReveal("secret".to_string()),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: Envelope = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, envelope);
    }

    #[tokio::test]
    async fn handle_connection_emits_peer_message_event_from_json_line() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (event_tx, mut event_rx) = mpsc::channel::<DaemonEvent>(8);

        let expected = Envelope {
            session_id: 42,
            participant_id: 7,
            msg: WireMessage::SchnorrNonce {
                role: TxRole::Refund(1),
                nonce: "abc123".to_string(),
            },
        };

        let payload = format!("{}\n", serde_json::to_string(&expected).unwrap());

        let client_task = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream.write_all(payload.as_bytes()).await.unwrap();
        });

        let (socket, peer_addr) = listener.accept().await.unwrap();

        let server_task = tokio::spawn(async move {
            handle_connection(socket, peer_addr.to_string(), event_tx)
                .await
                .unwrap();
        });

        let event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("event channel closed");

        match event {
            DaemonEvent::PeerMessage { envelope, from } => {
                assert_eq!(envelope, expected);
                assert!(!from.is_empty());
            }
            _ => panic!("expected PeerMessage event"),
        }

        client_task.await.unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn broadcast_sends_json_line() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let envelope = Envelope {
            session_id: 5,
            participant_id: 2,
            msg: WireMessage::SchnorrNonce {
                role: TxRole::Refund(1),
                nonce: "deadbeef".to_string(),
            },
        };

        let server_task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            line
        });

        let pool = new_connection_pool();
        broadcast(&[addr.to_string()], &envelope, &pool).await.unwrap();

        let line = server_task.await.unwrap();
        let parsed: Envelope = serde_json::from_str(line.trim()).unwrap();

        assert_eq!(parsed, envelope);
    }

    #[tokio::test]
    async fn broadcast_and_handle_connection_work_together() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (event_tx, mut event_rx) = mpsc::channel::<DaemonEvent>(8);

        let server_task = tokio::spawn(async move {
            let (socket, peer_addr) = listener.accept().await.unwrap();
            handle_connection(socket, peer_addr.to_string(), event_tx)
                .await
                .unwrap();
        });

        let expected = Envelope {
            session_id: 123,
            participant_id: 9,

            msg: WireMessage::SchnorrNonce {
                role: TxRole::Refund(1),
                nonce: "hello-world".to_string(),
            },
        };

        let pool = new_connection_pool();
        broadcast(&[addr.to_string()], &expected, &pool).await.unwrap();

        let event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("event channel closed");

        match event {
            DaemonEvent::PeerMessage { envelope, .. } => {
                assert_eq!(envelope, expected);
            }
            _ => panic!("expected PeerMessage event"),
        }

        // The persistent connection stays open (pool keeps it alive), so
        // handle_connection never returns EOF.  The message was verified above;
        // abort the server task rather than waiting indefinitely.
        server_task.abort();
    }
}
