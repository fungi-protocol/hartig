# pubsub-client

A client for hartig's pub-sub service. It posts and pulls fixed-size
 messages on topics, where each topic is named by a secret 16-byte
`TopicId`.

The client does no networking itself. You give it a `Transport` that sends an
HTTP request and returns the response, so you choose the HTTP stack.

## Implementing `Transport`

A transport using [reqwest](https://docs.rs/reqwest) (`reqwest = "0.13"`):

```rust
use bytes::Bytes;
use http::{Request, Response};
use pubsub_client::{Transport, TransportError};

struct Reqwest(reqwest::Client);

impl Transport for Reqwest {
    async fn send(&self, request: Request<Bytes>) -> Result<Response<Bytes>, TransportError> {
        let failed = |what: &str| TransportError::Other(what.to_owned());
        let request = reqwest::Request::try_from(request).map_err(|_| failed("bad request"))?;

        let response = self.0.execute(request).await.map_err(|err| {
            if err.is_connect() {
                TransportError::Connect("could not connect".to_owned())
            } else if err.is_timeout() {
                TransportError::Timeout("request timed out".to_owned())
            } else {
                failed("request failed")
            }
        })?;

        let mut builder = Response::builder().status(response.status());
        for (name, value) in response.headers() {
            builder = builder.header(name, value);
        }
        let body = response.bytes().await.map_err(|_| failed("could not read body"))?;
        builder.body(body).map_err(|_| failed("bad response"))
    }
}
```


## Usage

This uses the `Reqwest` transport from the example above.

```rust
use hartig_pubsub::types::{MESSAGE_SIZE, TopicId};
use pubsub_client::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let topic = TopicId::new([0xab; 16]);
    let transport = Reqwest(reqwest::Client::new());
    let mut client = Client::new(transport, "http://127.0.0.1:3000/pubsub");

    client.join(&topic);

    let seq = client.publish(&topic, vec![7; MESSAGE_SIZE]).await?;
    println!("posted at seq {seq}");

    for message in client.pull(&topic).await? {
        println!("got {} bytes", message.len());
    }

    Ok(())
}
