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
    use axum::http::header::{CONTENT_RANGE, RANGE};
    use axum::http::{Request, StatusCode};
    use hartig_pubsub::SEQ_HEADER;
    use hartig_pubsub::memory::MemoryStore;
    use hartig_pubsub::types::MESSAGE_SIZE;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const SIZE: u64 = MESSAGE_SIZE as u64;

    fn post(path: &str) -> Request<Body> {
        Request::post(path)
            .body(Body::from(vec![1; MESSAGE_SIZE]))
            .unwrap()
    }

    fn message(n: u8) -> Vec<u8> {
        vec![n; MESSAGE_SIZE]
    }

    /// Posts `body` to `topic` through the full app, returning the status and position.
    async fn publish(app: &Router, topic: &str, body: Vec<u8>) -> (StatusCode, String) {
        let request = Request::post(format!("{PUBSUB_PREFIX}/{topic}"))
            .body(Body::from(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let seq = response.headers()[SEQ_HEADER].to_str().unwrap().to_owned();
        (response.status(), seq)
    }

    /// Pulls `topic` from the start, returning the `Content-Range` and the body.
    async fn pull_all(app: &Router, topic: &str) -> (String, Vec<u8>) {
        let request = Request::get(format!("{PUBSUB_PREFIX}/{topic}"))
            .header(RANGE, "bytes=0-")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        let content_range = response.headers()[CONTENT_RANGE]
            .to_str()
            .unwrap()
            .to_owned();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (content_range, body.to_vec())
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

    #[tokio::test]
    async fn subscribers_share_one_log_per_topic() {
        let app = app(Arc::new(MemoryStore::new()));
        let abc = "ab".repeat(32);
        let def = "de".repeat(32);

        // Interleaved, so a store that mixed topics would show it.
        assert_eq!(
            publish(&app, &abc, message(1)).await,
            (StatusCode::CREATED, "0".to_owned())
        );
        assert_eq!(
            publish(&app, &def, message(9)).await,
            (StatusCode::CREATED, "0".to_owned())
        );
        assert_eq!(
            publish(&app, &abc, message(2)).await,
            (StatusCode::CREATED, "1".to_owned())
        );
        assert_eq!(
            publish(&app, &def, message(8)).await,
            (StatusCode::CREATED, "1".to_owned())
        );
        assert_eq!(
            publish(&app, &abc, message(3)).await,
            (StatusCode::CREATED, "2".to_owned())
        );

        // Alice, Bob and Charlie all see ABC's messages, in post order.
        let abc_log = [message(1), message(2), message(3)].concat();
        let abc_range = format!("bytes 0-{}/{}", 3 * SIZE - 1, 3 * SIZE);
        for subscriber in ["alice", "bob", "charlie"] {
            let (content_range, body) = pull_all(&app, &abc).await;
            assert_eq!(content_range, abc_range, "{subscriber}");
            assert_eq!(body, abc_log, "{subscriber}");
        }

        // DEF's log holds only its own messages.
        let (content_range, body) = pull_all(&app, &def).await;
        assert_eq!(
            content_range,
            format!("bytes 0-{}/{}", 2 * SIZE - 1, 2 * SIZE)
        );
        assert_eq!(body, [message(9), message(8)].concat());

        // A retried post keeps its original position and adds nothing.
        assert_eq!(
            publish(&app, &abc, message(2)).await,
            (StatusCode::NO_CONTENT, "1".to_owned())
        );
        let (content_range, body) = pull_all(&app, &abc).await;
        assert_eq!(content_range, abc_range);
        assert_eq!(body, abc_log);
    }
}
