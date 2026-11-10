//! Binary HTTP (RFC 9292) encoding and decoding, with zero padding
//! (RFC 9292 §3.8). Callers enforce input size limits.

use std::io::Cursor;

use http::{HeaderMap, Request, Response, Uri};

#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("malformed BHTTP: {0}")]
    Bhttp(#[from] bhttp::Error),
    #[error("invalid HTTP message: {0}")]
    Http(#[from] http::Error),
    #[error("message is longer than the padded length")]
    TooLarge,
    #[error("padding after the BHTTP message is not all zero")]
    Padding,
    #[error("wrong message kind, CONNECT, or trailers or informational responses present")]
    Unsupported,
    #[error("invalid request target: no scheme, empty path, or userinfo in the authority")]
    Target,
}

/// Zero-pad `message` to `len` bytes, or return an error if it is already larger.
pub fn pad(mut message: Vec<u8>, len: usize) -> Result<Vec<u8>, MessageError> {
    if message.len() > len {
        return Err(MessageError::TooLarge);
    }
    message.resize(len, 0);
    Ok(message)
}

/// Parse a padded BHTTP request in either framing mode.
pub fn decode_request(plaintext: &[u8]) -> Result<Request<Vec<u8>>, MessageError> {
    let message = read_padded(plaintext)?;
    let control = message.control();
    // bhttp's binary decoder accepts CONNECT, which has no target resource here.
    if !control.is_request() || control.method() == Some(&b"CONNECT"[..]) {
        return Err(MessageError::Unsupported);
    }

    // Every non-CONNECT request carries a scheme and a non-empty path, and
    // its authority has no userinfo (RFC 9113 §8.3.1).
    let scheme = control.scheme().unwrap_or_default();
    let path = control.path().unwrap_or_default();
    let authority = control.authority().unwrap_or_default();
    if scheme.is_empty() || path.is_empty() || authority.contains(&b'@') {
        return Err(MessageError::Target);
    }
    let mut target = Uri::builder();
    // Without an authority, keep only the origin-form path.
    if !authority.is_empty() {
        target = target.scheme(scheme).authority(authority);
    }
    let request = Request::builder()
        .method(control.method().unwrap_or_default())
        .uri(target.path_and_query(path).build()?);
    let request = message
        .header()
        .iter()
        .fold(request, |b, f| b.header(f.name(), f.value()));
    // Invalid methods, targets, and field names or values surface here.
    Ok(request.body(message.content().to_vec())?)
}

/// Encode a response as unpadded known-length BHTTP, rejecting 1xx statuses.
pub fn encode_response(response: &Response<Vec<u8>>) -> Result<Vec<u8>, MessageError> {
    if response.status().is_informational() {
        return Err(MessageError::Unsupported);
    }
    let status = bhttp::StatusCode::try_from(response.status().as_u16())?;
    write_known_length(
        bhttp::Message::response(status),
        response.headers(),
        response.body(),
    )
}

/// Encode a request as unpadded known-length BHTTP, defaulting the scheme to
/// `https` and rejecting userinfo (RFC 9113 §8.3.1).
pub fn encode_request(request: &Request<Vec<u8>>) -> Result<Vec<u8>, MessageError> {
    let uri = request.uri();
    let authority = uri.authority().map(|a| a.as_str()).unwrap_or("");
    if authority.contains('@') {
        return Err(MessageError::Target);
    }
    let message = bhttp::Message::request(
        request.method().as_str().into(),
        uri.scheme_str().unwrap_or("https").into(),
        authority.into(),
        uri.path_and_query()
            .map(|p| p.as_str())
            .unwrap_or("/")
            .into(),
    );
    write_known_length(message, request.headers(), request.body())
}

/// Parse a padded BHTTP response plaintext (client side).
pub fn decode_response(plaintext: &[u8]) -> Result<Response<Vec<u8>>, MessageError> {
    let message = read_padded(plaintext)?;
    let status = message
        .control()
        .status()
        .ok_or(MessageError::Unsupported)?;
    let response = Response::builder().status(status.code());
    let response = message
        .header()
        .iter()
        .fold(response, |b, f| b.header(f.name(), f.value()));
    Ok(response.body(message.content().to_vec())?)
}

/// Read one message, rejecting nonzero padding, trailers, and informational responses.
fn read_padded(plaintext: &[u8]) -> Result<bhttp::Message, MessageError> {
    let mut cursor = Cursor::new(plaintext);
    let message = bhttp::Message::read_bhttp(&mut cursor)?;
    let end = usize::try_from(cursor.position()).map_err(|_| MessageError::Padding)?;
    if plaintext
        .get(end..)
        .unwrap_or_default()
        .iter()
        .any(|&b| b != 0)
    {
        return Err(MessageError::Padding);
    }
    if !message.trailer().is_empty() || !message.informational().is_empty() {
        return Err(MessageError::Unsupported);
    }
    Ok(message)
}

fn write_known_length(
    mut message: bhttp::Message,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Vec<u8>, MessageError> {
    for (name, value) in headers {
        message.put_header(name.as_str(), value.as_bytes());
    }
    message.write_content(body);
    let mut out = Vec::new();
    message.write_bhttp(bhttp::Mode::KnownLength, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Method;

    fn sample_request() -> Request<Vec<u8>> {
        Request::builder()
            .method(Method::POST)
            .uri("https://gateway.example/mailbox/abc?x=1")
            .header("content-type", "application/octet-stream")
            .body(b"payload".to_vec())
            .expect("valid request")
    }

    #[test]
    fn pad_fills_with_zeros_and_rejects_overflow() {
        assert_eq!(pad(vec![1, 2, 3], 6).expect("fits"), [1, 2, 3, 0, 0, 0]);
        assert!(matches!(pad(vec![0; 9], 8), Err(MessageError::TooLarge)));
    }

    #[test]
    fn request_round_trip_with_padding() {
        let padded = pad(encode_request(&sample_request()).expect("encode"), 512).expect("fits");
        let decoded = decode_request(&padded).expect("decode");
        assert_eq!(decoded.method(), Method::POST);
        assert_eq!(decoded.uri(), "https://gateway.example/mailbox/abc?x=1");
        assert_eq!(
            decoded.headers()["content-type"],
            "application/octet-stream"
        );
        assert_eq!(decoded.body(), b"payload");
    }

    #[test]
    fn origin_form_target() {
        let message = bhttp::Message::request(
            b"GET".to_vec(),
            b"https".to_vec(),
            Vec::new(),
            b"/k".to_vec(),
        );
        let mut out = Vec::new();
        message
            .write_bhttp(bhttp::Mode::KnownLength, &mut out)
            .expect("write");
        assert_eq!(decode_request(&out).expect("decode").uri(), "/k");
    }

    #[test]
    fn indeterminate_length_request() {
        let message = bhttp::Message::request(
            b"GET".to_vec(),
            b"https".to_vec(),
            b"a.example".to_vec(),
            b"/".to_vec(),
        );
        let mut out = Vec::new();
        message
            .write_bhttp(bhttp::Mode::IndeterminateLength, &mut out)
            .expect("write");
        let padded = pad(out, 64).expect("fits");
        assert_eq!(
            decode_request(&padded).expect("decode").method(),
            Method::GET
        );
    }

    #[test]
    fn nonzero_padding_is_rejected() {
        let mut padded =
            pad(encode_request(&sample_request()).expect("encode"), 512).expect("fits");
        if let Some(last) = padded.last_mut() {
            *last = 1;
        }
        assert!(matches!(
            decode_request(&padded),
            Err(MessageError::Padding)
        ));
    }

    #[test]
    fn userinfo_is_rejected() {
        let request = Request::builder()
            .uri("https://user:pass@a.example/x")
            .body(Vec::new())
            .expect("valid request");
        assert!(matches!(
            encode_request(&request),
            Err(MessageError::Target)
        ));

        let message = bhttp::Message::request(
            b"GET".to_vec(),
            b"https".to_vec(),
            b"user:pass@a.example".to_vec(),
            b"/x".to_vec(),
        );
        let mut out = Vec::new();
        message
            .write_bhttp(bhttp::Mode::KnownLength, &mut out)
            .expect("write");
        assert!(matches!(decode_request(&out), Err(MessageError::Target)));
    }

    #[test]
    fn response_is_not_a_request() {
        let encoded = encode_response(&Response::new(Vec::new())).expect("encode");
        assert!(matches!(
            decode_request(&encoded),
            Err(MessageError::Unsupported)
        ));
    }

    #[test]
    fn connect_is_rejected() {
        let message = bhttp::Message::request(
            b"CONNECT".to_vec(),
            b"https".to_vec(),
            b"a.example:443".to_vec(),
            b"/".to_vec(),
        );
        let mut out = Vec::new();
        message
            .write_bhttp(bhttp::Mode::KnownLength, &mut out)
            .expect("write");
        assert!(matches!(
            decode_request(&out),
            Err(MessageError::Unsupported)
        ));
    }

    #[test]
    fn informational_response_is_rejected() {
        let response = Response::builder()
            .status(103)
            .body(Vec::new())
            .expect("valid");
        assert!(matches!(
            encode_response(&response),
            Err(MessageError::Unsupported)
        ));
    }

    #[test]
    fn response_round_trip() {
        let response = Response::builder()
            .status(404)
            .header("x-a", "b")
            .body(b"nope".to_vec())
            .expect("valid");
        let padded = pad(encode_response(&response).expect("encode"), 256).expect("fits");
        let decoded = decode_response(&padded).expect("decode");
        assert_eq!(decoded.status(), 404);
        assert_eq!(decoded.headers()["x-a"], "b");
        assert_eq!(decoded.body(), b"nope");
    }

    #[test]
    fn empty_scheme_is_rejected() {
        for authority in [&b"a.example"[..], b""] {
            let message = bhttp::Message::request(
                b"GET".to_vec(),
                Vec::new(),
                authority.to_vec(),
                b"/x".to_vec(),
            );
            let mut out = Vec::new();
            message
                .write_bhttp(bhttp::Mode::KnownLength, &mut out)
                .expect("write");
            assert!(matches!(decode_request(&out), Err(MessageError::Target)));
        }
    }

    #[test]
    fn empty_path_is_rejected() {
        let message = bhttp::Message::request(
            b"GET".to_vec(),
            b"https".to_vec(),
            b"a.example".to_vec(),
            Vec::new(),
        );
        let mut out = Vec::new();
        message
            .write_bhttp(bhttp::Mode::KnownLength, &mut out)
            .expect("write");
        assert!(matches!(decode_request(&out), Err(MessageError::Target)));
    }
}
