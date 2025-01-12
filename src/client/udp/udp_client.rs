// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2025 Max Wipfli <mail@maxwipfli.ch>

use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use bytes::BytesMut;
use eyre::{eyre, Result};
use rand::Rng;
use tokio::{
    net::UdpSocket,
    sync::{oneshot, Mutex},
    time::timeout,
};
use tracing::{debug, warn};

use crate::{client::Client, proto::txid_from_binary_message};

pub struct UdpClient {
    inner: Arc<UdpClientInner>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl UdpClient {
    pub async fn new(server_addr: SocketAddr) -> Result<Self> {
        let local_addr = match server_addr {
            SocketAddr::V4(_) => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
            SocketAddr::V6(_) => SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0),
        };
        let socket = UdpSocket::bind(local_addr).await?;
        socket.connect(server_addr).await?;

        let inner = Arc::new(UdpClientInner {
            socket,
            pending: Mutex::default(),
        });
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(Self::receive_task(inner.clone(), shutdown_rx));

        Ok(Self {
            inner,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Finds an unused TXID, inserts it into the pending map.
    /// Returns the TXID and a receiver for the response.
    async fn new_pending_request(&self) -> (u16, oneshot::Receiver<Result<BytesMut>>) {
        let (sender, receiver) = oneshot::channel();
        let mut pending = self.inner.pending.lock().await;
        let mut rng = rand::thread_rng();

        let txid = loop {
            let txid = rng.gen();
            // Check TXID is unused.
            if !pending.contains_key(&txid) {
                break txid;
            }
        };

        let prev_value = pending.insert(txid, sender);
        assert!(prev_value.is_none(), "TXID collision");
        (txid, receiver)
    }

    /// Task that receives data from the socket and resolves pending requests.
    /// Shuts down when the `shutdown_rx` channel is closed.
    async fn receive_task(inner: Arc<UdpClientInner>, mut shutdown_rx: oneshot::Receiver<()>) {
        loop {
            let mut buffer = BytesMut::zeroed(1024);
            let result = tokio::select! {
                _ = &mut shutdown_rx => {
                    debug!("UdpClient: shutting down receive task");
                    break;
                },
                result = inner.socket.recv(&mut buffer) => result
            };

            if let Err(err) = result {
                warn!("UdpClient: error receiving data: {:?}", err);
                continue;
            }
            let n = result.unwrap();
            let data = buffer.split_to(n);

            let txid = txid_from_binary_message(&data);

            let mut pending = inner.pending.lock().await;
            let Some(sender) = pending.remove(&txid) else {
                // Ignore responses that we didn't send a request for.
                continue;
            };
            let _ = sender.send(Ok(data));
        }
    }
}

impl Drop for UdpClient {
    fn drop(&mut self) {
        debug!("UdpClient: signalling shutdown to receive task");
        let _ = self.shutdown_tx.take().unwrap().send(());
    }
}

#[async_trait::async_trait]
impl Client for UdpClient {
    async fn resolve_raw(&self, mut data: BytesMut) -> Result<BytesMut> {
        // Insert pending request before sending.
        // This avoids a data race where the server could theoretically respond before the request is inserted.
        // Another concurrent request could then drop the response as there is no pending request with that ID.
        let (txid, receiver) = self.new_pending_request().await;

        // set TXID
        // FIXME: Factor this into a separate function in a "protocol" module.
        data[0..2].copy_from_slice(&txid.to_be_bytes());

        let timeout_duration = Duration::from_secs(5);

        // send request
        self.inner.socket.send(&data).await?;

        match timeout(timeout_duration, receiver).await {
            Ok(Ok(response)) => Ok(response?),
            Ok(Err(err)) => {
                warn!("UdpClient: error receiving response: {:?}", err);
                let mut pending = self.inner.pending.lock().await;
                let _ = pending.remove(&txid);
                Err(eyre!("error receiving response"))
            }
            Err(_) => {
                warn!("UdpClient: timeout receiving response");
                let mut pending = self.inner.pending.lock().await;
                let _ = pending.remove(&txid);
                Err(eyre!("timeout receiving response"))
            }
        }
    }
}

struct UdpClientInner {
    socket: UdpSocket,
    pending: Mutex<HashMap<u16, oneshot::Sender<Result<BytesMut>>>>,
}

#[cfg(test)]
mod tests {
    use std::{env, sync::Arc};

    use super::*;
    use crate::client::client::tests as client_tests;

    static TEST_UDP_SERVER_ADDR: &str = "9.9.9.10:53";

    async fn build_client() -> Arc<UdpClient> {
        let server_addr = env::var("FERRITE_TEST_UDP_SERVER_ADDR")
            .unwrap_or(TEST_UDP_SERVER_ADDR.to_string())
            .parse()
            .unwrap();
        let client = UdpClient::new(server_addr).await.unwrap();
        Arc::new(client)
    }

    #[tokio::test]
    async fn basic_a() {
        let client = build_client().await;
        client_tests::basic_a(client).await;
    }

    #[tokio::test]
    async fn basic_aaaa() {
        let client = build_client().await;
        client_tests::basic_aaaa(client).await;
    }
}
