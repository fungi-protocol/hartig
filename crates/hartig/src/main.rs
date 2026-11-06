const DEFAULT_ADDR: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::var("HARTIG_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_owned());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("hartig listening on {}", listener.local_addr()?);

    axum::serve(listener, hartig::app()).await
}
