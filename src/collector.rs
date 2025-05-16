use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::header;
use serde_json::json;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Gauge {
    latency_ms_worst: u64,
    latency_ms_best: u64,
    latency_ms_total: u64,
    count: u64,
}

impl Gauge {
    fn new() -> Self {
        Self {
            latency_ms_best: u64::MAX,
            latency_ms_worst: u64::MIN,
            latency_ms_total: 0,
            count: 0,
        }
    }

    fn append(&mut self, latency_ms: u64) {
        self.latency_ms_total += latency_ms;
        self.latency_ms_worst = self.latency_ms_worst.max(latency_ms);
        self.latency_ms_best = self.latency_ms_best.min(latency_ms);
        self.count += 1;
    }

    pub fn latency_ms_worst(&self) -> u64 {
        self.latency_ms_worst
    }

    pub fn latency_ms_best(&self) -> u64 {
        self.latency_ms_best
    }

    pub fn latency_ms_avg(&self) -> u64 {
        self.latency_ms_total / self.count
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

#[derive(Debug)]
struct CollectorInner {
    wellknown_ips: HashSet<IpAddr>,
    metrics: Mutex<HashMap<IpAddr, Gauge>>,
    report_in: url::Url,
    report_content: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct Collector {
    inner: Arc<CollectorInner>,
}

impl Collector {
    pub fn new(
        wellknown_ips: &[IpAddr],
        client: &reqwest::Client,
        report_in: &url::Url,
        report_content: &str,
    ) -> Self {
        let wellknown_ips = HashSet::from_iter(wellknown_ips.iter().copied());
        let metrics = Mutex::new(HashMap::new());
        let report_in = report_in.to_owned();
        let report_content = report_content.to_owned();
        let client = client.clone();

        let inner = Arc::new(CollectorInner {
            wellknown_ips,
            metrics,
            report_in,
            report_content,
            client,
        });

        Self { inner }
    }

    async fn report_unknown_ip(&self, ip: IpAddr) -> Result<()> {
        let json = json!({
            "content": self.inner.report_content,
            "embeds": [{
                "title": "New IP Address Detected!",
                "color": 0x800000,
                "fields": [{
                    "name": "New Address",
                    "value": ip.to_string(),
                }]
            }]
        });

        self.inner
            .client
            .post(self.inner.report_in.to_string())
            .header(header::CONTENT_TYPE, "application/json")
            .body(json.to_string())
            .send()
            .await
            .context("Connection Error")?
            .error_for_status()
            .context("HTTP Error")?;

        Ok(())
    }

    pub async fn tell(&self, ip: IpAddr, latency_ms: u64) {
        (*self
            .inner
            .metrics
            .lock()
            .await
            .entry(ip)
            .or_insert(Gauge::new()))
        .append(latency_ms);

        if self.inner.wellknown_ips.contains(&ip) {
            return;
        }

        // UNKNOWN IP IS COMMING!
        tracing::warn!("New IP Detected! {ip}");

        if let Err(e) = self.report_unknown_ip(ip).await {
            tracing::error!("Failed to send new ip report {e}");
        }
    }

    pub async fn metric(&self) -> HashMap<IpAddr, Gauge> {
        self.inner.metrics.lock().await.clone()
    }
}
