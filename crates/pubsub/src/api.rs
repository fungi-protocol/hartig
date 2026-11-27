use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};

use crate::store::StoreError;
use crate::types::{MESSAGE_SIZE, ParseTopicIdError, Seq};

/// Why a `Range` header was rejected
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum RangeError {
    #[error("range must be `START-` or `START-END`")]
    Malformed,
    #[error("only a single range is supported")]
    MultipleRanges,
    #[error("range must have a start; suffix ranges are not supported")]
    NoStartRange,
    #[error("range bounds must be unsigned integers")]
    InvalidNumber,
    #[error("range end must not be before its start")]
    EndBeforeStart,
}

/// Everything a pub-sub request can fail with.
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
            Self::InvalidTopic(_)
            | Self::WrongSize(_)
            | Self::MissingRange
            | Self::InvalidRange(_) => StatusCode::BAD_REQUEST,
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

/// What every pub-sub handler returns. Errors become responses through `IntoResponse`.
type ApiResult<T> = Result<T, ApiError>;

/// A seq range turned into what `TopicStore::read` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MessageRange {
    start: Seq,
    max: Option<usize>,
}

/// Parse a `Range` header value of the form `START-` or `START-END`, where both are inclusive seqs.
fn parse_range(header: &str) -> Result<MessageRange, RangeError> {
    // Checked before splitting.
    if header.contains(',') {
        return Err(RangeError::MultipleRanges);
    }

    let (start, end) = header.split_once('-').ok_or(RangeError::Malformed)?;
    if end.contains('-') {
        return Err(RangeError::Malformed);
    }
    let (start, end) = (start.trim(), end.trim());

    if start.is_empty() {
        return Err(RangeError::NoStartRange);
    }
    let start = parse_number(start)?;

    let max = if end.is_empty() {
        None
    } else {
        let end = parse_number(end)?;
        if end < start {
            return Err(RangeError::EndBeforeStart);
        }
        let count = u128::from(end - start) + 1;
        // The store caps `max` at the topic's length anyway.
        Some(usize::try_from(count).unwrap_or(usize::MAX))
    };

    Ok(MessageRange { start, max })
}

/// Parse an unsigned integer. `u64::from_str` alone would also accept a leading `+`.
fn parse_number(s: &str) -> Result<Seq, RangeError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(RangeError::InvalidNumber);
    }
    s.parse().map_err(|_| RangeError::InvalidNumber)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

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
                ApiError::InvalidRange(RangeError::EndBeforeStart),
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

    fn range(start: Seq, max: Option<usize>) -> Result<MessageRange, RangeError> {
        Ok(MessageRange { start, max })
    }

    #[test]
    fn open_range_reads_to_the_end() {
        assert_eq!(parse_range("0-"), range(0, None));
        assert_eq!(parse_range("5-"), range(5, None));
    }

    #[test]
    fn bounded_range_is_inclusive() {
        assert_eq!(parse_range("0-10"), range(0, Some(11)));
        assert_eq!(parse_range("2-5"), range(2, Some(4)));
        assert_eq!(parse_range("5-5"), range(5, Some(1)));
    }

    #[test]
    fn whitespace_around_bounds_is_allowed() {
        assert_eq!(parse_range("0 - 10"), range(0, Some(11)));
        assert_eq!(parse_range(" 5 - "), range(5, None));
    }

    #[test]
    fn rejects_multiple_ranges() {
        assert_eq!(parse_range("0-1,5-6"), Err(RangeError::MultipleRanges));
        assert_eq!(parse_range("abc,"), Err(RangeError::MultipleRanges));
    }

    #[test]
    fn rejects_malformed_ranges() {
        for header in ["", "5", "abc", "1-2-3", "0--"] {
            assert_eq!(parse_range(header), Err(RangeError::Malformed), "{header}");
        }
    }

    #[test]
    fn rejects_suffix_range() {
        assert_eq!(parse_range("-5"), Err(RangeError::NoStartRange));
    }

    #[test]
    fn rejects_invalid_numbers() {
        let too_big = format!("{}-", "9".repeat(20));
        for header in ["abc-", "+5-", "0-+9", "bytes=0-", too_big.as_str()] {
            assert_eq!(
                parse_range(header),
                Err(RangeError::InvalidNumber),
                "{header}"
            );
        }
    }

    #[test]
    fn rejects_end_before_start() {
        assert_eq!(parse_range("10-5"), Err(RangeError::EndBeforeStart));
    }

    #[test]
    fn huge_bounded_range_does_not_overflow() {
        let result = parse_range(&format!("0-{}", Seq::MAX)).unwrap();
        assert_eq!(result.start, 0);
        assert!(result.max.is_some_and(|max| max > 0));
    }
}
