use bytes::Bytes;
use http::{Request, Response};
use std::future::Future;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("request timed out: {0}")]
    Timeout(String),
    #[error("transport error: {0}")]
    Other(String),
}

pub trait Transport: Send + Sync {
    /// Sends a request and returns the server's response.
    /// `Err` means the request didn't complete.
    fn send(
        &self,
        request: Request<Bytes>,
    ) -> impl Future<Output = Result<Response<Bytes>, TransportError>> + Send;
}
