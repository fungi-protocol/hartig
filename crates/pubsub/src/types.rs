use std::fmt;

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
