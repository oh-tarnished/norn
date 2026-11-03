//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One unary RPC by its full name, taking and returning canonical protobuf JSON.
//!
//! A tool's arguments are its RPC's request message as JSON, so nothing here knows
//! any one method: the descriptor set says what each message is, and the call goes
//! through the loopback as encoded bytes. A field the message lacks is refused.

use std::sync::OnceLock;

use bytes::{Buf, BufMut, Bytes};
use http::uri::PathAndQuery;
use norn_protocol::FILE_DESCRIPTOR_SET;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, MethodDescriptor};
use serde_json::Value;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::{Request, Status};
use tpp_grpc::Loopback;

/// The schema, decoded once.
fn pool() -> Result<&'static DescriptorPool, Status> {
    static POOL: OnceLock<Option<DescriptorPool>> = OnceLock::new();
    POOL.get_or_init(|| DescriptorPool::decode(FILE_DESCRIPTOR_SET).ok())
        .as_ref()
        .ok_or_else(|| Status::internal("the embedded schema does not decode"))
}

fn method(service: &str, name: &str) -> Result<MethodDescriptor, Status> {
    let service = pool()?.get_service_by_name(service);
    service
        .and_then(|s| s.methods().find(|m| m.name() == name))
        .ok_or_else(|| Status::internal(format!("{name} is not in the schema")))
}

fn encode(descriptor: MessageDescriptor, args: Value) -> Result<Bytes, Status> {
    let message = DynamicMessage::deserialize(descriptor, args)
        .map_err(|e| Status::invalid_argument(format!("not the request message: {e}")))?;
    Ok(message.encode_to_vec().into())
}

fn decode(descriptor: MessageDescriptor, bytes: Bytes) -> Result<Value, Status> {
    let message = DynamicMessage::decode(descriptor, bytes)
        .map_err(|e| Status::internal(format!("decoding the answer: {e}")))?;
    serde_json::to_value(&message).map_err(|e| Status::internal(format!("encoding JSON: {e}")))
}

/// Calls `service`'s `name` with `args`, through `loopback`.
///
/// # Errors
/// `INVALID_ARGUMENT` for JSON that is not the request; whatever the call returns.
pub(super) async fn call(
    loopback: Loopback,
    service: &str,
    name: &str,
    args: Value,
) -> Result<Value, Status> {
    let descriptor = method(service, name)?;
    let body = encode(descriptor.input(), args)?;
    let path = PathAndQuery::try_from(format!("/{service}/{name}"))
        .map_err(|e| Status::internal(e.to_string()))?;
    let mut client = tonic::client::Grpc::new(loopback);
    client
        .ready()
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let reply = client.unary(Request::new(body), path, Raw).await?;
    decode(descriptor.output(), reply.into_inner())
}

/// Encoded messages, passed through: the message's meaning is [`call`]'s.
#[derive(Clone, Copy)]
struct Raw;

impl Codec for Raw {
    type Encode = Bytes;
    type Decode = Bytes;
    type Encoder = Raw;
    type Decoder = Raw;

    fn encoder(&mut self) -> Raw {
        Raw
    }

    fn decoder(&mut self) -> Raw {
        Raw
    }
}

impl Encoder for Raw {
    type Item = Bytes;
    type Error = Status;

    fn encode(&mut self, item: Bytes, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        dst.put(item);
        Ok(())
    }
}

impl Decoder for Raw {
    type Item = Bytes;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Bytes>, Status> {
        Ok(Some(src.copy_to_bytes(src.remaining())))
    }
}
