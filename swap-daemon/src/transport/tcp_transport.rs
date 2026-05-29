use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use super::Transport;

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
}

impl<S> Transport for TcpTransport<S>
where
    S: AsyncWrite + Unpin + Send,
{
    fn send<'a>(&'a mut self, src: &'a [u8]) -> impl Future<Output = tokio::io::Result<()>> + Send + 'a {
        self.stream.write_all(src)
    }
}