use crate::types::{Seq, TopicId};

/// The storage backend itself failed, not the caller's input.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage backend failed: {0}")]
    Backend(String),
}

/// What happened to an appended message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendResult {
    /// The message was new and was stored at `seq`.
    /// Maps to a 201 status code.
    Inserted { seq: Seq },

    /// The message was already in the topic at `seq`; nothing was stored.
    /// Maps to a 204 status code.
    Duplicate { seq: Seq },
}

impl AppendResult {
    /// The message's position, whether it was new or a duplicate.
    pub fn seq(self) -> Seq {
        match self {
            Self::Inserted { seq } => seq,
            Self::Duplicate { seq } => seq,
        }
    }
}

/// Messages read from a topic, plus the topic's length at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResult {
    /// Messages in position order, starting at the requested position.
    pub messages: Vec<Vec<u8>>,
    /// Total messages in the topic, taken in the same atomic step as `messages`.
    pub total_count: Seq,
}

/// Per-topic message logs. Every backend must guarantee:
/// - Messages are append-only and kept in arrival order.
/// - Positions start at 0 per topic and never shift or get reused.
/// - Methods are safe to call concurrently; the backend does its own locking.
/// - Message size is not checked here; the HTTP layer enforces it.
pub trait TopicStore: Send + Sync {
    /// Append `message` to `topic`, taking ownership of it.
    /// - The first append creates the topic, atomically with storing the message.
    /// - A duplicate (same bytes, same topic) stores nothing and returns the original position.
    fn append(&self, topic: &TopicId, message: Vec<u8>) -> Result<AppendResult, StoreError>;

    /// Read up to `max` messages from position `start` (`None` reads to the end).
    /// - Reading past the end, or an unknown topic, returns no messages and is not an error.
    /// - Reading never creates a topic.
    fn read(
        &self,
        topic: &TopicId,
        start: Seq,
        max: Option<usize>,
    ) -> Result<ReadResult, StoreError>;
}

/// Checks every `TopicStore` backend must pass. Each backend calls these from its own tests.
#[cfg(test)]
pub(crate) mod store_contract {
    use super::{AppendResult, ReadResult, TopicStore};
    use crate::types::{Seq, TopicId};

    fn topic(n: u8) -> TopicId {
        TopicId::new([n; 32])
    }

    fn msg(n: u8) -> Vec<u8> {
        vec![n; 4]
    }

    fn inserted(seq: Seq) -> AppendResult {
        AppendResult::Inserted { seq }
    }

    fn read_all(store: &dyn TopicStore, t: &TopicId) -> ReadResult {
        store.read(t, 0, None).unwrap()
    }

    pub(crate) fn positions_increase_by_one(store: &dyn TopicStore) {
        let t = topic(1);
        for n in 0..3 {
            assert_eq!(store.append(&t, msg(n)).unwrap(), inserted(n.into()));
        }
    }

    pub(crate) fn topics_are_isolated(store: &dyn TopicStore) {
        let (a, b) = (topic(1), topic(2));
        assert_eq!(store.append(&a, msg(1)).unwrap(), inserted(0));
        assert_eq!(store.append(&b, msg(2)).unwrap(), inserted(0));
        assert_eq!(store.append(&a, msg(3)).unwrap(), inserted(1));
        assert_eq!(store.append(&b, msg(4)).unwrap(), inserted(1));

        let expected_a = ReadResult {
            messages: vec![msg(1), msg(3)],
            total_count: 2,
        };
        let expected_b = ReadResult {
            messages: vec![msg(2), msg(4)],
            total_count: 2,
        };
        assert_eq!(read_all(store, &a), expected_a);
        assert_eq!(read_all(store, &b), expected_b);
    }

    pub(crate) fn duplicate_is_stored_once(store: &dyn TopicStore) {
        let t = topic(1);
        assert_eq!(store.append(&t, msg(1)).unwrap(), inserted(0));
        assert_eq!(store.append(&t, msg(2)).unwrap(), inserted(1));
        assert_eq!(
            store.append(&t, msg(1)).unwrap(),
            AppendResult::Duplicate { seq: 0 }
        );

        let expected = ReadResult {
            messages: vec![msg(1), msg(2)],
            total_count: 2,
        };
        assert_eq!(read_all(store, &t), expected);
    }

