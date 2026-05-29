use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

/// Per-daemon persistent connection pool.  Keyed by peer TCP address.
/// Reusing connections avoids exhausting OS ephemeral ports when many
/// messages are sent between the same pairs of daemons.
pub type ConnectionPool = Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>>;

/// Creates a new connection pool.
///
/// This function initializes and returns a `ConnectionPool`, which is an
/// `Arc<Mutex<HashMap<K, V>>>`. The connection pool is implemented as a
/// thread-safe hash map, wrapped in an `Arc` for reference counting so it
/// can be shared between threads, and a `Mutex` to ensure synchronized access
/// across threads.
///
/// # Returns
///
/// * `ConnectionPool` - A thread-safe, shareable connection pool.
///
pub fn new_connection_pool() -> ConnectionPool {
    Arc::new(Mutex::new(HashMap::new()))
}
