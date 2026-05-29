use crate::transport::Transport;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ConnectionManager<T: Transport> {
    pool: DashMap<Arc<str>, Arc<Mutex<Option<T>>>>,
}

impl<T: Transport> ConnectionManager<T> {
    pub fn new() -> Self {
        Self {
            pool: DashMap::new(),
        }
    }

    pub async fn get_connection(&self, addr: Arc<str>) -> Arc<Mutex<Option<T>>> {
        self.pool
            .entry(addr)
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .value()
            .clone()
    }
}
