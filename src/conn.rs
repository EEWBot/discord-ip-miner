use std::net::{SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result as AHResult};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use h2::client::{Connection, ResponseFuture, SendRequest};
use http::{
    Request, StatusCode,
    header::{CONTENT_TYPE, HOST, HeaderMap, USER_AGENT},
    method::Method,
};
use serde_json::json;
use tokio::{
    net::{TcpSocket, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{RootCertStore, pki_types::ServerName},
};

use crate::authenticator::Authenticator;
use crate::limiter::{Limiter, Status};
use crate::metrics::Metrics;
use crate::request::JobReceiver;
use crate::discord::Ratelimit;

const ALPN_H2: &str = "h2";
const HTTP2_SETTINGS_MAX_CONCURRENT_STREAMS: usize = 98;
const CLOUDFLARE_HTTP2_REQUEST_LIMIT: usize = 9990;

async fn setup_connection(
    from: SocketAddrV4,
    to: SocketAddrV4,
) -> AHResult<(SendRequest<Bytes>, Connection<TlsStream<TcpStream>>)> {
    let tls_client_config = Arc::new({
        let root_store = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let mut c = tokio_rustls::rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        c.alpn_protocols.push(ALPN_H2.as_bytes().to_owned());

        c
    });

    let socket = TcpSocket::new_v4().unwrap();

    socket
        .bind(SocketAddr::V4(from))
        .context("Failed to bind local address")?;

    let tcp_stream = socket
        .connect(SocketAddr::V4(to))
        .await
        .context("Failed to establish TCP connection to discord.com")?;

    let dns_name = ServerName::try_from("discord.com").unwrap();

    let tls = TlsConnector::from(tls_client_config)
        .connect(dns_name, tcp_stream)
        .await?;

    {
        let (_, session) = tls.get_ref();

        let negotiated = session.alpn_protocol();
        let reference = Some(ALPN_H2.as_bytes());

        anyhow::ensure!(negotiated == reference, "Negotiated protocol is not HTTP/2");
    }

    Ok(h2::client::handshake(tls).await?)
}

async fn response_handling(
    name: &str,
    request: crate::request::Request,
    response: ResponseFuture,
    permit: OwnedSemaphorePermit,
    limiter: &'static Limiter,
    metrics: Metrics,
    send_t: DateTime<Utc>,
) -> AHResult<()> {
    let mut response = match response.await {
        Ok(v) => v,
        Err(e) => {
            return Err(e).context("Got error related to connection. DROPPED!");
        }
    };

    match response.status() {
        status_code if status_code.is_success() => {
            tracing::debug!("{name} OK");
        }

        StatusCode::NOT_FOUND => {
            limiter.tell_notfound(&request.target);
            tracing::warn!("{name} 404 detected! Canceled.");
        }

        StatusCode::TOO_MANY_REQUESTS => {
            let body = response.body_mut().data().await;

            let ratelimit = body.map(|body_result| {
                body_result.map(|body| serde_json::from_slice::<Ratelimit>(&body))
            });

            let retry_after = match ratelimit {
                Some(Ok(Ok(Ratelimit { retry_after }))) => retry_after,
                _ => 600.0f32,
            };

            // The limiter may have a longer timeout.
            let _ = limiter.tell_ratelimit(&request.target, retry_after);

            tracing::warn!("{name} Ratelimit Configured! (DROPPED)",);
        }

        status_code if status_code.is_client_error() => {
            tracing::warn!(
                "{name} {} Occured. Maybe invalid request. Canceled.",
                status_code
            );
        }

        status_code if status_code.is_server_error() => {
            tracing::warn!(
                "{name} {} Occured. Maybe server error. (DROPPED)",
                status_code
            );
        }

        status_code => {
            tracing::warn!("{name} Unknown StatusCode {}", status_code);
        }
    }

    drop(permit);

    let rtt = Utc::now() - send_t;
    metrics.append(rtt.num_milliseconds()).await;

    Ok(())
}

pub async fn sender(
    name: &'static str,
    from: SocketAddrV4,
    to: SocketAddrV4,
    request_rx: JobReceiver,
    ogp_url: &'static url::Url,
    limiter: &'static Limiter,
    auth: &'static Authenticator,
    metrics: Metrics,
) -> AHResult<()> {
    let (mut client, mut connection) = setup_connection(from, to)
        .await
        .context("Failed to connect to discord.com")?;

    let mut ping_pong = connection.ping_pong().unwrap();

    tracing::info!("{name} Connection established!");

    tokio::spawn(async move {
        // The error handled by request sender and response handler.
        connection.await.expect("Connection Failed");
    });

    let semaphroe = Arc::new(Semaphore::new(HTTP2_SETTINGS_MAX_CONCURRENT_STREAMS));

    let mut request_count = 0;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
    headers.insert(USER_AGENT, "WebhookSender/0.1.0".parse().unwrap());
    headers.insert(HOST, "discord.com".parse().unwrap());

    loop {
        let permit = semaphroe.clone().acquire_owned().await.unwrap();
        let last_request = request_count + 1 >= CLOUDFLARE_HTTP2_REQUEST_LIMIT;

        tokio::select! {
            request = request_rx.recv() => {
                let request = request.unwrap();
                match limiter.current(&request) {
                    Status::Ratelimited(_retry_after) => {
                        tracing::warn!("{name} Ratelimited! Cacnceled.");
                        continue;
                    },
                    Status::Known404 => {
                        tracing::warn!("{name} Known 404 target detected. Cacnceled.");
                        continue;
                    },
                    Status::Pass => (),
                }


                let mut target_uri = request.target.clone();

                // Copy query string w/o "wait"
                let mut target_uri_query: Vec<(String, String)> = target_uri.query_pairs().filter_map(|(k, v)|
                    if k == "wait" {
                        None
                    } else {
                        Some((k.to_string(), v.to_string()))
                    }
                ).collect();

                // Add wait=true
                target_uri_query.push(("wait".to_string(), "true".to_string()));

                // Write-back to target
                target_uri.query_pairs_mut().clear().extend_pairs(target_uri_query.iter());

                let mut h2_header = Request::builder().method(Method::POST).uri(target_uri.as_str()).body(()).unwrap();

                *h2_header.headers_mut() = headers.clone();

                let send_t = Utc::now();

                let h2_body = {
                    let mut ogp_url = ogp_url.to_owned();

                    let ts = send_t.timestamp_millis();
                    let signature = auth.sign(ts);

                    ogp_url.set_query(Some(&format!("t={ts}&s={signature:x}")));

                    Bytes::from(
                        json!({
                            "content": ogp_url.to_string(),
                        })
                        .to_string()
                        .into_bytes(),
                    )
                };

                request_count += 1;

                let (response, mut respond) = match client.send_request(h2_header, false) {
                    Ok(v) => v,
                    Err(e) => {
                        return Err(e).context("Failed to send Request Header, DROPPED!");
                    },
                };

                respond.reserve_capacity(h2_body.len());

                if let Err(e) = respond.send_data(h2_body, true) {
                    return Err(e).context("Failed to send Request Body, DROPPED!");
                };

                tokio::spawn({
                    let metrics = metrics.clone();
                    async move {
                        response_handling(name, request, response, permit, limiter, metrics, send_t).await
                    }
                });

                if last_request {
                    tracing::info!("{name} Reached to cloudflare HTTP/2 limit. Connection will be closed.");
                    return Ok(());
                }
            },
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                tracing::debug!("{name} ping");
                let ping = h2::Ping::opaque();

                ping_pong.ping(ping).await.context("Failed to send ping")?;
            }
        }
    }
}

pub async fn sender_loop(
    name: &'static str,
    from: SocketAddrV4,
    to: SocketAddrV4,
    request_rx: JobReceiver,
    ogp_url: &'static url::Url,
    limiter: &'static Limiter,
    auth: &'static Authenticator,
    metrics: Metrics,
) -> ! {
    loop {
        match sender(
            name,
            from,
            to,
            request_rx.clone(),
            ogp_url,
            limiter,
            auth,
            metrics.clone(),
        )
        .await
        {
            Ok(()) => tracing::info!("{name} Sender is closed normally, restarting..."),
            Err(e) => tracing::info!("{name} Sender is closed unexpectedly {e:?}, restarting..."),
        }
    }
}
