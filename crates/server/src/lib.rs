use std::sync::Arc;

use axum::{Router, extract::State, http::header, response::IntoResponse, routing::get};
use hartig_metrics::Metrics;
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

#[derive(Clone, Debug, Default)]
pub struct ServerState {
    metrics: Arc<Metrics>,
}

impl ServerState {
    #[must_use]
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self { metrics }
    }
}

pub fn router(state: ServerState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/metrics", get(metrics))
        .with_state(state)
}

/// Serves requests until the listener fails or the server is shut down.
///
/// # Errors
///
/// Returns an I/O error when accepting or serving a connection fails.
pub async fn serve(listener: TcpListener, state: ServerState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

/// Serves HTTP/1.1 or HTTP/2 over TLS.
///
/// A failed handshake affects only that connection. Listener failures stop
/// the server and are returned to the caller.
///
/// # Errors
///
/// Returns an I/O error when accepting a TCP connection fails.
pub async fn serve_tls(
    listener: TcpListener,
    state: ServerState,
    acceptor: TlsAcceptor,
) -> std::io::Result<()> {
    let router = router(state);
    loop {
        let (stream, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let service = router.clone();
        tokio::spawn(async move {
            let Ok(stream) = acceptor.accept(stream).await else {
                return;
            };
            let builder = Builder::new(TokioExecutor::new());
            let connection =
                builder.serve_connection(TokioIo::new(stream), TowerToHyperService::new(service));
            let _ = connection.await;
        });
    }
}

async fn health() -> &'static str {
    "ok\n"
}

async fn metrics(State(state): State<ServerState>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        state.metrics.encode(),
    )
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::{ServerState, router};

    #[tokio::test]
    async fn liveness_endpoint_is_available() {
        let response = router(ServerState::default())
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_endpoint_is_available() {
        let response = router(ServerState::default())
            .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
