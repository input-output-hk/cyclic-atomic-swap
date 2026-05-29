use crate::connection_manager::ConnectionManager;
use crate::receipt::Receipt;
use crate::transport::Transport;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use crate::swap::Swap;

pub struct DaemonContext<'a, T: Transport> {
    pub at: &'a str,
    pub connection_manager: Arc<ConnectionManager<T>>,
    pub receipt_map: &'a mut HashMap<String, Receipt>,
    pub swap_map: &'a mut HashMap<String, Swap>,
    pub observation_tx: &'a Option<mpsc::Sender<Receipt>>,
}
