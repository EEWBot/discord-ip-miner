use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use axum_client_ip::{ClientIp, ClientIpSource};
use chrono::{DateTime, Utc, serde::ts_milliseconds};
use moka::sync::{Cache, CacheBuilder};
use serde::Deserialize;
use tokio::net::TcpListener;

use crate::authenticator::{Authenticator, Sha1Bytes};
use crate::collector::Collector;

#[derive(Debug, Clone)]
struct AppState {
    timeout: Duration,
    auth: Authenticator,
    collector: Collector,
    seen: Cache<i64, ()>,
}

#[derive(Debug, Deserialize)]
struct Params {
    #[serde(rename = "t", with = "ts_milliseconds")]
    ts: DateTime<Utc>,

    #[serde(rename = "s", with = "hex")]
    signature: [u8; 20],
}

fn ogp_resp(ts: DateTime<Utc>) -> Html<String> {
    Html(include_str!("../assets/ogp.html").replace("{TIME}", &ts.to_rfc2822()))
}

async fn ogp(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    Query(query): Query<Params>,
) -> Html<String> {
    let signature: Sha1Bytes = query.signature.into();

    if !app.auth.verify(query.ts.timestamp_millis(), &signature) {
        tracing::warn!("EInvalidHMAC {ip}");
        return ogp_resp(query.ts);
    }

    let dt = Utc::now().signed_duration_since(query.ts);

    if dt.as_seconds_f32() < 0.0 {
        tracing::warn!("ETimePaladox {ip}");
        return ogp_resp(query.ts);
    }

    if dt.as_seconds_f32() > app.timeout.as_secs_f32() {
        tracing::warn!("ETimeout {ip}");
        return ogp_resp(query.ts);
    }

    let entry = app.seen.entry(query.ts.timestamp()).or_insert(());

    if !entry.is_fresh() {
        tracing::warn!("ESeen {ip}");
        return ogp_resp(query.ts);
    }

    app.collector
        .tell(ip, dt.num_milliseconds().cast_unsigned())
        .await;

    ogp_resp(query.ts)
}

async fn root() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

pub async fn run(
    listen: SocketAddr,
    client_ip_source: ClientIpSource,
    auth: &Authenticator,
    collector: &Collector,
    timeout: Duration,
) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    let collector = collector.to_owned();
    let auth = auth.to_owned();

    let seen = CacheBuilder::new(64).time_to_live(timeout * 2).build();

    let app = Router::new()
        .route("/", get(root))
        .route("/ogp", get(ogp))
        .with_state(AppState {
            timeout,
            auth,
            collector,
            seen,
        })
        .layer(client_ip_source.into_extension());

    tracing::info!("listening on {}", listener.local_addr().unwrap());

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
