use std::future::Future;

pub trait Transport: Send {
    fn send<'a>(&'a mut self, src: &'a [u8]) -> impl Future<Output = tokio::io::Result<()>> + Send + 'a;
}
