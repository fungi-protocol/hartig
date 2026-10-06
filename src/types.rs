use std::fmt;

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
