use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct TcpTransport<S = TcpStream> {
    stream: S
}

impl<S> TcpTransport<S> 
where 
    S: AsyncWrite + Unpin
{
    pub fn new(stream: S) -> Self {
        Self { stream }
    }
    
    pub async fn send(&mut self, src: &[u8]) -> tokio::io::Result<()> {
        self.stream.write_all(src).await
    }
}