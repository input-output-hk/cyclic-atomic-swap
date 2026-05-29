use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Per-daemon persistent connection pool.  Keyed by peer address.
/// Reusing connections avoids exhausting OS ephemeral ports when many
/// messages are sent between the same pairs of daemons.
pub type ConnectionPool<T> = Arc<Mutex<HashMap<String, Arc<Mutex<T>>>>>;

/// Creates a new connection pool.
///
/// This function initializes and returns a `ConnectionPool<T>`, which is an
/// `Arc<Mutex<HashMap<String, Arc<Mutex<T>>>>>`. The connection pool is implemented as a
/// thread-safe hash map, wrapped in an `Arc` for reference counting so it
/// can be shared between threads, and a `Mutex` to ensure synchronized access
/// across threads.
///
/// # Returns
///
/// * `ConnectionPool<T>` - A thread-safe, shareable connection pool.
///
pub fn new_connection_pool<T>() -> ConnectionPool<T> {
    Arc::new(Mutex::new(HashMap::new()))
}
