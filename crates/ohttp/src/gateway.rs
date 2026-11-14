//! The Oblivious Gateway Resource: decapsulate, admit, encapsulate, plus the
//! headers every gateway response carries.

use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};

use crate::{ENCAPSULATED_MESSAGE_BYTES, PADDED_BHTTP_RESPONSE_BYTES, ServerKeyConfig, message};

/// Media type of an encapsulated request (RFC 9458 §9.2).
pub const OHTTP_REQ_MEDIA_TYPE: &str = "message/ohttp-req";
/// Media type of an encapsulated response (RFC 9458 §9.3).
pub const OHTTP_RES_MEDIA_TYPE: &str = "message/ohttp-res";

/// An Oblivious Gateway Resource (RFC 9458 §2): one key configuration and one
/// credential slot.
pub struct Gateway<P = NoEnforcement> {
    key_config: ServerKeyConfig,
    policy: P,
}

impl Gateway {
    /// A gateway with no credential enforcement.
    pub fn new(key_config: ServerKeyConfig) -> Self {
        Self::with_policy(key_config, NoEnforcement)
    }
}

impl<P: CredentialPolicy> Gateway<P> {
    pub fn with_policy(key_config: ServerKeyConfig, policy: P) -> Self {
        Self { key_config, policy }
    }

    pub fn key_config(&self) -> &ServerKeyConfig {
        &self.key_config
    }

    /// Decapsulate an Encapsulated Request and apply the credential policy.
    ///
    /// Wrong lengths and decapsulation failures return [`DecapsulationError`]. Invalid
    /// BHTTP requests, `100-continue` expectations, and policy denials return a
    /// refusal for the caller to encapsulate.
    pub fn decapsulate(
        &self,
        encapsulated: &[u8],
    ) -> Result<DecapsulatedRequest, DecapsulationError> {
        if encapsulated.len() != ENCAPSULATED_MESSAGE_BYTES {
            return Err(DecapsulationError);
        }
        let (plaintext, response) = self
            .key_config
            .server
            .decapsulate(encapsulated)
            .map_err(|_| DecapsulationError)?;
        let response_context = ResponseContext { response };

        let disposition = match message::decode_request(&plaintext) {
            Err(_) => Disposition::Refused(empty_response(StatusCode::BAD_REQUEST)),
            // Encapsulated responses cannot be progressive, so the gateway
            // must reject a 100-continue expectation (RFC 9458 §5.1).
            Ok(request) if expects_100_continue(request.headers()) => {
                Disposition::Refused(empty_response(StatusCode::EXPECTATION_FAILED))
            }
            Ok(request) => match self.policy.admit(Access::of(request.method()), &request) {
                Admission::Admit => Disposition::Admitted(request),
                Admission::Deny(status) => Disposition::Refused(empty_response(status)),
            },
        };
        Ok(DecapsulatedRequest {
            disposition,
            response_context,
        })
    }

    /// Handle one Encapsulated Request with a synchronous target resource and
    /// build the gateway's HTTP response to the relay.
    pub fn handle(
        &self,
        encapsulated: &[u8],
        service: impl FnOnce(Request<Vec<u8>>) -> Response<Vec<u8>>,
    ) -> Response<Vec<u8>> {
        let DecapsulatedRequest {
            disposition,
            response_context,
        } = match self.decapsulate(encapsulated) {
            Ok(request) => request,
            Err(error) => return error.response(),
        };
        let response = match disposition {
            Disposition::Admitted(request) => service(request),
            Disposition::Refused(response) => response,
        };
        match response_context.encapsulate(response) {
            Ok(body) => Self::encapsulated_response(body),
            Err(EncapsulationError) => {
                gateway_response(StatusCode::INTERNAL_SERVER_ERROR, None, Vec::new())
            }
        }
    }

    /// Wrap an Encapsulated Response as the gateway's `200 message/ohttp-res` response.
    pub fn encapsulated_response(body: Vec<u8>) -> Response<Vec<u8>> {
        gateway_response(StatusCode::OK, Some(OHTTP_RES_MEDIA_TYPE), body)
    }
}

/// A decapsulated request and the means to answer it.
#[derive(Debug)]
pub struct DecapsulatedRequest {
    pub disposition: Disposition,
    pub response_context: ResponseContext,
}

#[derive(Debug)]
pub enum Disposition {
    /// Hand this to the resource.
    Admitted(Request<Vec<u8>>),
    /// Answer with this without calling any resource.
    Refused(Response<Vec<u8>>),
}

/// The response context of one decapsulated request, consumed to encapsulate
/// its response.
pub struct ResponseContext {
    response: ohttp::ServerResponse,
}

