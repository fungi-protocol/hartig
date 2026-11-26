//! The gateway's OHTTP key configuration (RFC 9458 §3) and message sizes.

use http::{Response, StatusCode};
use ohttp::{
    KeyConfig, SymmetricSuite,
    hpke::{Aead, Kdf, Kem},
};

use crate::gateway::gateway_response;

/// Gateway KEM: DHKEM(X25519, HKDF-SHA256).
const KEM: Kem = Kem::X25519Sha256;

/// The sole advertised symmetric suite: HKDF-SHA256 and AES-128-GCM.
const SYMMETRIC: SymmetricSuite = SymmetricSuite::new(Kdf::HkdfSha256, Aead::Aes128Gcm);

/// Size of every Encapsulated Request and Response (provisional).
pub const ENCAPSULATED_MESSAGE_BYTES: usize = 8192;

/// Exact size of a padded BHTTP request inside an Encapsulated Request
/// (RFC 9458 §4.3): header, then `enc`, then the ciphertext and its tag.
pub const PADDED_BHTTP_REQUEST_BYTES: usize =
    ENCAPSULATED_MESSAGE_BYTES - (OHTTP_REQ_HEADER_BYTES + N_ENC + N_T);

/// Exact size of a padded BHTTP response inside an Encapsulated Response
/// (RFC 9458 §4.4): response nonce, then the ciphertext and its tag.
pub const PADDED_BHTTP_RESPONSE_BYTES: usize =
    ENCAPSULATED_MESSAGE_BYTES - (RESPONSE_NONCE_BYTES + N_T);

// RFC 9180 sizes of the suite; `suite_sizes` checks them against `ohttp`.

/// `key_id` (1) + `kem_id`, `kdf_id`, `aead_id` (2 each) (RFC 9458 §4.1).
const OHTTP_REQ_HEADER_BYTES: usize = 1 + 2 + 2 + 2;
/// `Nenc` of [`KEM`].
const N_ENC: usize = 32;
/// `Nt` of the [`SYMMETRIC`] AEAD.
const N_T: usize = 16;
/// `max(Nn, Nk)` of the [`SYMMETRIC`] AEAD (RFC 9458 §4.4).
const RESPONSE_NONCE_BYTES: usize = 16;

/// Media type of an encoded list of key configurations (RFC 9458 §3.2).
pub const OHTTP_KEYS_MEDIA_TYPE: &str = "application/ohttp-keys";

/// The gateway's HPKE key pair and its encoded `application/ohttp-keys`.
#[derive(Debug)]
pub struct ServerKeyConfig {
    pub(crate) server: ohttp::Server,
    encoded_ohttp_keys: Vec<u8>,
}

impl ServerKeyConfig {
    /// Generate a fresh key pair from the OS RNG.
    pub fn generate(key_id: u8) -> Result<Self, ohttp::Error> {
        let config = KeyConfig::new(key_id, KEM, vec![SYMMETRIC])?;
        Ok(Self {
            encoded_ohttp_keys: KeyConfig::encode_list(&[&config])?,
            server: ohttp::Server::new(config)?,
        })
    }

    /// The `application/ohttp-keys` body (RFC 9458 §3.2).
    pub fn encoded_ohttp_keys(&self) -> &[u8] {
        &self.encoded_ohttp_keys
    }

    /// Build a `200 application/ohttp-keys` response with wildcard CORS.
    pub fn ohttp_keys_response(&self) -> Response<Vec<u8>> {
        gateway_response(
            StatusCode::OK,
            Some(OHTTP_KEYS_MEDIA_TYPE),
            self.encoded_ohttp_keys.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use http::header;

    use super::*;

    #[test]
    fn suite_sizes() {
        let aead = SYMMETRIC.aead();
        assert_eq!(N_ENC, KEM.n_enc());
        assert_eq!(N_T, aead.n_t());
        assert_eq!(RESPONSE_NONCE_BYTES, aead.n_n().max(aead.n_k()));
    }

    #[test]
    fn ohttp_keys_response() {
        let response = ServerKeyConfig::generate(1)
            .expect("generate")
            .ohttp_keys_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            OHTTP_KEYS_MEDIA_TYPE
        );
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

        let configs = KeyConfig::decode_list(response.body()).expect("decode list");
        assert_eq!(configs.len(), 1);
    }
}
