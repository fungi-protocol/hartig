use std::sync::Arc;

use axum::{Router, extract::State, http::header, response::IntoResponse, routing::get};
use hartig_metrics::Metrics;
use tokio::net::TcpListener;

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
