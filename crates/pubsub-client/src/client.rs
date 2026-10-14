use std::collections::HashMap;
use std::fmt;

use hartig_pubsub::types::{MESSAGE_SIZE, Seq, TopicId};
use http::StatusCode;

use crate::transport::{Transport, TransportError};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("transport failed: {0}")]
    Transport(#[from] TransportError),
    #[error("topic is not joined")]
    NotJoined,
    #[error("message must be {size} bytes, got {0}", size = MESSAGE_SIZE)]
    WrongMessageSize(usize),
    #[error("unexpected status {0}")]
    UnexpectedStatus(StatusCode),
    #[error("missing or malformed Content-Range")]
    BadContentRange,
    #[error("Content-Range starts at seq {got}, expected {expected}")]
    ContentRangeMismatch { expected: Seq, got: Seq },
    #[error("missing or malformed position header")]
    BadSeqHeader,
}

/// Posts and pulls fixed-size messages for the topics it has joined.
pub struct Client<T: Transport> {
    transport: T,
    base_url: String,
    positions: HashMap<TopicId, Seq>,
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T, base_url: impl Into<String>) -> Self {
        Self {
            transport,
            base_url: base_url.into(),
            positions: HashMap::new(),
        }
    }

    /// Starts reading `topic` from the beginning. Joining again keeps the current position.
    pub fn join(&mut self, topic: &TopicId) {
        self.positions.entry(*topic).or_insert(0);
    }

    pub fn leave(&mut self, topic: &TopicId) {
        self.positions.remove(topic);
    }

    pub fn is_joined(&self, topic: &TopicId) -> bool {
        self.positions.contains_key(topic)
    }
}

/// Shows only how many topics are joined
impl<T: Transport> fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("topics", &self.positions.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{Request, Response};

    struct NoTransport;

    impl Transport for NoTransport {
        async fn send(&self, _request: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
            unreachable!("no request should be sent")
        }
    }

    fn client() -> Client<NoTransport> {
        Client::new(NoTransport, "http://host/pubsub")
    }

    fn topic(n: u8) -> TopicId {
        TopicId::new([n; 16])
    }

    #[test]
    fn joining_twice_keeps_position() {
        let mut client = client();
        client.join(&topic(1));
        client.positions.insert(topic(1), 5);
        client.join(&topic(1));

        assert_eq!(client.positions[&topic(1)], 5);
    }

    #[test]
    fn rejoining_after_leave_starts_over() {
        let mut client = client();
        client.join(&topic(1));
        client.positions.insert(topic(1), 5);
        client.leave(&topic(1));
        client.join(&topic(1));

        assert_eq!(client.positions[&topic(1)], 0);
    }

    #[test]
    fn debug_hides_topic_ids() {
        let mut client = client();
        let secret = TopicId::new([0xab; 16]);
        client.join(&secret);

        let debug = format!("{client:?}");
        assert!(debug.contains("topics: 1"));
        assert!(!debug.contains(&hex::encode(secret.as_bytes())));
    }
}
