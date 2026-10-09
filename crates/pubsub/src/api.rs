use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode,
        header::{CONTENT_RANGE, CONTENT_TYPE, RANGE},
    },
    response::{IntoResponse, Response},
    routing::post,
};
use std::sync::Arc;

use crate::store::{AppendResult, StoreError, TopicStore};
use crate::types::{MESSAGE_SIZE, ParseTopicIdError, Seq, TopicId};

/// Response header carrying a posted message's position, on both 201 and 204.
pub const SEQ_HEADER: HeaderName = HeaderName::from_static("hartig-seq");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RangeRequest {
    /// End is inclusive, bytes 0-99 for example implies 100 bytes.
    start: u64,
    end: Option<u64>,
}

/// Why a `Range` header was rejected.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum RangeError {
    #[error("range must start with `bytes=`")]
    NoByteRange,
    #[error("only a single range is supported")]
    MultipleRanges,
    #[error("range must have a start; suffix ranges are not supported")]
    NoStartRange,
    #[error("range bounds must be plain decimal numbers")]
    InvalidNumber,
    #[error("range end must not be before its start")]
    EndBeforeStart,
    #[error("range must start on a message boundary")]
    Misaligned,
    #[error("range must cover at least one whole message")]
    TooSmall,
}

/// Everything a pub-sub request can fail with. None of these carry a topic ID.
#[derive(Debug, thiserror::Error)]
enum ApiError {
    #[error(transparent)]
    InvalidTopic(#[from] ParseTopicIdError),
    #[error("message must be exactly {MESSAGE_SIZE} bytes, got {0}")]
    WrongSize(usize),
    #[error("a `Range` header is required")]
    MissingRange,
    #[error(transparent)]
    InvalidRange(#[from] RangeError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::InvalidTopic(_) => StatusCode::BAD_REQUEST,
            Self::WrongSize(_) => StatusCode::BAD_REQUEST,
            Self::MissingRange => StatusCode::BAD_REQUEST,
            Self::InvalidRange(_) => StatusCode::BAD_REQUEST,
            Self::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = match self {
            // Backend details are for server logs, not clients.
            Self::Store(_) => "internal error".to_owned(),
            other => other.to_string(),
        };

        (status, body).into_response()
    }
}

/// What every pub-sub handler returns.
type ApiResult<T> = Result<T, ApiError>;

/// The store shared by every request
type SharedStore = Arc<dyn TopicStore>;

/// The pub-sub service's routes, with the store already attached.
pub fn router(store: SharedStore) -> Router {
    Router::new()
        .route("/{topic}", post(post_message).get(get_messages))
        // Bodies over MESSAGE_SIZE get 413 
        // so the handler's own size check only ever sees bodies that are too short.
        .layer(DefaultBodyLimit::max(MESSAGE_SIZE))
        .with_state(store)
}

/// `POST /{topic}`: append one fixed-size message. 201 if new, 204 if a duplicate.
async fn post_message(
    State(store): State<SharedStore>,
    Path(topic): Path<String>,
    body: Bytes,
) -> ApiResult<Response> {
    let topic: TopicId = topic.parse()?;

    if body.len() != MESSAGE_SIZE {
        return Err(ApiError::WrongSize(body.len()));
    }

    // Blocking call: fine for the in-memory store just for the in-memory store.
    let result = store.append(&topic, Vec::from(body))?;

    let status = match result {
        AppendResult::Inserted { .. } => StatusCode::CREATED,
        AppendResult::Duplicate { .. } => StatusCode::NO_CONTENT,
    };
    let seq = HeaderValue::from(result.seq());

    Ok((status, [(SEQ_HEADER, seq)]).into_response())
}

/// `GET /{topic}` with `Range: bytes=START-[END]`: pull messages from a byte offset.
/// 206 with the messages, or 416 with the topic's length when there's nothing new.
async fn get_messages(
    State(store): State<SharedStore>,
    Path(topic): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let topic: TopicId = topic.parse()?;

    let header = headers.get(RANGE).ok_or(ApiError::MissingRange)?;
    // A header that isn't visible ASCII can't be `bytes=...`.
    let header = header.to_str().map_err(|_| RangeError::NoByteRange)?;
    let range = parse_range(header)?;
    let messages = to_message_range(range)?;

    let result = store.read(&topic, messages.start, messages.max)?;
    let total = result.total_count.saturating_mul(MESSAGE_SIZE as u64);

    // Caught up, past the end, or an unknown topic (total 0).
    if result.messages.is_empty() {
        let content_range = format!("bytes */{total}");
        return Ok((
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(CONTENT_RANGE, content_range)],
        )
            .into_response());
    }

