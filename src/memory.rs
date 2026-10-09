use crate::store::{AppendResult, ReadResult, StoreError, TopicStore};
use crate::types::{Seq, TopicId};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
struct TopicLog {
    messages: Vec<Vec<u8>>,
    dupe_index: HashMap<[u8; 32], Seq>,
}

pub struct MemoryStore {
    /// Kept only in memory , lost on restart
    topics: Mutex<HashMap<TopicId, TopicLog>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
        }
    }
}

fn hash_message(message: &[u8]) -> [u8; 32] {
    Sha256::digest(message).into()
}

impl TopicStore for MemoryStore {
    fn append(&self, topic: &TopicId, message: Vec<u8>) -> Result<AppendResult, StoreError> {
        let message_hash = hash_message(&message);

        let mut guard = self
            .topics
            .lock()
            .map_err(|_| StoreError::Backend("lock poisoned".to_owned()))?;

        // Creating the topic and appending is atomic
        let log = guard.entry(*topic).or_default();

        if let Some(&seq) = log.dupe_index.get(&message_hash) {
            return Ok(AppendResult::Duplicate { seq });
        }

        let seq = log.messages.len() as Seq;
        log.messages.push(message);
        log.dupe_index.insert(message_hash, seq);

        Ok(AppendResult::Inserted { seq })
    }

    fn read(
        &self,
        topic: &TopicId,
        start: Seq,
        max: Option<usize>,
    ) -> Result<ReadResult, StoreError> {
        let guard = self
            .topics
            .lock()
            .map_err(|_| StoreError::Backend("lock poisoned".to_owned()))?;

        //  reading must never create a topic.
        let Some(log) = guard.get(topic) else {
            return Ok(ReadResult {
                messages: Vec::new(),
                total_count: 0,
            });
        };

        let len = log.messages.len();
        let total_count = len as Seq;

        let start = match usize::try_from(start) {
            Ok(start) if start < len => start,
            _ => {
                return Ok(ReadResult {
                    messages: Vec::new(),
                    total_count,
                });
            }
        };

        let end = match max {
            Some(max) => len.min(start.saturating_add(max)),
            None => len,
        };

        Ok(ReadResult {
            messages: log.messages[start..end].to_vec(),
            total_count,
        })
    }
}
