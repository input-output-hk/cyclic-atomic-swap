use crate::transport::{Transport, Connector, Listener};
use bytes::Bytes;
use std::future::Future;
use std::pin::Pin;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct TcpTransport {
    stream: TcpStream,
}

impl TcpTransport {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, data: Bytes) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + '_>> {
        Box::pin(async move {
            let len = data.len() as u32;
            self.stream.write_u32(len).await?;
            self.stream.write_all(&data).await?;
            self.stream.flush().await?;
            Ok(())
        })
    }

    fn receive(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<Bytes>> + Send + '_>> {
        Box::pin(async move {
            let len = self.stream.read_u32().await? as usize;
            if len > 1024 * 1024 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Message too large",
                ));
            }
            let mut buf = vec![0u8; len];
            self.stream.read_exact(&mut buf).await?;
            Ok(Bytes::from(buf))
        })
    }
}

pub struct TcpListenerAdaptor {
    listener: tokio::net::TcpListener,
}

impl TcpListenerAdaptor {
    pub async fn bind(addr: &str) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        Ok(Self { listener })
    }
}

impl Listener for TcpListenerAdaptor {
    type T = TcpTransport;
    fn accept(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<(Self::T, String)>> + Send + '_>> {
        Box::pin(async move {
            let (stream, addr) = self.listener.accept().await?;
            Ok((TcpTransport::new(stream), addr.to_string()))
        })
    }
}

pub struct TcpConnector;

impl Connector for TcpConnector {
    type T = TcpTransport;
    fn connect(&self, addr: &str) -> Pin<Box<dyn Future<Output = std::io::Result<Self::T>> + Send + '_>> {
        let addr = addr.to_string();
        Box::pin(async move {
            let stream = TcpStream::connect(addr).await?;
            Ok(TcpTransport::new(stream))
        })
    }
}

impl Default for TcpConnector {
    fn default() -> Self {
        Self
    }
}
