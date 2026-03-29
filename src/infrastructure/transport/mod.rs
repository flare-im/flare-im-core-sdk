//! 网络层（Network）— IO 与逻辑分离，仅负责连接与帧收发。

pub mod http;
pub mod socket;

pub use http::HttpClient;
pub use socket::{SocketHandler, SocketTransport};
