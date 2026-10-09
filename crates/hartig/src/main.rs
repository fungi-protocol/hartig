use hartig_pubsub::memory::MemoryStore;
use std::sync::Arc;

/// Used when `HARTIG_ADDR` isn't set.
const DEFAULT_ADDR: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::var("HARTIG_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_owned());
    let store = Arc::new(MemoryStore::new());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("hartig listening on {}", listener.local_addr()?);

    axum::serve(listener, hartig::app(store)).await
}
