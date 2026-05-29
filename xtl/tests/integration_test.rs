use tracing::info;

#[test]
fn suggest_threads_number() {
    let _ = tracing_subscriber::fmt::try_init();
    let cpu_threads = std::thread::available_parallelism();
    info!("CPU Threads: {:?}.", cpu_threads);
    let worker_threads = cpu_threads.map(|n| n.get()).unwrap_or(4).saturating_mul(2);
    info!("Worker Threads: {:?}.", worker_threads);
}
