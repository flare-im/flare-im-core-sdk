use thiserror::Error;

#[derive(Error, Debug)]
pub enum SdkError {
    #[error("not connected")]
    NotConnected,

    #[error("invalid state: expected {expected}, got {actual}")]
    InvalidState { expected: &'static str, actual: String },

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("token expired")]
    TokenExpired,

    #[error("send failed: {0}")]
    SendFailed(String),

    #[error("sync failed: {0}")]
    SyncFailed(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("server error (code={code}): {message}")]
    Server { code: i32, message: String },

    #[error("command failed: {0}")]
    CommandFailed(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("handler error: {0}")]
    Handler(String),

    #[error("codec error: {0}")]
    Codec(String),

    #[error("ack timeout for message {message_id}")]
    AckTimeout { message_id: String },

    #[error("proto decode: {0}")]
    ProtoDecode(#[from] prost::DecodeError),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SdkError>;
