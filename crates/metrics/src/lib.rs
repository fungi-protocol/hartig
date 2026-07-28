use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Metrics {
    capacity: AtomicU64,
    client_errors: AtomicU64,
    server_errors: AtomicU64,
}

impl Metrics {
    pub fn set_capacity(&self, capacity: u64) {
        self.capacity.store(capacity, Ordering::Relaxed);
    }

    pub fn record_client_error(&self) {
        self.client_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_server_error(&self) {
        self.server_errors.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            concat!(
                "# TYPE hartig_capacity gauge\n",
                "hartig_capacity {}\n",
                "# TYPE hartig_errors_total counter\n",
                "hartig_errors_total{{class=\"client\"}} {}\n",
                "hartig_errors_total{{class=\"server\"}} {}\n",
            ),
            self.capacity.load(Ordering::Relaxed),
            self.client_errors.load(Ordering::Relaxed),
            self.server_errors.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Metrics;

    #[test]
    fn exposes_only_coarse_metrics() {
        let metrics = Metrics::default();
        metrics.set_capacity(8);
        metrics.record_client_error();

        let encoded = metrics.encode();
        assert!(encoded.contains("hartig_capacity 8"));
        assert!(encoded.contains("class=\"client\"} 1"));
        assert!(encoded.contains("class=\"server\"} 0"));
    }
}
