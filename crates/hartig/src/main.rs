use std::net::SocketAddr;

const ADDR_VAR: &str = "HARTIG_ADDR";

fn listen_addr() -> Result<SocketAddr, String> {
    let raw = std::env::var(ADDR_VAR).map_err(|err| format!("{ADDR_VAR}: {err}"))?;
    raw.parse()
        .map_err(|err| format!("{ADDR_VAR}={raw:?} is not a socket address: {err}"))
}

#[tokio::main]
async fn main() {
    let addr = match listen_addr() {
        Ok(addr) => addr,
        Err(msg) => {
            eprintln!("hartig: {msg}");
            std::process::exit(1);
        }
    };
    if let Err(err) = serve(addr).await {
        eprintln!("hartig: {err}");
        std::process::exit(1);
    }
}

async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("hartig listening on {}", listener.local_addr()?);
    axum::serve(listener, hartig::app()).await
}
