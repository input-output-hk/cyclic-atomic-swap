use crate::connection_manager::ConnectionManager;
use crate::daemon_context::DaemonContext;
use crate::envelope::Envelope;
use crate::message::Message;
use crate::party_confidential::PartyConfidential;
use crate::receipt::Receipt;
use crate::swap::Swap;
use crate::swap_description::SwapDescription;
use crate::transport::{Connector, Listener, Transport};
use bytes::Bytes;
use futures::StreamExt;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

#[derive(Debug, Clone)]
pub struct Daemon<T: Transport, C: Connector<T = T>> {
    _transport: PhantomData<T>,
    _connector: PhantomData<C>,
}

pub struct DaemonArgs<T: Transport, LISTENER: Listener<T = T>> {
    pub party: PartyConfidential,
    pub connection_manager: Arc<ConnectionManager<T>>,
    pub messages_buffer_size: usize,
    pub liveness_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub observer_tx: Option<mpsc::Sender<Receipt>>,
    pub shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    pub listener: LISTENER,
}

impl<T: Transport, C: Connector<T = T>> Daemon<T, C> {
    pub async fn ask(
        connection_manager: Arc<ConnectionManager<T>>,
        message_buffer_size: usize,
        id: String,
        from: String,
        to: Vec<String>,
        content: Message,
    ) {
        let to_vec = to.clone();
        tokio::spawn(async move {
            futures::stream::iter(to)
                .for_each_concurrent(message_buffer_size, |to| {
                    let connection_manager = connection_manager.clone();
                    let id = id.clone();
                    let from = from.clone();
                    let to_vec = to_vec.clone();
                    let content = content.clone();
                    async move {
                        let ask = Envelope::Ask {
                            id,
                            from: from.clone(),
                            to: to_vec.clone(),
                            message: content,
                        };
                        let raw_message = Bytes::from(ask.to_json().unwrap());
                        Self::handle_network_transmission(
                            connection_manager,
                            &from,
                            &to,
                            raw_message,
                        )
                        .await;
                    }
                })
                .await;
        });
    }

    fn _acknowledge(
        at: &str,
        receipt_map: &mut HashMap<String, Receipt>,
        observation_tx: &Option<mpsc::Sender<Receipt>>,
        id: String,
        from: String,
        size: usize,
    ) {
        let receipt = receipt_map
            .entry(id.clone())
            .or_insert_with(|| Receipt::new(id.clone(), size));

        receipt.add_ack(from);

        if receipt.is_complete() {
            info!(at = %at, ulid = %id, "Ack {}/{}", receipt.ack().len(), receipt.size());
            if let Some(tx) = observation_tx {
                let _ = tx.try_send(receipt.clone());
            }
            receipt_map.remove(&id);
        }
    }

    fn _not_acknowledge(
        at: &str,
        receipt_map: &mut HashMap<String, Receipt>,
        observation_tx: &Option<mpsc::Sender<Receipt>>,
        id: String,
        from: String,
        size: usize,
    ) {
        let receipt = receipt_map
            .entry(id.clone())
            .or_insert_with(|| Receipt::new(id.clone(), size));

        receipt.add_nak(from);

        if receipt.is_complete() {
            info!(at = %at, ulid = %id, "Nak {}/{}", receipt.ack().len(), receipt.size());
            if let Some(tx) = observation_tx {
                let _ = tx.try_send(receipt.clone());
            }
            receipt_map.remove(&id);
        }
    }

    async fn handle_ack(
        context: DaemonContext<'_, T>,
        id: String,
        from: String,
        _to: String,
        size: usize,
    ) {
        debug!(at = %context.at, from = %from, ulid=%id, "Ack.");
        Self::_acknowledge(
            context.at,
            context.receipt_map,
            context.observation_tx,
            id,
            from,
            size,
        );
    }

    async fn handle_ask(
        context: DaemonContext<'_, T>,
        id: String,
        from: String,
        to: Vec<String>,
        message: Message,
    ) {
        debug!(at = %context.at, from = %from, ulid=%id, "Ask.");
        let context = Self::handle_message(context, id.clone(), message);
        let size = to.len();

        if from == context.at {
            Self::_acknowledge(
                context.at,
                context.receipt_map,
                context.observation_tx,
                id,
                from,
                size,
            );
        } else {
            let to = from.clone();
            let from = context.at.to_string();
            let ack = Envelope::Ack {
                id,
                from: from.clone(),
                to: to.clone(),
                size,
            };
            let raw_message = Bytes::from(ack.to_json().unwrap());
            let connection_manager = context.connection_manager.clone();
            tokio::spawn(async move {
                Self::handle_network_transmission(connection_manager, &from, &to, raw_message)
                    .await;
            });
        }
    }

    async fn handle_nak(
        context: DaemonContext<'_, T>,
        id: String,
        from: String,
        _to: String,
        size: usize,
    ) {
        debug!(at = %context.at, from = %from, ulid=%id, "Nak.");
        Self::_not_acknowledge(
            context.at,
            context.receipt_map,
            context.observation_tx,
            id,
            from,
            size,
        );
    }

