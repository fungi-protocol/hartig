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

    match (
        std::env::var_os("HARTIG_TLS_CERT"),
        std::env::var_os("HARTIG_TLS_KEY"),
    ) {
        (Some(certificate), Some(private_key)) => {
            let config = hartig_tls::server_config(certificate, private_key)?;
            hartig_server::serve_tls(listener, state, hartig_tls::acceptor(config)).await?;
        }
        (None, None) => hartig_server::serve(listener, state).await?,
        _ => return Err("HARTIG_TLS_CERT and HARTIG_TLS_KEY must be set together".into()),
    }
    Ok(())
}
