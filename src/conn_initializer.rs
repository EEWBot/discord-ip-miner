use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};

use anyhow::{Context, Result as AHResult};
use hickory_resolver::{Resolver, config::ResolverConfig, name_server::TokioConnectionProvider};

use crate::limiter::Limiter;
use crate::metrics::Metrics;
use crate::request::JobSender;
use crate::authenticator::Authenticator;

async fn query_discord_ips() -> AHResult<Vec<Ipv4Addr>> {
    let resolver = Resolver::builder_with_config(
        ResolverConfig::default(),
        TokioConnectionProvider::default(),
    )
    .build();

    let mut ips = vec![];
    let response = resolver
        .lookup_ip("discord.com")
        .await
        .context("Failed to resolve discord.com")?;

    ips.extend(response.iter().map(|ip| match ip {
        IpAddr::V4(ip) => ip,
        _ => panic!("WTF!? discord.com provides IPv6 Addr"),
    }));

    tracing::info!("I got {} ips in discord.com! {ips:?}", ips.len());

    Ok(ips)
}

pub async fn initialize(
    sender_ips: &[Ipv4Addr],
    multiplier: u8,
    ogp_url: &'static url::Url,
    authenticator: &'static Authenticator,
    metrics: Metrics,
) -> AHResult<(JobSender, &'static Limiter)> {
    let target_ips = query_discord_ips().await?;

    let target_socks: Vec<_> = target_ips
        .iter()
        .map(|ip| SocketAddrV4::new(*ip, 443))
        .collect();

    let sender_socks: Vec<_> = sender_ips
        .iter()
        .map(|ip| SocketAddrV4::new(*ip, 0))
        .collect();

    let limiter = &*Box::leak(Box::new(Limiter::default()));

    let (tx, rx) = async_channel::unbounded();

    for sock_no in 0..multiplier {
        for from in &sender_socks {
            for to in &target_socks {
                let rx = rx.clone();
                let from = *from;
                let to = *to;

                tokio::spawn({
                    let metrics = metrics.clone();
                    async move {
                        let name = &*format!("C{sock_no} {from}-{to}").leak();
                        crate::conn::sender_loop(
                            name,
                            from,
                            to,
                            rx,
                            ogp_url,
                            limiter,
                            authenticator,
                            metrics,
                        )
                        .await;
                    }
                });
            }
        }
    }

    Ok((tx, limiter))
}
