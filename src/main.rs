// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2025 Max Wipfli <mail@maxwipfli.ch>

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

use clap::Parser;
use eyre::Result;
use reqwest::Url;
use tracing::debug;
use tracing_subscriber::EnvFilter;

use aufloes::{client::https::HttpsClient, resolver};

#[derive(Debug, Parser)]
struct Args {
    /// Port to bind UDP server to.
    #[arg(short, long, default_value_t = 53)]
    port: u16,

    /// Be more verbose.
    #[arg(short, long)]
    verbose: bool,

    /// Upstream server URL
    /// Currently, only DNS-over-HTTPS (DoH) upstreams are supported.
    /// Example: https://dnsserver.example.net/dns-query
    #[arg(value_parser = parse_url)]
    server: Url,

    /// Upstream server IP address.
    /// This is required if using DNS-over-HTTPS (DoH) and this resolver is configured as the system resolver.
    #[arg(long = "ip")]
    server_ip: Option<IpAddr>,
}

fn parse_url(s: &str) -> Result<Url, String> {
    let url = Url::parse(s).map_err(|e| e.to_string())?;
    if url.scheme() != "https" {
        return Err("URL scheme is not 'https' (only DNS-over-HTTPS is supported)".to_string());
    }
    Ok(url)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // set up logging
    let env_filter_str = if args.verbose {
        format!("info,{}=debug", env!("CARGO_PKG_NAME").replace("-", "_"))
    } else {
        "info".to_string()
    };
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(env_filter_str))
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    debug!("verbose logging enabled");

    let upstream_client = Arc::new(HttpsClient::new(args.server, args.server_ip)?);

    let localhost = [Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()];
    let bind_addrs = localhost.map(|ip| SocketAddr::new(ip, args.port));

    resolver::run(upstream_client, &bind_addrs).await?;

    Ok(())
}
