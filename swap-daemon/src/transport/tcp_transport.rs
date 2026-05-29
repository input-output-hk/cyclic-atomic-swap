
// 
// receive

// read
// write

// put
// get

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub struct TcpTransport {
    
}

impl TcpTransport {
    pub async fn send(tcpStream: &mut TcpStream, src: &[u8]) -> tokio::io::Result<()> {
         tcpStream.write_all(src).await
    }
}