impl ResponseContext {
    /// Encode, pad, and encapsulate `response`. Encoding errors or oversized
    /// responses are replaced with an empty BHTTP `500`.
    /// Returns [`EncapsulationError`] if the fallback or encapsulation fails.
    pub fn encapsulate(self, response: Response<Vec<u8>>) -> Result<Vec<u8>, EncapsulationError> {
        let plaintext = match self.padded(&response) {
            Some(plaintext) => plaintext,
            None => self
                .padded(&empty_response(StatusCode::INTERNAL_SERVER_ERROR))
                .ok_or(EncapsulationError)?,
        };
        self.response
            .encapsulate(&plaintext)
            .map_err(|_| EncapsulationError)
    }

    fn padded(&self, response: &Response<Vec<u8>>) -> Option<Vec<u8>> {
        message::pad(
            message::encode_response(response).ok()?,
            PADDED_BHTTP_RESPONSE_BYTES,
        )
        .ok()
    }
}

impl std::fmt::Debug for ResponseContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseContext").finish_non_exhaustive()
    }
}

/// Response encapsulation failed; [`Gateway::handle`] returns an unencrypted `500`.
#[derive(Debug, thiserror::Error)]
#[error("failed to encapsulate the response")]
pub struct EncapsulationError;

/// An Encapsulated Request of the wrong size, or one that failed to decapsulate
/// (unknown key ID, unsupported suite, or AEAD failure).
/// [`DecapsulationError::response`] maps all such failures to the same unencrypted `422`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("failed to decapsulate the request")]
pub struct DecapsulationError;

impl DecapsulationError {
    pub fn response(self) -> Response<Vec<u8>> {
        gateway_response(StatusCode::UNPROCESSABLE_ENTITY, None, Vec::new())
    }
}

/// Whether a request reads or writes, from default HTTP semantics: safe
/// methods (RFC 9110 §9.2.1) read; everything else writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

impl Access {
    pub fn of(method: &Method) -> Self {
        match *method {
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE => Self::Read,
            _ => Self::Write,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Admit,
    /// Refuse with this BHTTP response status, encapsulated by the caller.
    Deny(StatusCode),
}

/// Admission policy applied to each successfully decoded BHTTP request.
pub trait CredentialPolicy: Send + Sync {
    fn admit(&self, access: Access, request: &Request<Vec<u8>>) -> Admission;
}

/// Admit every decoded request without checking credentials.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEnforcement;

impl CredentialPolicy for NoEnforcement {
    fn admit(&self, _access: Access, _request: &Request<Vec<u8>>) -> Admission {
        Admission::Admit
    }
}

/// Build a `204` preflight response allowing any origin, GET/POST/OPTIONS,
/// and the Content-Type header, without allowing credentials.
pub fn cors_preflight_response() -> Response<Vec<u8>> {
    let mut response = gateway_response(StatusCode::NO_CONTENT, None, Vec::new());
    let headers = response.headers_mut();
    for (name, value) in [
        (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS"),
        (header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type"),
        (header::ACCESS_CONTROL_MAX_AGE, "86400"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

/// Build a gateway response to the relay with `Access-Control-Allow-Origin: *`.
pub(crate) fn gateway_response(
    status: StatusCode,
    content_type: Option<&'static str>,
    body: Vec<u8>,
) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let headers: &mut HeaderMap = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    if let Some(content_type) = content_type {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    }
    response
}

fn empty_response(status: StatusCode) -> Response<Vec<u8>> {
    let mut response = Response::new(Vec::new());
    *response.status_mut() = status;
    response
}

/// Whether any `Expect` field carries `100-continue` (case-insensitive,
/// comma-separated list; RFC 9110 §10.1.1).
fn expects_100_continue(headers: &HeaderMap) -> bool {
    headers.get_all(header::EXPECT).iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value
                .split(',')
                .any(|item| item.trim().eq_ignore_ascii_case("100-continue"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_follows_method_safety() {
        assert_eq!(Access::of(&Method::GET), Access::Read);
        assert_eq!(Access::of(&Method::HEAD), Access::Read);
        assert_eq!(Access::of(&Method::POST), Access::Write);
        assert_eq!(Access::of(&Method::PUT), Access::Write);
        assert_eq!(Access::of(&Method::DELETE), Access::Write);
    }

    #[test]
    fn preflight_is_origin_agnostic_without_credentials() {
        let response = cors_preflight_response();
        let headers = response.headers();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS));
        assert_eq!(
            headers[header::ACCESS_CONTROL_ALLOW_METHODS],
            "GET, POST, OPTIONS"
        );
    }
}
