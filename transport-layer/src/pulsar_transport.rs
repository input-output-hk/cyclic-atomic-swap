use crate::transport::{Transport, Connector, Listener};
use bytes::Bytes;
use pulsar::{Pulsar, TokioExecutor, Producer, Consumer, SubType};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use futures::StreamExt;

pub struct PulsarTransport {
    producer: Arc<Mutex<Producer<TokioExecutor>>>,
    consumer: Option<Arc<Mutex<Consumer<Vec<u8>, TokioExecutor>>>>,
}

impl PulsarTransport {
    pub fn new(producer: Producer<TokioExecutor>, consumer: Option<Consumer<Vec<u8>, TokioExecutor>>) -> Self {
        Self {
            producer: Arc::new(Mutex::new(producer)),
            consumer: consumer.map(|c| Arc::new(Mutex::new(c))),
        }
    }
}

impl Transport for PulsarTransport {
    fn send(&mut self, data: Bytes) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + '_>> {
        let producer = self.producer.clone();
        Box::pin(async move {
            let mut guard = producer.lock().await;
            guard.send_non_blocking(data.to_vec())
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(())
        })
    }

    fn receive(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<Bytes>> + Send + '_>> {
        let consumer = self.consumer.clone();
        Box::pin(async move {
            if let Some(consumer) = consumer {
                let mut guard = consumer.lock().await;
                if let Some(msg) = guard.next().await {
                    let msg = msg.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                    guard.ack(&msg).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                    Ok(Bytes::from(msg.deserialize()))
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Pulsar stream ended"))
                }
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "No consumer configured for this transport"))
            }
        })
    }
}

pub struct PulsarConnector {
    pulsar_url: String,
}

impl Default for PulsarConnector {
    fn default() -> Self {
        Self {
            pulsar_url: "pulsar://127.0.0.1:6650".to_string(),
        }
    }
}

impl Connector for PulsarConnector {
    type T = PulsarTransport;
    fn connect(&self, addr: &str) -> Pin<Box<dyn Future<Output = std::io::Result<Self::T>> + Send + '_>> {
        let url = self.pulsar_url.clone();
        let topic = addr.to_string();
        Box::pin(async move {
            let pulsar: Pulsar<_> = Pulsar::builder(url, TokioExecutor)
                .build()
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            let producer = pulsar
                .producer()
                .with_topic(topic)
                .build()
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            Ok(PulsarTransport::new(producer, None))
        })
    }
}

pub struct PulsarListenerAdaptor {
    pulsar_url: String,
    _topic: String,
    _subscription: String,
    consumer: Option<Consumer<Vec<u8>, TokioExecutor>>,
}

impl PulsarListenerAdaptor {
    pub async fn bind(url: &str, topic: &str, subscription: &str) -> std::io::Result<Self> {
        let pulsar: Pulsar<_> = Pulsar::builder(url, TokioExecutor)
            .build()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let consumer: Consumer<Vec<u8>, _> = pulsar
            .consumer()
            .with_topic(topic)
            .with_consumer_name(subscription)
            .with_subscription_type(SubType::Shared)
            .with_subscription(subscription)
            .build()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        Ok(Self {
            pulsar_url: url.to_string(),
            _topic: topic.to_string(),
            _subscription: subscription.to_string(),
            consumer: Some(consumer),
        })
    }
}

impl Listener for PulsarListenerAdaptor {
    type T = PulsarTransport;
    fn accept(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<(Self::T, String)>> + Send + '_>> {
        let consumer = self.consumer.take();
        let url = self.pulsar_url.clone();
        Box::pin(async move {
            if let Some(consumer) = consumer {
                // In Pulsar, "accepting" a connection is more like receiving the first message
                // or just returning a transport that can read from the subscription.
                // However, our Daemon expects accept() to yield a new transport per "connection".
                // For a message broker, we might treat the whole subscription as one "connection"
                // or yield a transport that handles a subset of messages.
                
                // To fit the Listener trait, we return the consumer-based transport once.
                // Subsequent calls might need a different strategy or this might be a singleton "listener".
                
                let pulsar: Pulsar<_> = Pulsar::builder(url, TokioExecutor)
                    .build()
                    .await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                // We need a producer to reply to messages (handle_ask, handle_ack, etc.)
                // The "source_addr" in Pulsar is tricky; we'll use the topic or a metadata field.
                let producer = pulsar.producer()
                    .with_topic("default-reply-topic") // Ideally extracted from message metadata
                    .build()
                    .await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                let transport = PulsarTransport::new(producer, Some(consumer));
                Ok((transport, "pulsar-source".to_string()))
            } else {
                // If we already yielded the consumer, we might wait for more or return error
                // In this simplified implementation, we only support one main consumer transport.
                futures::future::pending().await
            }
        })
    }
}
