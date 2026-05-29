use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

pub type ConnectionPool = Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>>;

pub fn new_connection_pool() -> ConnectionPool {
    Arc::new(Mutex::new(HashMap::new()))
}
