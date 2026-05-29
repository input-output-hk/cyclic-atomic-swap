use bytes::Bytes;
use std::future::Future;
use std::pin::Pin;

pub mod connection_pool;
pub mod tcp;

pub trait Transport: Send + Sync + 'static {
    fn send(&mut self, data: Bytes) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + '_>>;
    fn receive(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<Bytes>> + Send + '_>>;
}

pub trait Connector: Send + Sync + 'static + Default {
    type T: Transport;
    fn connect(&self, addr: &str) -> Pin<Box<dyn Future<Output = std::io::Result<Self::T>> + Send + '_>>;
}

pub trait Listener: Send + Sync + 'static {
    type T: Transport;
    fn accept(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<(Self::T, String)>> + Send + '_>>;
}
