use hartig_pubsub::types::TopicId;

/// Builds the URL for a topic: the base, then `/`, then the topic ID in hex.
pub(crate) fn create_url(base: &str, topic_id: &TopicId) -> String {
    let host = base.trim_end_matches('/');
    let topic_id = hex::encode(topic_id.as_bytes());
    format!("{host}/{topic_id}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn url_is_base_then_hex() {
        let url = create_url("http://host/pubsub", &TopicId::new([0xab; 16]));
        assert_eq!(url, format!("http://host/pubsub/{}", "ab".repeat(16)));
    }

    #[test]
    fn hex_parses_back_into_topic() {
        let topic = TopicId::new([0x5c; 16]);
        let url = create_url("http://host/pubsub", &topic);
        let (_, hex) = url.rsplit_once('/').unwrap();
        assert_eq!(hex.parse::<TopicId>(), Ok(topic));
    }

    #[test]
    fn trailing_slash_gives_same_url() {
        let topic = TopicId::new([0x01; 16]);
        assert_eq!(
            create_url("http://host/pubsub/", &topic),
            create_url("http://host/pubsub", &topic)
        );
    }
}
