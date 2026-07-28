use std::{net::SocketAddr, sync::Arc};

use hartig_metrics::Metrics;
use hartig_server::ServerState;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("HARTIG_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
        .parse::<SocketAddr>()?;
    let listener = TcpListener::bind(address).await?;
    let state = ServerState::new(Arc::new(Metrics::default()));

    hartig_server::serve(listener, state).await?;
    Ok(())
}
