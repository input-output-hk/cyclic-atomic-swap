use crate::transport::{Transport, Connector, Listener};
use bytes::Bytes;
use std::future::Future;
use std::pin::Pin;
use tokio::net::UdpSocket;
use std::sync::Arc;

pub struct UdpTransport {
    socket: Arc<UdpSocket>,
    target_addr: Option<String>,
    initial_data: Option<Bytes>,
}

impl UdpTransport {
    pub fn new(socket: UdpSocket) -> Self {
        Self { socket: Arc::new(socket), target_addr: None, initial_data: None }
    }

    pub fn with_target(socket: Arc<UdpSocket>, addr: String) -> Self {
        Self { socket, target_addr: Some(addr), initial_data: None }
    }

    pub fn with_target_and_data(socket: Arc<UdpSocket>, addr: String, data: Bytes) -> Self {
        Self { socket, target_addr: Some(addr), initial_data: Some(data) }
    }
}

impl Transport for UdpTransport {
    fn send(&mut self, data: Bytes) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + '_>> {
        Box::pin(async move {
            if let Some(target) = &self.target_addr {
                self.socket.send_to(&data, target).await?;
            } else {
                self.socket.send(&data).await?;
            }
            Ok(())
        })
    }

    fn receive(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<Bytes>> + Send + '_>> {
        Box::pin(async move {
            if let Some(data) = self.initial_data.take() {
                return Ok(data);
            }
            let mut buf = vec![0u8; 65507]; // Max UDP payload size
            let len = self.socket.recv(&mut buf).await?;
            buf.truncate(len);
            Ok(Bytes::from(buf))
        })
    }
}

pub struct UdpListenerAdaptor {
    socket: Arc<UdpSocket>,
}

impl UdpListenerAdaptor {
    pub async fn bind(addr: &str) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        Ok(Self { socket: Arc::new(socket) })
    }
}

impl Listener for UdpListenerAdaptor {
    type T = UdpTransport;
    fn accept(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<(Self::T, String)>> + Send + '_>> {
        Box::pin(async move {
            let mut buf = vec![0u8; 65507];
            let (len, addr) = self.socket.recv_from(&mut buf).await?;
            buf.truncate(len);
            
            let addr_str = addr.to_string();
            let data = Bytes::from(buf);
            let transport = UdpTransport::with_target_and_data(self.socket.clone(), addr_str.clone(), data);
            
            Ok((transport, addr_str))
        })
    }
}

pub struct UdpConnector;

impl Connector for UdpConnector {
    type T = UdpTransport;
    fn connect(&self, addr: &str) -> Pin<Box<dyn Future<Output = std::io::Result<Self::T>> + Send + '_>> {
        let addr = addr.to_string();
        Box::pin(async move {
            // Bind to an ephemeral port
            let socket = UdpSocket::bind("0.0.0.0:0").await?;
            socket.connect(addr).await?;
            Ok(UdpTransport::new(socket))
        })
    }
}

impl Default for UdpConnector {
    fn default() -> Self {
        Self
    }
}