    let body = result.messages.concat();
    let first = range.start;
    let last = first + body.len() as u64 - 1;
    let content_range = format!("bytes {first}-{last}/{total}");

    Ok((
        StatusCode::PARTIAL_CONTENT,
        [
            (CONTENT_TYPE, "application/octet-stream".to_owned()),
            (CONTENT_RANGE, content_range),
        ],
        body,
    )
        .into_response())
}

/// A byte range turned into what `TopicStore::read` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MessageRange {
    start: Seq,
    /// `None` reads to the end of the topic.
    max: Option<usize>,
}

/// Parse a `Range` header value of the form `bytes=START-` or `bytes=START-END`.
fn parse_range(header: &str) -> Result<RangeRequest, RangeError> {
    // Only lowercase `bytes=`: client should  always send exactly that.
    let spec = header
        .strip_prefix("bytes=")
        .ok_or(RangeError::NoByteRange)?;

    // Checked before splitting, so `0-1,5-6` gets this error rather than a number error.
    if spec.contains(',') {
        return Err(RangeError::MultipleRanges);
    }

    let (start, end) = spec.split_once('-').ok_or(RangeError::InvalidNumber)?;

    // `bytes=-500` means "the last 500 bytes" in HTTP; Hartig doesn't support it.
    //Todo
    if start.is_empty() {
        return Err(RangeError::NoStartRange);
    }
    let start = parse_number(start)?;

    let end = if end.is_empty() {
        None
    } else {
        Some(parse_number(end)?)
    };

    if end.is_some_and(|end| end < start) {
        return Err(RangeError::EndBeforeStart);
    }

    Ok(RangeRequest { start, end })
}

/// Parse a plain decimal number. `u64::from_str` alone would also accept a leading `+`.
fn parse_number(s: &str) -> Result<u64, RangeError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(RangeError::InvalidNumber);
    }
    s.parse().map_err(|_| RangeError::InvalidNumber)
}

