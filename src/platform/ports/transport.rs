//! Transport ports.
//!
//! The IM core sends and receives protocol payloads through this boundary.
//! Native adapters may use WebSocket + QUIC protocol racing via `flare-core`;
//! browser adapters should provide WebSocket-only transport.

use async_trait::async_trait;
use std::collections::HashMap;

use crate::shared::error::Result;

#[derive(Debug, Clone, Default)]
pub struct AuthContext {
    pub user_id: String,
    pub token: String,
    pub tenant_id: Option<String>,
    pub device_id: Option<String>,
    pub metadata: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum OutboundPacketKind {
    Message,
    Event,
    Ack,
    Data,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct OutboundPacket {
    pub id: String,
    pub kind: OutboundPacketKind,
    pub payload: Vec<u8>,
    pub metadata: HashMap<String, Vec<u8>>,
    pub at_least_once: bool,
}

#[derive(Debug, Clone)]
pub enum InboundPacketKind {
    Message,
    Event,
    Ack,
    Data,
    System,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct InboundPacket {
    pub id: String,
    pub kind: InboundPacketKind,
    pub payload: Vec<u8>,
    pub metadata: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportState {
    Disconnected,
    Connecting,
    Connected,
    Ready,
    Reconnecting,
}

#[async_trait]
pub trait TransportPort: Send + Sync {
    async fn connect(&self, auth: AuthContext) -> Result<()>;
    async fn disconnect(&self) -> Result<()>;
    async fn send(&self, packet: OutboundPacket) -> Result<()>;
    async fn state(&self) -> TransportState;
}
