//! IO-free Oblivious HTTP gateway core (RFC 9458) with BHTTP messages (RFC 9292).
//!
//! [`Gateway::decapsulate`] decapsulates a request and applies its credential policy.
//! Callers dispatch admitted requests and encapsulate responses with
//! [`ResponseContext`], or use [`Gateway::handle`] for a synchronous target resource. HTTP transport and
//! routing are supplied by the caller.

mod gateway;
mod key_config;
pub mod message;

pub use gateway::{
    Access, Admission, CredentialPolicy, DecapsulatedRequest, DecapsulationError, Disposition,
    EncapsulationError, Gateway, NoEnforcement, OHTTP_REQ_MEDIA_TYPE, OHTTP_RES_MEDIA_TYPE,
    ResponseContext, cors_preflight_response,
};
pub use key_config::{
    ENCAPSULATED_MESSAGE_BYTES, OHTTP_KEYS_MEDIA_TYPE, PADDED_BHTTP_REQUEST_BYTES,
    PADDED_BHTTP_RESPONSE_BYTES, ServerKeyConfig,
};
