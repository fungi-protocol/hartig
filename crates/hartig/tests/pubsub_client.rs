#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use hartig_pubsub::memory::MemoryStore;
use hartig_pubsub::types::{MESSAGE_SIZE, TopicId};
use http::header::HOST;
use http::{Request, Response, Uri};
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use pubsub_client::{Client, Transport, TransportError};
use tokio::net::{TcpListener, TcpStream};

/// Sends each request over a fresh HTTP/1 connection.
struct Http;

impl Transport for Http {
    async fn send(&self, request: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
        let failed = |what: &str| TransportError::Other(what.to_owned());
        let (mut parts, body) = request.into_parts();
        let authority = parts
            .uri
            .authority()
            .ok_or_else(|| failed("no authority"))?
            .clone();

        let stream = TcpStream::connect(authority.as_str())
            .await
            .map_err(|_| TransportError::Connect("could not connect".to_owned()))?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| failed("handshake failed"))?;
        tokio::spawn(connection);

        let path = parts.uri.path_and_query().map_or("/", |p| p.as_str());
        parts.uri = Uri::try_from(path).map_err(|_| failed("bad path"))?;
        parts
            .headers
            .insert(HOST, authority.as_str().parse().unwrap());

        let response = sender
            .send_request(Request::from_parts(parts, Full::new(body)))
            .await
            .map_err(|_| failed("request failed"))?;
        let (parts, body) = response.into_parts();
        let body = body
            .collect()
            .await
            .map_err(|_| failed("could not read body"))?
            .to_bytes();
        Ok(Response::from_parts(parts, body))
    }
}

async fn start_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = hartig::app(Arc::new(MemoryStore::new()));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn message(n: u8) -> Vec<u8> {
    vec![n; MESSAGE_SIZE]
}

#[tokio::test]
async fn messages_published_by_one_client_are_pulled_by_another() {
    let addr = start_server().await;
    let base = format!("http://{addr}/pubsub");
    let topic = TopicId::new([7; 16]);

    let mut publisher = Client::new(Http, base.clone());
    let mut reader = Client::new(Http, base);
    publisher.join(&topic);
    reader.join(&topic);

    assert!(reader.pull(&topic).await.unwrap().is_empty());

    assert_eq!(publisher.publish(&topic, message(0)).await.unwrap(), 0);
    assert_eq!(publisher.publish(&topic, message(1)).await.unwrap(), 1);
    assert_eq!(reader.pull(&topic).await.unwrap(), [message(0), message(1)]);

    assert_eq!(publisher.publish(&topic, message(2)).await.unwrap(), 2);
    assert_eq!(reader.pull(&topic).await.unwrap(), [message(2)]);
    assert!(reader.pull(&topic).await.unwrap().is_empty());
}
