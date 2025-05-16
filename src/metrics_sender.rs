use std::time::Duration;
use std::net::IpAddr;
use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::header;
use serde_json::json;

use crate::collector::{Collector, Gauge};

async fn report(
    client: &reqwest::Client,
    report_in: &url::Url,
    metrics: &HashMap<IpAddr, Gauge>
) -> Result<()> {
    let fields: Vec<_> = metrics.iter().map(|(ip, metrics)| {
        let seen = metrics.count();
        let best = metrics.latency_ms_best();
        let avg = metrics.latency_ms_avg();
        let worst = metrics.latency_ms_worst();

        json!({
            "name": ip,
            "value": format!(
                "**seen: {seen} times**\nbest: {best}ms\n**avg: {avg}ms**\nworst: {worst}ms"
            ),
            "inline": true,
        })
    }).collect();

    let json = json!({
        "embeds": [{
            "title": "Metrics Report",
            "color": 0x008000,
            "fields": fields,
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

pub async fn run(
    client: &reqwest::Client,
    collector: &Collector,
    report_in: &url::Url,
    interval: &Duration,
) {
    let mut interval = tokio::time::interval(*interval);

    let _ = interval.tick().await;

    loop {
        let _ = interval.tick().await;
        let metric = collector.metric().await;
        if let Err(e) = report(client, report_in, &metric).await {
            tracing::error!("Failed to send new metrics report {e}");
        }
    }
}