    pub(crate) fn duplicates_are_per_topic(store: &dyn TopicStore) {
        assert_eq!(store.append(&topic(1), msg(1)).unwrap(), inserted(0));
        assert_eq!(store.append(&topic(2), msg(1)).unwrap(), inserted(0));
    }

    pub(crate) fn read_from_position_returns_newer(store: &dyn TopicStore) {
        let t = topic(1);
        for n in 0..4 {
            store.append(&t, msg(n)).unwrap();
        }

        let expected = ReadResult {
            messages: vec![msg(2), msg(3)],
            total_count: 4,
        };
        assert_eq!(store.read(&t, 2, None).unwrap(), expected);
    }

    pub(crate) fn read_respects_max(store: &dyn TopicStore) {
        let t = topic(1);
        for n in 0..4 {
            store.append(&t, msg(n)).unwrap();
        }

        let expected = ReadResult {
            messages: vec![msg(0), msg(1)],
            total_count: 4,
        };
        assert_eq!(store.read(&t, 0, Some(2)).unwrap(), expected);
        assert_eq!(
            store.read(&t, 0, Some(usize::MAX)).unwrap(),
            read_all(store, &t)
        );
    }

    pub(crate) fn read_past_end_returns_nothing(store: &dyn TopicStore) {
        let t = topic(1);
        for n in 0..4 {
            store.append(&t, msg(n)).unwrap();
        }

        let expected = ReadResult {
            messages: vec![],
            total_count: 4,
        };
        assert_eq!(store.read(&t, 4, None).unwrap(), expected);
        assert_eq!(store.read(&t, 100, None).unwrap(), expected);
        assert_eq!(store.read(&t, Seq::MAX, None).unwrap(), expected);
    }

    pub(crate) fn read_unknown_topic_returns_nothing(store: &dyn TopicStore) {
        let t = topic(1);
        let empty = ReadResult {
            messages: vec![],
            total_count: 0,
        };
        assert_eq!(read_all(store, &t), empty);

        // The topic still starts at 0 after being read. An empty topic created
        // by the read looks the same through the trait, so each backend checks
        // "reading creates nothing" in its own tests.
        assert_eq!(store.append(&t, msg(1)).unwrap(), inserted(0));
        let expected = ReadResult {
            messages: vec![msg(1)],
            total_count: 1,
        };
        assert_eq!(read_all(store, &t), expected);
    }

    pub(crate) fn fan_out(store: &dyn TopicStore) {
        let t = topic(1);
        let sent = vec![msg(1), msg(2), msg(3)];
        for m in &sent {
            store.append(&t, m.clone()).unwrap();
        }

        // Alice, Bob and Charlie each pull the topic from the start.
        for _reader in ["alice", "bob", "charlie"] {
            assert_eq!(read_all(store, &t).messages, sent);
        }

        // Bob, caught up at position 3, gets only the new message.
        store.append(&t, msg(4)).unwrap();
        let expected = ReadResult {
            messages: vec![msg(4)],
            total_count: 4,
        };
        assert_eq!(store.read(&t, 3, None).unwrap(), expected);
    }

    pub(crate) fn concurrent_appends_get_unique_gap_free_positions(store: &dyn TopicStore) {
        const THREADS: u8 = 8;
        const PER_THREAD: u8 = 100;
        let t = topic(1);

        let mut seqs: Vec<Seq> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..THREADS)
                .map(|thread| {
                    s.spawn(move || {
                        (0..PER_THREAD)
                            .map(|i| store.append(&t, vec![thread, i]).unwrap().seq())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect()
        });

        let total = Seq::from(THREADS) * Seq::from(PER_THREAD);
        seqs.sort_unstable();
        assert_eq!(seqs, (0..total).collect::<Vec<_>>());

        let all = read_all(store, &t);
        assert_eq!(all.total_count, total);
        assert_eq!(all.messages.len() as Seq, total);
    }
}