/// Convert a byte range into message positions. The start must fall on a message
/// boundary; a bounded end is rounded down to whole messages.
fn to_message_range(range: RangeRequest) -> Result<MessageRange, RangeError> {
    let size = MESSAGE_SIZE as u64;
    if !range.start.is_multiple_of(size) {
        return Err(RangeError::Misaligned);
    }

    let max = match range.end {
        None => None,
        Some(end) => {
            // u128 so `bytes=0-18446744073709551615` can't overflow the `+ 1`.
            let len = u128::from(end - range.start) + 1;
            let count = len / u128::from(size);
            if count == 0 {
                return Err(RangeError::TooSmall);
            }
            // The store caps `max` at the topic's length anyway.
            Some(usize::try_from(count).unwrap_or(usize::MAX))
        }
    };

    Ok(MessageRange {
        start: range.start / size,
        max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryStore;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[test]
    fn api_errors_map_to_statuses() {
        let cases = [
            (
                ApiError::InvalidTopic(ParseTopicIdError::InvalidHex),
                StatusCode::BAD_REQUEST,
            ),
            (
                ApiError::WrongSize(MESSAGE_SIZE - 1),
                StatusCode::BAD_REQUEST,
            ),
            (ApiError::WrongSize(0), StatusCode::BAD_REQUEST),
            (
                ApiError::WrongSize(MESSAGE_SIZE + 1),
                StatusCode::BAD_REQUEST,
            ),
            (ApiError::MissingRange, StatusCode::BAD_REQUEST),
            (
                ApiError::InvalidRange(RangeError::Misaligned),
                StatusCode::BAD_REQUEST,
            ),
            (
                ApiError::Store(StoreError::Backend("disk".to_owned())),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (error, status) in cases {
            let label = format!("{error:?}");
            assert_eq!(error.into_response().status(), status, "{label}");
        }
    }

    #[tokio::test]
    async fn store_error_body_hides_backend_details() {
        let response =
            ApiError::Store(StoreError::Backend("lock poisoned".to_owned())).into_response();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"internal error");
    }

    fn range(start: u64, end: Option<u64>) -> Result<RangeRequest, RangeError> {
        Ok(RangeRequest { start, end })
    }

    #[test]
    fn accepts_open_and_bounded_ranges() {
        assert_eq!(parse_range("bytes=0-"), range(0, None));
        assert_eq!(parse_range("bytes=200-"), range(200, None));
        assert_eq!(parse_range("bytes=200-599"), range(200, Some(599)));
        assert_eq!(parse_range("bytes=5-5"), range(5, Some(5)));
    }

    #[test]
    fn rejects_missing_bytes_prefix() {
        for header in ["", "200-", "items=0-", "BYTES=0-"] {
            assert_eq!(
                parse_range(header),
                Err(RangeError::NoByteRange),
                "{header}"
            );
        }
    }

    #[test]
    fn rejects_multiple_ranges() {
        assert_eq!(
            parse_range("bytes=0-1,5-6"),
            Err(RangeError::MultipleRanges)
        );
    }

    #[test]
    fn rejects_suffix_range() {
        assert_eq!(parse_range("bytes=-500"), Err(RangeError::NoStartRange));
    }

    #[test]
    fn rejects_invalid_numbers() {
        let too_big = format!("bytes={}-", "9".repeat(20));
        for header in [
            "bytes=",
            "bytes=200",
            "bytes=abc-",
            "bytes=+5-",
            "bytes=0-+9",
            "bytes= 0-",
            "bytes=1-2-3",
            too_big.as_str(),
        ] {
            assert_eq!(
                parse_range(header),
                Err(RangeError::InvalidNumber),
                "{header}"
            );
        }
    }

    #[test]
    fn rejects_end_before_start() {
        assert_eq!(parse_range("bytes=10-5"), Err(RangeError::EndBeforeStart));
    }

    fn messages(start: u64, end: Option<u64>) -> Result<MessageRange, RangeError> {
        to_message_range(RangeRequest { start, end })
    }

    fn message_range(start: Seq, max: Option<usize>) -> Result<MessageRange, RangeError> {
        Ok(MessageRange { start, max })
    }

    #[test]
    fn open_range_reads_to_the_end() {
        assert_eq!(messages(0, None), message_range(0, None));
        assert_eq!(messages(2 * SIZE, None), message_range(2, None));
    }

    #[test]
    fn bounded_range_counts_whole_messages() {
        assert_eq!(
            messages(2 * SIZE, Some(6 * SIZE - 1)),
            message_range(2, Some(4))
        );
        assert_eq!(messages(0, Some(SIZE - 1)), message_range(0, Some(1)));
    }

    #[test]
    fn bounded_range_rounds_down() {
        // Two and a half messages' worth of bytes counts as two.
        assert_eq!(
            messages(2 * SIZE, Some(4 * SIZE + SIZE / 2)),
            message_range(2, Some(2))
        );
        // One byte short of two messages counts as one.
        assert_eq!(messages(0, Some(2 * SIZE - 2)), message_range(0, Some(1)));
    }

    #[test]
    fn rejects_misaligned_start() {
        for start in [1, SIZE - 1, SIZE + SIZE / 2, 2 * SIZE + 1] {
            assert_eq!(
                messages(start, None),
                Err(RangeError::Misaligned),
                "{start}"
            );
        }
    }

    #[test]
    fn rejects_range_smaller_than_one_message() {
        assert_eq!(messages(0, Some(0)), Err(RangeError::TooSmall));
        assert_eq!(
            messages(2 * SIZE, Some(3 * SIZE - 2)),
            Err(RangeError::TooSmall)
        );
    }

    #[test]
    fn huge_bounded_range_does_not_overflow() {
        let result = messages(0, Some(u64::MAX)).unwrap();
        assert_eq!(result.start, 0);
        assert!(result.max.is_some_and(|max| max > 0));
    }

    fn post_request(topic: &str, body: Vec<u8>) -> Request<Body> {
        Request::post(format!("/{topic}"))
            .body(Body::from(body))
            .unwrap()
    }

    #[tokio::test]
    async fn post_new_then_duplicate() {
        let app = router(Arc::new(MemoryStore::new()));
        let topic = "ab".repeat(32);
        let message = vec![1; MESSAGE_SIZE];

        let first = app
            .clone()
            .oneshot(post_request(&topic, message.clone()))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);
        assert_eq!(first.headers()[SEQ_HEADER], "0");

        let retry = app.oneshot(post_request(&topic, message)).await.unwrap();
        assert_eq!(retry.status(), StatusCode::NO_CONTENT);
        assert_eq!(retry.headers()[SEQ_HEADER], "0");
    }

    const SIZE: u64 = MESSAGE_SIZE as u64;

    fn new_app() -> Router {
        router(Arc::new(MemoryStore::new()))
    }

    fn topic(n: u8) -> String {
        hex::encode([n; 32])
    }

    fn message(n: u8) -> Vec<u8> {
        vec![n; MESSAGE_SIZE]
    }

    async fn post(app: &Router, topic: &str, body: Vec<u8>) -> Response {
        app.clone()
            .oneshot(post_request(topic, body))
            .await
            .unwrap()
    }

    async fn pull(app: &Router, topic: &str, range: Option<&str>) -> Response {
        let mut request = Request::get(format!("/{topic}"));
        if let Some(range) = range {
            request = request.header(RANGE, range);
        }
        let request = request.body(Body::empty()).unwrap();
        app.clone().oneshot(request).await.unwrap()
    }

    async fn body_of(response: Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }

    /// Posts messages `0..count` to `topic`.
    async fn post_messages(app: &Router, topic: &str, count: u8) {
        for n in 0..count {
            let response = post(app, topic, message(n)).await;
            assert_eq!(response.status(), StatusCode::CREATED);
            assert_eq!(response.headers()[SEQ_HEADER], n.to_string());
        }
    }

    /// The bytes of messages `from..to` back to back, as a 206 body should hold them.
    fn messages_between(from: u8, to: u8) -> Vec<u8> {
        (from..to).flat_map(message).collect()
    }

    #[tokio::test]
    async fn post_rejects_wrong_size() {
        let app = new_app();
        let t = topic(1);

        let short = post(&app, &t, vec![1; 10]).await;
        assert_eq!(short.status(), StatusCode::BAD_REQUEST);
        let expected = format!("message must be exactly {MESSAGE_SIZE} bytes, got 10");
        assert_eq!(body_of(short).await, expected.as_bytes());

        let empty = post(&app, &t, vec![]).await;
        assert_eq!(empty.status(), StatusCode::BAD_REQUEST);

        let long = post(&app, &t, vec![1; MESSAGE_SIZE + 1]).await;
        assert_eq!(long.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // Nothing was stored by any of them.
        let after = pull(&app, &t, Some("bytes=0-")).await;
        assert_eq!(after.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[tokio::test]
    async fn post_rejects_invalid_topic() {
        let app = new_app();
        let non_hex = "zz".repeat(32);
        let too_short = "ab".repeat(31);
        for t in ["abc", too_short.as_str(), non_hex.as_str()] {
            let response = post(&app, t, message(1)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{t}");
        }
    }

    #[tokio::test]
    async fn pull_everything() {
        let app = new_app();
        let t = topic(1);
        post_messages(&app, &t, 3).await;

        let response = pull(&app, &t, Some("bytes=0-")).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/octet-stream");
        let expected_range = format!("bytes 0-{}/{}", 3 * SIZE - 1, 3 * SIZE);
        assert_eq!(response.headers()[CONTENT_RANGE], expected_range.as_str());
        assert_eq!(body_of(response).await, messages_between(0, 3));
    }

    #[tokio::test]
    async fn pull_from_middle_returns_only_newer() {
        let app = new_app();
        let t = topic(1);
        post_messages(&app, &t, 3).await;

        let range = format!("bytes={SIZE}-");
        let response = pull(&app, &t, Some(&range)).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        let expected_range = format!("bytes {SIZE}-{}/{}", 3 * SIZE - 1, 3 * SIZE);
        assert_eq!(response.headers()[CONTENT_RANGE], expected_range.as_str());
        assert_eq!(body_of(response).await, messages_between(1, 3));
    }

    #[tokio::test]
    async fn pull_bounded_range() {
        let app = new_app();
        let t = topic(1);
        post_messages(&app, &t, 3).await;

        let range = format!("bytes=0-{}", 2 * SIZE - 1);
        let response = pull(&app, &t, Some(&range)).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        let expected_range = format!("bytes 0-{}/{}", 2 * SIZE - 1, 3 * SIZE);
        assert_eq!(response.headers()[CONTENT_RANGE], expected_range.as_str());
        assert_eq!(body_of(response).await, messages_between(0, 2));
    }

    #[tokio::test]
    async fn pull_at_end_is_416_with_length() {
        let app = new_app();
        let t = topic(1);
        post_messages(&app, &t, 3).await;

        let range = format!("bytes={}-", 3 * SIZE);
        let response = pull(&app, &t, Some(&range)).await;
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        let expected_range = format!("bytes */{}", 3 * SIZE);
        assert_eq!(response.headers()[CONTENT_RANGE], expected_range.as_str());
        assert!(body_of(response).await.is_empty());
    }

    #[tokio::test]
    async fn pull_unknown_topic_is_416_with_zero() {
        let app = new_app();
        let response = pull(&app, &topic(9), Some("bytes=0-")).await;
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */0");
    }

    #[tokio::test]
    async fn pull_rejects_bad_ranges() {
        let app = new_app();
        let t = topic(1);
        post_messages(&app, &t, 3).await;

        let misaligned = format!("bytes={}-", SIZE + 1);
        let end_before_start = format!("bytes={SIZE}-0");
        let too_small = format!("bytes=0-{}", SIZE - 2);
        for range in [
            misaligned.as_str(),
            "bytes=1-",
            "items=0-",
            "0-",
            "bytes=-100",
            "bytes=0-1,5-6",
            "bytes=abc-",
            end_before_start.as_str(),
            too_small.as_str(),
        ] {
            let response = pull(&app, &t, Some(range)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{range}");
        }

        let missing = pull(&app, &t, None).await;
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn offline_client_catches_up_in_one_request() {
        let app = new_app();
        let t = topic(1);

        // Alice has read the first two messages, so her cursor is at 2.
        post_messages(&app, &t, 2).await;
        let first_read = pull(&app, &t, Some("bytes=0-")).await;
        assert_eq!(body_of(first_read).await, messages_between(0, 2));

        // Four more arrive while she's offline.
        for n in 2..6 {
            assert_eq!(
                post(&app, &t, message(n)).await.status(),
                StatusCode::CREATED
            );
        }

        // One request from her cursor returns exactly the four she missed.
        let range = format!("bytes={}-", 2 * SIZE);
        let catch_up = pull(&app, &t, Some(&range)).await;
        assert_eq!(catch_up.status(), StatusCode::PARTIAL_CONTENT);
        let expected_range = format!("bytes {}-{}/{}", 2 * SIZE, 6 * SIZE - 1, 6 * SIZE);
        assert_eq!(catch_up.headers()[CONTENT_RANGE], expected_range.as_str());
        let body = body_of(catch_up).await;
        assert_eq!(body.len(), 4 * MESSAGE_SIZE);
        assert_eq!(body, messages_between(2, 6));

        // Her cursor is now 6, and the next pull says nothing new.
        let range = format!("bytes={}-", 6 * SIZE);
        let again = pull(&app, &t, Some(&range)).await;
        assert_eq!(again.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        let expected_range = format!("bytes */{}", 6 * SIZE);
        assert_eq!(again.headers()[CONTENT_RANGE], expected_range.as_str());
    }
}
