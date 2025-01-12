use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use bytes::BytesMut;
use eyre::Result;
use tokio::net::UdpSocket;
use tracing::{info, warn};

use crate::{client::Client, proto::txid_from_binary_message};

pub async fn run(upstream: Arc<dyn Client>, bind_addrs: &[SocketAddr]) -> Result<()> {
    let socket = Arc::new(UdpSocket::bind(bind_addrs).await?);
    socket_handler(socket, upstream).await
}

static REQUEST_ID: AtomicU64 = AtomicU64::new(0);

struct Request {
    id: u64,
    stamp: Instant,
    peer: SocketAddr,
    data: BytesMut,
}

async fn socket_handler(socket: Arc<UdpSocket>, upstream: Arc<dyn Client>) -> Result<()> {
    loop {
        let mut buffer = BytesMut::zeroed(1024);
        let result = socket.recv_from(&mut buffer).await;
        let stamp = Instant::now();
        if let Err(err) = result {
            warn!("error in recv_from(): {}", err);
            continue;
        }
        let (n, peer) = result.unwrap();
        let data = buffer.split_to(n);

        let request = Request {
            id: REQUEST_ID.fetch_add(1, Ordering::SeqCst),
            stamp,
            peer,
            data,
        };

        tokio::spawn(request_handler(socket.clone(), upstream.clone(), request));
    }
}

async fn request_handler(
    socket: Arc<UdpSocket>,
    upstream_client: Arc<dyn Client>,
    request: Request,
) {
    info!(
        "request #{}: received {} bytes from downstream peer ({})",
        request.id,
        request.data.len(),
        request.peer
    );
    // store transaction ID for later
    let txid = txid_from_binary_message(&request.data);

    let result = upstream_client.resolve_raw(request.data).await;
    if let Err(err) = result {
        warn!(
            "request #{}: error in upstream request: {}",
            request.id, err
        );
        return;
    }

    // restore transaction ID
    let mut data = result.unwrap().to_vec();
    data[0..2].copy_from_slice(&txid.to_be_bytes());

    info!(
        "request #{}: received {} bytes from upstream server",
        request.id,
        data.len()
    );

    let result = socket.send_to(&data, request.peer).await;
    if let Err(err) = result {
        warn!("request #{}: error in send_to(): {}", request.id, err);
        return;
    }
    let n = result.unwrap();
    assert_eq!(n, data.len());

    info!(
        "request #{}: finished in {} ms",
        request.id,
        request.stamp.elapsed().as_millis()
    );
}
