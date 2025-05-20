use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use clap::Parser;
use tokio::sync::oneshot;

#[derive(Parser, Debug)]
struct Cli {
    #[clap(env, long, default_value = "0.0.0.0:3000")]
    listen: SocketAddr,

    #[clap(long, env, value_delimiter = ',', default_value = "0.0.0.0")]
    sender_ips: Vec<Ipv4Addr>,

    #[clap(long, env, default_value_t = 1)]
    multiplier: u8,

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
    ogp_endpoint: url::Url,

    #[clap(env, long, default_value = "")]
    report_content: String,

    #[clap(env, long, default_value = "TOP SECRET")]
    hmac_secret: String,

    #[clap(long, env)]
    lure_ins: PathBuf,
}

mod discord;
mod authenticator;
mod collector;
mod conn;
mod conn_initializer;
mod limiter;
mod metrics;
mod metrics_sender;
mod reporter;
mod request;
mod sender;
mod web;

use authenticator::Authenticator;
use collector::Collector;
use metrics::Metrics;
use sender::Targets;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let cli = Cli::parse();

    let (web_tx, web_rx) = oneshot::channel();
    let (sender_tx, sender_rx) = oneshot::channel();

    let lure_ins = Targets::try_new(&cli.lure_ins).unwrap();

    let client = reqwest::ClientBuilder::new()
        .user_agent("UnknownIPReporter/0.1.0")
        .build()
        .unwrap();

    let auth = &*Box::leak(Box::new(Authenticator::new(cli.hmac_secret.as_bytes())));
    let ogp_url = &*Box::leak(Box::new(cli.ogp_endpoint));

    let collector = Collector::new(
        &cli.wellknown_ips,
        &client,
        &cli.report_in,
        &cli.report_content,
    );

    let metrics = Metrics::new();

    // web-worker thread
    tokio::spawn({
        let collector = collector.clone();

        async move {
            let exit_state = web::run(
                cli.listen,
                cli.client_ip_source,
                auth,
                &collector,
                *cli.timeout,
            )
            .await;

            web_tx.send(exit_state).unwrap();
        }
    });

    // metrics (1) thread
    tokio::spawn({
        let collector = collector.clone();
        let report_in = cli.report_in.clone();
        async move { metrics_sender::run(&client, &collector, &report_in, &cli.metrics_interval).await }
    });

    // metrics (2) thread
    tokio::spawn({
        let metrics = metrics.clone();
        let report_in = cli.report_in.clone();

        async move { reporter::run(&cli.metrics_interval, &report_in, metrics).await }
    });


    let (sender, _limiter) =
        conn_initializer::initialize(&cli.sender_ips, cli.multiplier, &ogp_url, auth, metrics)
            .await
            .expect("failed to initialize connection");

    // sender thread
    tokio::spawn({
        async move {
            let exit_state = sender::run(sender, &lure_ins, &cli.measurement_interval).await;
            sender_tx.send(exit_state).unwrap();
        }
    });

    tokio::select! {
        v = web_rx => tracing::error!("Web Error: {:?}", v.unwrap()),
        v = sender_rx => tracing::error!("Sender Error: {:?}", v.unwrap()),
    }
}
