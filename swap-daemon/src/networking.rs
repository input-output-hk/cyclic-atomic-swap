use std::sync::Arc;
use bytes::Bytes;

use crate::transport::connection_pool::ConnectionPool;
use crate::transport::{Transport, Connector};
use crate::types::{DaemonEvent, Envelope};
use serde_json;
use tokio::{
    sync::{mpsc, Mutex},
};
use tracing::{error, info};


/// Asynchronously handles an incoming connection from a client using a pluggable Transport.
///
/// This function reads data from the provided `Transport`, parses each message into an
/// envelope, and forwards the parsed message as a `DaemonEvent` through the given `mpsc::Sender`.
/// If the connection is closed or an error occurs, the function will exit gracefully.
///
/// # Parameters
///
/// * `transport` - The `Transport` representing the connection with the client.
/// * `from` - A `String` identifying the source of the connection (e.g., an IP address or hostname).
/// * `event_tx` - An `mpsc::Sender<DaemonEvent>` used to send parsed messages to another part of the system for further handling.
///
/// # Returns
///
/// * `Ok(())` - Indicates that the connection was handled successfully or closed without errors.
/// * `Err(Box<dyn std::error::Error + Send + Sync>)` - Returns an error if there was a failure
///   during message handling, such as an I/O error, parsing error, or channel send failure.
///
/// # Errors
///
/// The function returns an error if:
/// * Reading from the `Transport` fails.
/// * Parsing the data into an envelope fails.
/// * Sending a `DaemonEvent` through the channel fails.
///
pub async fn handle_connection<T: Transport>(
    mut transport: T,
    from: String,
    event_tx: mpsc::Sender<DaemonEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        match transport.receive().await {
            Ok(data) => {
                let line = std::str::from_utf8(&data)?;
                let envelope = parse_envelope(line)?;
                event_tx
                    .send(DaemonEvent::PeerMessage {
                        envelope: envelope,
                        from: from.clone(),
                    })
                    .await?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => {
                return Err(Box::new(e));
            }
        }
    }

    info!("Connection closed by {}", from);
    Ok(())
}

/// Parses a JSON string into an `Envelope` object.
///
/// # Parameters
/// - `line`: A `&str` containing the JSON string representation of an `Envelope`.
///
/// # Returns
/// - `Ok(Envelope)` if the provided string is successfully parsed into an `Envelope` object.
/// - `Err(Box<dyn std::error::Error + Send + Sync>)` if parsing fails due to an invalid JSON format or other parsing-related errors.
///
/// # Errors
/// This function will return an error if:
/// - The input string is not a valid JSON.
/// - The JSON structure does not match the expected structure of the `Envelope` type.
///
pub fn parse_envelope(line: &str) -> Result<Envelope, Box<dyn std::error::Error + Send + Sync>> {
    let envelope: Envelope = serde_json::from_str(line)?;
    Ok(envelope)
}

/// Broadcasts a message to multiple addresses asynchronously using a pluggable Transport.
///
/// This function takes a list of target `addresses`, a reference to an `Envelope` object
/// containing the message payload, and a `ConnectionPool`. It serializes the `Envelope`
/// into a payload, and attempts to send it to all provided addresses via the transport.
///
/// # Parameters
///
/// * `addresses` - A slice of `String`s representing the target addresses to which the
///   message should be sent.
/// * `envelope` - A reference to the `Envelope` object that contains the message payload.
/// * `pool` - A reference to the shared `ConnectionPool<T>` that manages active connections.
///
/// # Returns
///
/// A `Result` which is:
/// * `Ok(())` - If the operation is successful and the messages were either delivered or no
///   fatal error occurred.
/// * `Err(Box<dyn std::error::Error>)` - If an error occurred during payload serialization.
///
/// # Errors
///
/// - If the `envelope` cannot be serialized into JSON (`serde_json::to_vec` failure), the function
///   immediately returns an error.
/// - If a connection attempt fails, an error is logged, but the execution continues for other addresses.
/// - If a write operation to a transport fails, the respective connection is removed from the pool, and
///   the error is logged.
pub async fn broadcast<T: Transport, C: Connector<T = T>>(
    addresses: &[String],
    envelope: &Envelope,
    pool: &ConnectionPool<T>,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload = Bytes::from(serde_json::to_vec(envelope)?);

    let mut tasks = Vec::new();

    for addr in addresses {
        let addr = addr.clone();
        let pool = pool.clone();
        let payload = payload.clone();

        tasks.push(tokio::spawn(async move {
            // Check pool without holding lock during connect
            let transport_arc = pool.lock().await.get(&addr).cloned();

            let transport_arc = if let Some(t) = transport_arc {
                t
            } else {
                let connector = C::default();
                match connector.connect(&addr).await {
                    Ok(transport) => {
                        let t = Arc::new(Mutex::new(transport));
                        let mut guard = pool.lock().await;
                        // Use entry so a racing task's connection wins
                        guard.entry(addr.clone()).or_insert(t).clone()
                    }
                    Err(e) => {
                        error!("failed to connect to {}: {}", addr, e);
                        return;
                    }
                }
            };

            let mut transport = transport_arc.lock().await;
            if let Err(e) = transport.send(payload).await {
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
    use crate::transport::tcp::{TcpTransport, TcpConnector};
    use crate::types::{TxRole, WireMessage};
    use tokio::{
        io::{AsyncWriteExt, BufReader, AsyncBufReadExt, AsyncReadExt},
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

        let payload = serde_json::to_vec(&expected).unwrap();

        let client_task = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let len = payload.len() as u32;
            stream.write_u32(len).await.unwrap();
            stream.write_all(&payload).await.unwrap();
        });

        let (socket, peer_addr) = listener.accept().await.unwrap();

        let server_task = tokio::spawn(async move {
            handle_connection(TcpTransport::new(socket), peer_addr.to_string(), event_tx)
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
            let mut reader = socket;
            let len = reader.read_u32().await.unwrap() as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).await.unwrap();
            String::from_utf8(buf).unwrap()
        });

        let pool = new_connection_pool::<TcpTransport>();
        broadcast::<TcpTransport, TcpConnector>(&[addr.to_string()], &envelope, &pool).await.unwrap();

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
            handle_connection(TcpTransport::new(socket), peer_addr.to_string(), event_tx)
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

        let pool = new_connection_pool::<TcpTransport>();
        broadcast::<TcpTransport, TcpConnector>(&[addr.to_string()], &expected, &pool).await.unwrap();

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
