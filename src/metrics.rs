use std::sync::Arc;

use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Gauge {
    best_ms: i64,
    total_ms: i64,
    worst_ms: i64,
    count: i64,
}

impl Gauge {
    fn new() -> Self {
        Self {
            best_ms: i64::MAX,
            total_ms: 0,
            worst_ms: i64::MIN,
            count: 0,
        }
    }

    pub fn append(&mut self, time_ms: i64) {
        self.best_ms = self.best_ms.min(time_ms);
        self.worst_ms = self.worst_ms.max(time_ms);
        self.total_ms += time_ms;
        self.count += 1;
    }

    pub fn avg_ms(&self) -> i64 {
        self.total_ms / self.count
    }

    pub fn best_ms(&self) -> i64 {
        self.best_ms
    }

    pub fn worst_ms(&self) -> i64 {
        self.worst_ms
    }

    pub fn count(&self) -> i64 {
        self.count
    }
}

#[derive(Debug)]
struct MetricsInner {
    gauge: Mutex<Gauge>,
}

impl MetricsInner {
    fn new() -> Self {
        Self {
            gauge: Mutex::new(Gauge::new()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(MetricsInner::new()),
        }
    }

    pub async fn append(&self, time_ms: i64) {
        self.inner.gauge.lock().await.append(time_ms);
    }

    pub async fn read(&self) -> Gauge {
        self.inner.gauge.lock().await.clone()
    }
}
