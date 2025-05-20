use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header;
use serde_json::json;

use crate::metrics::{Gauge, Metrics};

async fn report(client: &reqwest::Client, report_in: &url::Url, gauge: &Gauge) -> Result<()> {
    let json = json!({
        "embeds": [{
            "title": "Webhook Benchmark Metrics",
            "color": 0x008000,
            "fields": [
                {
                    "name": "Count",
                    "value": format!("{} times", gauge.count()),
                },
                {
                    "name": "Best",
                    "value": format!("{}ms", gauge.best_ms()),
                    "inline": true,
                },
                {
                    "name": "Average",
                    "value": format!("{}ms", gauge.avg_ms()),
                    "inline": true,
                },
                {
                    "name": "Worst",
                    "value": format!("{}ms", gauge.worst_ms()),
                    "inline": true,
                },
            ]
        }]
    });

    client
        .post(report_in.to_string())
        .header(header::CONTENT_TYPE, "application/json")
        .body(json.to_string())
        .send()
        .await
        .context("Connection Error")?
        .error_for_status()
        .context("HTTP Error")?;

    Ok(())
}

pub async fn run(report_interval: &Duration, report_in: &url::Url, metrics: Metrics) {
    let client = reqwest::Client::builder()
        .user_agent("BenchmarkResultReporter/0.1.0")
        .build()
        .unwrap();

    tokio::time::sleep(Duration::from_secs(60)).await;

    let mut interval = tokio::time::interval(*report_interval);

    loop {
        let _ = interval.tick().await;
        let gauge = metrics.read().await;

        if let Err(e) = report(&client, report_in, &gauge).await {
            tracing::error!("Failed to send new metrics report {e}");
        }
    }
}
