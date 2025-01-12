// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2025 Max Wipfli <mail@maxwipfli.ch>

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use bytes::BytesMut;
use eyre::{eyre, Result};
use reqwest::{header, redirect::Policy, Url};
use tracing::debug;

use crate::{client::Client, proto::txid_from_binary_message};

/// A DNS-over-HTTPS (DoH) client.
///
/// This client attempts to conform to the DNS-over-HTTPS specification as defined in [RFC8484].
///
/// [RFC8484]: https://datatracker.ietf.org/doc/html/rfc8484
pub struct HttpsClient {
    client: reqwest::Client,
    url: reqwest::Url,
}

impl HttpsClient {
    const CONTENT_TYPE_DNS_MESSAGE: &'static str = "application/dns-message";

    /// Create a new `HttpsClient` with the given server URL.
    /// For bootstrap purposes, the IP address of the server can be provided.
    pub fn new(url: Url, ip: Option<IpAddr>) -> Result<Self> {
        if url.scheme() != "https" {
            return Err(eyre!(
                "DohClient cannot be constructed with URL of scheme '{}' (expected 'https')",
                url.scheme()
            ));
        }
        let Some(host) = url.host_str() else {
            return Err(eyre!(
                "DohClient cannot be costructed with URL that doesn't specify host"
            ));
        };

        let mut client_builder = reqwest::Client::builder()
            // Do not follow redirects.
            .redirect(Policy::none())
            // Time out requests after 10 seconds.
            // TODO: Tune this and make it configurable.
            .timeout(Duration::from_secs(10))
            // Do not use plain HTTP.
            .https_only(true)
            // Prefer HTTP/2.
            .http2_prior_knowledge();

        if let Some(ip) = ip {
            // Resolve server's hostname out-of-band.
            // This may be required to avoid bootstrap issues if this is the configured system resolver.
            debug!(
                "HttpsClient: resolving '{}' to {} to bootstrap DNS",
                host, ip
            );
            client_builder = client_builder.resolve(host, SocketAddr::new(ip, 0));
        }

        let client = client_builder.build()?;

        Ok(Self { client, url })
    }
}

#[async_trait::async_trait]
impl Client for HttpsClient {
    async fn resolve_raw(&self, mut data: BytesMut) -> Result<BytesMut> {
        let txid = txid_from_binary_message(&data);

        // "In order to maximize HTTP cache friendliness, DoH clients [...]
        // SHOULD use a DNS ID of 0 in every DNS request."
        // RFC 8484, Section 4.1
        data[0] = 0;
        data[1] = 0;

        let response = self
            .client
            .post(self.url.clone())
            .header(header::ACCEPT, Self::CONTENT_TYPE_DNS_MESSAGE)
            .header(header::CONTENT_TYPE, Self::CONTENT_TYPE_DNS_MESSAGE)
            .body(data.freeze())
            .send()
            .await?;

        // TODO: Perform better error handling, potentially using retries etc.
        if !response.status().is_success() {
            return Err(eyre!(
                "DohClient: server returned non-success status: {}",
                response.status()
            ));
        }

        let mut data = BytesMut::from(response.bytes().await?);
        // restore transaction ID
        data[0..2].copy_from_slice(&txid.to_be_bytes());
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use std::{env, sync::Arc};

    use super::*;
    use crate::client::client::tests as client_tests;

    static TEST_DOH_SERVER_URL: &str = "https://dns10.quad9.net/dns-query";

    fn build_client() -> Arc<HttpsClient> {
        let server_url = env::var("FERRITE_TEST_DOH_SERVER_URL")
            .unwrap_or(TEST_DOH_SERVER_URL.to_string())
            .parse()
            .unwrap();
        // For testing purposes, we don't need to resolve the server's hostname, as we can rely on the system resolver.
        let ip = None;
        let client = HttpsClient::new(server_url, ip).unwrap();
        Arc::new(client)
    }

    #[tokio::test]
    async fn basic_a() {
        let client = build_client();
        client_tests::basic_a(client).await;
    }

    #[tokio::test]
    async fn basic_aaaa() {
        let client = build_client();
        client_tests::basic_aaaa(client).await;
    }
}
