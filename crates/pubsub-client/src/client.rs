use std::collections::HashMap;
use std::fmt;

use bytes::Bytes;
use hartig_pubsub::types::{MESSAGE_SIZE, Seq, TopicId};
use http::header::RANGE;
use http::{Request, StatusCode};

use crate::transport::{Transport, TransportError};
use crate::url::create_url;

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
    #[error("body holds {0} bytes, not a whole number of {size}-byte messages", size = MESSAGE_SIZE)]
    WrongBodySize(usize),
    #[error("missing or malformed position header")]
    BadSeqHeader,
    #[error("base URL is not a valid URL")]
    InvalidUrl,
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

    /// Fetches new messages from the pubsub server since the last pull.
    pub async fn pull(&mut self, topic: &TopicId) -> Result<Vec<Bytes>, ClientError> {
        let position = *self.positions.get(topic).ok_or(ClientError::NotJoined)?;

        let request = Request::get(create_url(&self.base_url, topic))
            .header(RANGE, format!("{position}-"))
            .body(Bytes::new())
            .map_err(|_| ClientError::InvalidUrl)?;
        let response = self.transport.send(request).await?;

        match response.status() {
            StatusCode::PARTIAL_CONTENT => {}
            StatusCode::RANGE_NOT_SATISFIABLE => return Ok(Vec::new()),
            status => return Err(ClientError::UnexpectedStatus(status)),
        }

        let body = response.into_body();
        if body.len() % MESSAGE_SIZE != 0 {
            return Err(ClientError::WrongBodySize(body.len()));
        }

        let messages: Vec<Bytes> = (0..body.len())
            .step_by(MESSAGE_SIZE)
            .map(|start| body.slice(start..start + MESSAGE_SIZE))
            .collect();
        self.positions
            .insert(*topic, position + messages.len() as Seq);
        Ok(messages)
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
    use axum::Router;
    use axum::body::Body;
    use hartig_pubsub::memory::MemoryStore;
    use http::Response;
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tower::ServiceExt;

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

    /// Sends requests straight into the real router, with no network.
    struct InProcess(Router);

    impl Transport for InProcess {
        async fn send(&self, request: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
            let response = self
                .0
                .clone()
                .oneshot(request.map(Body::from))
                .await
                .unwrap();
            let (parts, body) = response.into_parts();
            let body = body.collect().await.unwrap().to_bytes();
            Ok(Response::from_parts(parts, body))
        }
    }

    /// Ignores the request and always returns this status and body.
    struct Canned {
        status: StatusCode,
        body: Vec<u8>,
    }

    impl Transport for Canned {
        async fn send(&self, _request: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
            let response = Response::builder().status(self.status);
            Ok(response.body(Bytes::from(self.body.clone())).unwrap())
        }
    }

    /// A valid message filled with `n`, so tests can tell messages apart.
    fn message(n: u8) -> Vec<u8> {
        vec![n; MESSAGE_SIZE]
    }

    fn server() -> (Router, Client<InProcess>) {
        let app = hartig_pubsub::router(Arc::new(MemoryStore::new()));
        let client = Client::new(InProcess(app.clone()), "http://host");
        (app, client)
    }

    /// Posts to the router directly, since the client cannot publish yet.
    async fn post(app: &Router, topic: &TopicId, body: Vec<u8>) {
        let request = Request::post(format!("/{}", hex::encode(topic.as_bytes())))
            .body(Body::from(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    fn joined<T: Transport>(transport: T) -> Client<T> {
        let mut client = Client::new(transport, "http://host");
        client.join(&topic(1));
        client
    }

    #[tokio::test]
    async fn pull_requires_join() {
        let (_, mut client) = server();
        assert!(matches!(
            client.pull(&topic(1)).await,
            Err(ClientError::NotJoined)
        ));
    }

    #[tokio::test]
    async fn pull_returns_messages_in_order_then_nothing() {
        let (app, mut client) = server();
        client.join(&topic(1));
        for n in 0..3 {
            post(&app, &topic(1), message(n)).await;
        }

        let pulled = client.pull(&topic(1)).await.unwrap();
        assert_eq!(pulled, [message(0), message(1), message(2)]);
        assert_eq!(client.positions[&topic(1)], 3);
        assert!(client.pull(&topic(1)).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn pull_returns_only_new_messages() {
        let (app, mut client) = server();
        client.join(&topic(1));
        post(&app, &topic(1), message(0)).await;
        client.pull(&topic(1)).await.unwrap();

        post(&app, &topic(1), message(1)).await;
        post(&app, &topic(1), message(2)).await;
        assert_eq!(
            client.pull(&topic(1)).await.unwrap(),
            [message(1), message(2)]
        );
    }

    #[tokio::test]
    async fn pull_reads_only_its_own_topic() {
        let (app, mut client) = server();
        client.join(&topic(1));
        client.join(&topic(2));
        post(&app, &topic(1), message(1)).await;
        post(&app, &topic(2), message(2)).await;

        assert_eq!(client.pull(&topic(1)).await.unwrap(), [message(1)]);
        assert_eq!(client.pull(&topic(2)).await.unwrap(), [message(2)]);
    }

    #[tokio::test]
    async fn pull_rejects_unexpected_status() {
        let mut client = joined(Canned {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: vec![],
        });
        assert!(matches!(
            client.pull(&topic(1)).await,
            Err(ClientError::UnexpectedStatus(
                StatusCode::INTERNAL_SERVER_ERROR
            ))
        ));
        assert_eq!(client.positions[&topic(1)], 0);
    }

    #[tokio::test]
    async fn pull_rejects_partial_message() {
        let mut client = joined(Canned {
            status: StatusCode::PARTIAL_CONTENT,
            body: vec![0; MESSAGE_SIZE + 1],
        });
        assert!(matches!(
            client.pull(&topic(1)).await,
            Err(ClientError::WrongBodySize(n)) if n == MESSAGE_SIZE + 1
        ));
        assert_eq!(client.positions[&topic(1)], 0);
    }
}
