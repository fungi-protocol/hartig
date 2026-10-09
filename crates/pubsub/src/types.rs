use std::{fmt, str::FromStr};

/// Every message is exactly this many bytes.
pub const MESSAGE_SIZE: usize = 1024;

/// Topic IDs come from a shared secret, so they are fixed size.
/// Using 32 bytes for now.
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct TopicId([u8; 32]);
pub type Seq = u64;

impl TopicId {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// Knowing a topic ID grants access to the topic, so never print its bytes.
impl fmt::Debug for TopicId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TopicId(<redacted>)")
    }
}

/// Why a topic ID string was rejected. Never includes the input, since a topic ID is a secret.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseTopicIdError {
    #[error("topic ID must be 64 hex characters, got {0}")]
    WrongLength(usize),
    #[error("topic ID must contain only hex characters")]
    InvalidHex,
}

impl FromStr for TopicId {
    type Err = ParseTopicIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(ParseTopicIdError::WrongLength(s.len()));
        }

        let mut bytes = [0u8; 32];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| ParseTopicIdError::InvalidHex)?;
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_hex_parses() {
        let expected = TopicId::new([0xab; 32]);
        assert_eq!("ab".repeat(32).parse::<TopicId>(), Ok(expected));
        assert_eq!("AB".repeat(32).parse::<TopicId>(), Ok(expected));
    }

    #[test]
    fn wrong_length_is_rejected() {
        for len in [0, 8, 63, 65] {
            let id = "0".repeat(len);
            assert!(matches!(
                id.parse::<TopicId>(),
                Err(ParseTopicIdError::WrongLength(n)) if n == len
            ));
        }
    }

    #[test]
    fn invalid_hex_is_rejected() {
        let id = format!("GG{}", "0".repeat(62));
        assert_eq!(id.parse::<TopicId>(), Err(ParseTopicIdError::InvalidHex));
    }

    #[test]
    fn error_never_contains_input() {
        let id = format!("{}Q", "0".repeat(63));
        let err = id.parse::<TopicId>().unwrap_err();
        assert!(!format!("{err}").contains('Q'));
        assert!(!format!("{err:?}").contains('Q'));
    }
}
