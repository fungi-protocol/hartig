//! IO-free Oblivious HTTP gateway core (RFC 9458) with BHTTP messages (RFC 9292).

mod key_config;
pub mod message;

pub use key_config::{
    ENCAPSULATED_MESSAGE_BYTES, OHTTP_KEYS_MEDIA_TYPE, PADDED_BHTTP_REQUEST_BYTES,
    PADDED_BHTTP_RESPONSE_BYTES, ServerKeyConfig,
};
