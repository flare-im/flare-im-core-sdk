use std::sync::Arc;

use tokio::sync::{Mutex, Notify};

use crate::client::config::SdkConfig;
use crate::infrastructure::protocol::{Codec, PacketSender, ProtobufCodec};
use crate::shared::error::{ErrorCode, FlareError, Result};

pub struct SocketTransport {
    sender: Arc<PacketSender>,
}

impl SocketTransport {
    pub fn new(config: SdkConfig) -> Self {
        Self::with_codec(config, Arc::new(ProtobufCodec))
    }

    pub fn with_codec(_config: SdkConfig, codec: Arc<dyn Codec>) -> Self {
        let client: Arc<Mutex<Option<()>>> = Arc::new(Mutex::new(None));
        let sender = Arc::new(PacketSender::new(client, codec));
        Self { sender }
    }

    pub fn sender(&self) -> &Arc<PacketSender> {
        &self.sender
    }

    pub async fn connect(
        &self,
        _user_id: &str,
        _token: &str,
        _listener: Arc<SocketHandler>,
        _ready_notify: Arc<Notify>,
    ) -> Result<()> {
        Err(wasm_transport_unavailable("connect"))
    }

    pub async fn disconnect(&self) -> Result<()> {
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        false
    }
}

pub struct SocketHandler;

impl SocketHandler {
    pub fn new(
        _dispatcher: Arc<crate::core::Dispatcher>,
        _codec: Arc<dyn Codec>,
        _ready_notify: Arc<Notify>,
    ) -> Self {
        Self
    }
}

fn wasm_transport_unavailable(operation: &str) -> FlareError {
    FlareError::localized(
        ErrorCode::OperationNotSupported,
        format!("{operation} requires a Web runtime transport adapter"),
    )
}
