// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2025 Max Wipfli <mail@maxwipfli.ch>

use bytes::BytesMut;
use eyre::Result;

#[async_trait::async_trait]
pub trait Client: Send + Sync {
    async fn resolve_raw(&self, data: BytesMut) -> Result<BytesMut>;
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;

    use bytes::Bytes;
    use hickory_proto::{
        op::{Message, Query},
        rr::RecordType,
        serialize::binary::BinDecodable,
    };

    use super::*;

    fn build_request(name: &str, query_type: RecordType) -> BytesMut {
        let mut request = Message::new();
        request.add_query(Query::query(name.parse().unwrap(), query_type));
        request.set_recursion_desired(true);
        let data = request.to_vec().unwrap();
        Bytes::from(data).into()
    }

    pub async fn basic_a(client: Arc<dyn Client>) {
        let request_bytes = build_request("www.example.com", RecordType::A);
        let response_bytes = client.resolve_raw(request_bytes.into()).await.unwrap();
        let response = Message::from_bytes(&response_bytes).unwrap();

        assert_eq!(response.answer_count(), 1);
        assert_eq!(response.answers()[0].record_type(), RecordType::A);
    }

    pub async fn basic_aaaa(client: Arc<dyn Client>) {
        let request_bytes = build_request("www.example.com", RecordType::AAAA);
        let response_bytes = client.resolve_raw(request_bytes.into()).await.unwrap();
        let response = Message::from_bytes(&response_bytes).unwrap();

        assert_eq!(response.answer_count(), 1);
        assert_eq!(response.answers()[0].record_type(), RecordType::AAAA);
    }
}
