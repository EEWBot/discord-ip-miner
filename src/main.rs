use std::net::{IpAddr, SocketAddr};

use clap::Parser;
use tokio::sync::oneshot;

#[derive(Parser, Debug)]
struct Cli {
    #[clap(env, long, default_value = "0.0.0.0:3000")]
    listen: SocketAddr,

    #[clap(env, long, value_delimiter = ',', required = false)]
    wellknown_ips: Vec<IpAddr>,

    #[clap(env, long, default_value = "60s")]
    measurement_interval: humantime::Duration,

    #[clap(env, long, default_value = "10s")]
    timeout: humantime::Duration,

    #[clap(env, long, default_value = "8h")]
    metrics_interval: humantime::Duration,

    /// See: https://docs.rs/axum-client-ip/1.0.0/axum_client_ip/index.html#configurable-vs-specific-extractors
    #[clap(env, long, default_value = "ConnectInfo")]
    client_ip_source: axum_client_ip::ClientIpSource,

    #[clap(env, long)]
    report_in: url::Url,

    #[clap(env, long)]
    lure_in: url::Url,

    #[clap(env, long)]
    ogp_endpoint: url::Url,

    #[clap(env, long, default_value = "")]
    report_content: String,

    #[clap(env, long, default_value = "TOP SECRET")]
    hmac_secret: String,
}

mod authenticator;
mod collector;
mod metrics_sender;
mod sender;
mod web;

use authenticator::Authenticator;
use collector::Collector;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let cli = Cli::parse();

    let (web_tx, web_rx) = oneshot::channel();
    let (sender_tx, sender_rx) = oneshot::channel();

    let client = reqwest::ClientBuilder::new()
        .user_agent("UnknownIPReporter/0.1.0")
        .build()
        .unwrap();

    let auth = Authenticator::new(cli.hmac_secret.as_bytes());

    let collector = Collector::new(
        &cli.wellknown_ips,
        &client,
        &cli.report_in,
        &cli.report_content,
    );

    // web-worker thread
    tokio::spawn({
        let auth = auth.clone();
        let collector = collector.clone();

        async move {
            let exit_state = web::run(
                cli.listen,
                cli.client_ip_source,
                &auth,
                &collector,
                *cli.timeout,
            )
            .await;

            web_tx.send(exit_state).unwrap();
        }
    });

    // metrics thread
    tokio::spawn({
        let collector = collector.clone();

        async move {
            metrics_sender::run(
                &client,
                &collector,
                &cli.report_in,
                &cli.metrics_interval,
            ).await
        }
    });

    // sender thread
    tokio::spawn({
        let auth = auth.clone();

        let client = reqwest::ClientBuilder::new()
            .user_agent("OGPClock/0.1.0")
            .build()
            .unwrap();

        async move {
            let exit_state = sender::run(
                &client,
                &cli.ogp_endpoint,
                &cli.lure_in,
                &cli.measurement_interval,
                &auth,
            )
            .await;
            sender_tx.send(exit_state).unwrap();
        }
    });


    tokio::select! {
        v = web_rx => tracing::error!("Web Error: {:?}", v.unwrap()),
        v = sender_rx => tracing::error!("Sender Error: {:?}", v.unwrap()),
    }
}
