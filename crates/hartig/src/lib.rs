//! Hartig: composes its services into one app.

use axum::Router;
use hartig_pubsub::store::TopicStore;
use std::sync::Arc;

/// Where the pub-sub service is mounted.
pub const PUBSUB_PREFIX: &str = "/pubsub";

/// Hartig's full HTTP app: every service mounted at its prefix.
pub fn app(pubsub_store: Arc<dyn TopicStore>) -> Router {
    Router::new().nest(PUBSUB_PREFIX, hartig_pubsub::router(pubsub_store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use hartig_pubsub::memory::MemoryStore;
    use hartig_pubsub::types::MESSAGE_SIZE;
    use tower::ServiceExt;

    fn post(path: &str) -> Request<Body> {
        Request::post(path)
            .body(Body::from(vec![1; MESSAGE_SIZE]))
            .unwrap()
    }

    #[tokio::test]
    async fn pubsub_is_mounted_under_its_prefix() {
        let app = app(Arc::new(MemoryStore::new()));
        let topic = "ab".repeat(32);

        let mounted = app
            .clone()
            .oneshot(post(&format!("{PUBSUB_PREFIX}/{topic}")))
            .await
            .unwrap();
        assert_eq!(mounted.status(), StatusCode::CREATED);

        let unprefixed = app.oneshot(post(&format!("/{topic}"))).await.unwrap();
        assert_eq!(unprefixed.status(), StatusCode::NOT_FOUND);
    }
}
