use std::{fmt, str::FromStr};

/// Fixed message size in bytes. A placeholder until the wire-level spec settles it.
pub const MESSAGE_SIZE: usize = 1024;

/// Topic IDs come from a shared secret, so they are fixed size.
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct TopicId([u8; 16]);
pub type Seq = u64;

impl TopicId {
    pub fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
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
    #[error("topic ID must be 32 hex characters, got {0}")]
    WrongLength(usize),
    #[error("topic ID must contain only hex characters")]
    InvalidHex,
}

impl FromStr for TopicId {
    type Err = ParseTopicIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let len = s.chars().count();
        if len != 32 {
            return Err(ParseTopicIdError::WrongLength(len));
        }

        let mut bytes = [0u8; 16];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| ParseTopicIdError::InvalidHex)?;
        Ok(Self(bytes))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn valid_hex_parses() {
        let expected = TopicId::new([0xab; 16]);
        assert_eq!("ab".repeat(16).parse::<TopicId>(), Ok(expected));
        assert_eq!("AB".repeat(16).parse::<TopicId>(), Ok(expected));
    }

    #[test]
    fn wrong_length_is_rejected() {
        for len in [0, 8, 31, 33] {
            let id = "0".repeat(len);
            assert!(matches!(
                id.parse::<TopicId>(),
                Err(ParseTopicIdError::WrongLength(n)) if n == len
            ));
        }
    }

    #[test]
    fn invalid_hex_is_rejected() {
        let id = format!("GG{}", "0".repeat(30));
        assert_eq!(id.parse::<TopicId>(), Err(ParseTopicIdError::InvalidHex));
    }

    #[test]
    fn length_is_counted_in_characters() {
        // 32 characters but 33 bytes.
        let id = format!("{}é", "0".repeat(31));
        assert_eq!(id.parse::<TopicId>(), Err(ParseTopicIdError::InvalidHex));
    }

    #[test]
    fn error_never_contains_input() {
        let id = format!("{}Q", "0".repeat(31));
        let err = id.parse::<TopicId>().unwrap_err();
        assert!(!format!("{err}").contains('Q'));
        assert!(!format!("{err:?}").contains('Q'));
    }
}
