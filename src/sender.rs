use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::header;
use serde_json::json;

use crate::authenticator::Authenticator;

async fn send(client: &reqwest::Client, lure_in: &url::Url, target: &url::Url) -> Result<()> {
    let json = json!({
        "content": target,
    });

    client
        .post(lure_in.to_string())
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
    ogp_endpoint: &url::Url,
    lure_in: &url::Url,
    interval: &Duration,
    auth: &Authenticator,
) {
    let mut interval = tokio::time::interval(*interval);

    // Wait Web Server
    tokio::time::sleep(Duration::from_secs(5)).await;

    loop {
        let _ = interval.tick().await;

        let now = Utc::now();
        let ts = now.timestamp_millis();
        let signature = auth.sign(ts);

        let mut target = ogp_endpoint.clone();
        target.set_query(Some(&format!("t={ts}&s={signature:x}")));

        if let Err(e) = send(client, lure_in, &target).await {
            tracing::error!("Failed to send lure message {e}");
        }
    }
}