    async fn handle_envelope(context: DaemonContext<'_, T>, envelope: Envelope) -> bool {
        match envelope {
            Envelope::Ask {
                id,
                from,
                to,
                message,
            } => {
                Self::handle_ask(context, id, from, to, message).await;
            }
            Envelope::Ack { id, from, to, size } => {
                Self::handle_ack(context, id, from, to, size).await;
            }
            Envelope::Nak { id, from, to, size } => {
                Self::handle_nak(context, id, from, to, size).await;
            }
        }
        true
    }

    fn handle_message(
        context: DaemonContext<'_, T>,
        id: String,
        message: Message,
    ) -> DaemonContext<'_, T> {
        match message {
            Message::SwapDescription(swap_config) => Self::handle_swap_config(context, swap_config),
            Message::Text(text) => {
                debug!(at = %context.at, id=%id, text = %text, "Message.");
                context
            }
        }
    }

    async fn handle_network_reception(
        at: &str,
        tx: mpsc::Sender<Envelope>,
        mut transport: T,
        source_addr: String,
    ) {
        trace!(at = &at, from = %source_addr, "Connected.");

        loop {
            match transport.receive().await {
                Ok(data) => {
                    let message = String::from_utf8_lossy(&data);
                    match Envelope::from_json(&message) {
                        Ok(envelope) => {
                            if let Err(err) = tx.send(envelope).await {
                                error!(at = &at, from = %source_addr, message = %message, err = %err, "Read: dispatch job error.");
                                break;
                            } else {
                                trace!(at = &at, from = %source_addr, message = %message, "Read.");
                            }
                        }
                        Err(err) => {
                            error!(at = &at, from = %source_addr, message = %message, err = %err, "Read: parse error.");
                            break;
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(err) => {
                    error!(at = &at, from = %source_addr, err = %err, "Read error.");
                    break;
                }
            }
        }
    }

    async fn handle_network_transmission(
        manager: Arc<ConnectionManager<T>>,
        from: &str,
        to: &str,
        message: Bytes,
    ) {
        let mutex = manager.get_connection(Arc::from(to)).await;
        let mut guard = mutex.lock().await;

        for _ in 0..2 {
            if let Some(ref mut transport) = *guard {
                match transport.send(message.clone()).await {
                    Ok(_) => {
                        trace!(from = %from, to = %to, "Transmission: sent.");
                        return;
                    }
                    Err(_) => {
                        *guard = None;
                    }
                }
            }

            match C::default().connect(to.as_ref()).await {
                Ok(mut transport) => {
                    trace!(from = %from, to = %to, "Transmission: connected.");
                    match transport.send(message.clone()).await {
                        Ok(_) => {
                            trace!(from = %from, to = %to, "Transmission: sent.");
                            *guard = Some(transport);
                            return;
                        }
                        Err(err) => {
                            error!(from = %from, to = %to, err = %err, "Transmission: send error.");
                            return;
                        }
                    }
                }
                Err(err) => {
                    error!(from = %from, to = %to, err = %err, "Transmission error.");
                    return;
                }
            }
        }
    }

    fn handle_swap_config(
        context: DaemonContext<'_, T>,
        swap_config: SwapDescription,
    ) -> DaemonContext<'_, T> {
        if context.swap_map.contains_key(swap_config.id()) {
            warn!(at = %context.at, id = %swap_config.id(), "SwapConfig ignored: id already exists.");
        } else {
            context.swap_map.insert(
                swap_config.id().to_string(),
                Swap::new(swap_config.id().to_string(), swap_config.parties().to_vec()),
            );
            info!(at = %context.at, id = %swap_config.id(), "SwapConfig.");
        }
        context
    }

    pub async fn run<LISTENER: Listener<T = T>>(
        args: DaemonArgs<T, LISTENER>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut shutdown_rx = args.shutdown_rx;
        let mut listener = args.listener;
        let at = args.party.address.as_str();
        let connection_manager = args.connection_manager;
        let messages_buffer_size = args.messages_buffer_size;
        let liveness_tx = args.liveness_tx;
        let observer_tx = args.observer_tx;

        info!(at = %at, "Start.");
        let mut receipt_map: HashMap<String, Receipt> = HashMap::new();
        let mut swap_map: HashMap<String, Swap> = HashMap::new();
        let (job_tx, mut job_rx) = mpsc::channel::<Envelope>(messages_buffer_size);
        if let Some(liveness) = liveness_tx {
            let _ = liveness.send(());
        }
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
                connection = listener.accept() => {
                    match connection {
                        Ok((transport, source_addr)) => {
                            let tx = job_tx.clone();
                            let at = at.to_string();
                            tokio::spawn(async move {
                                Self::handle_network_reception(&at, tx, transport, source_addr).await;
                            });
                        }
                        Err(err) => {
                            error!(at = %at, err = %err, "Connection error.");
                        }
                    }
                }
                job = job_rx.recv() => {
                    if let Some(envelope) = job {
                        let context = DaemonContext {
                                at: &at,
                                connection_manager: connection_manager.clone(),
                                receipt_map: &mut receipt_map,
                                swap_map: &mut swap_map,
                                observation_tx: &observer_tx,
                            };
                        if !Self::handle_envelope(context, envelope).await {
                                break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        info!(at = %at, "Stop.");
        Ok(())
    }
}
