#[cfg(not(target_arch = "wasm32"))]
pub mod socket_client;
#[cfg(not(target_arch = "wasm32"))]
pub mod socket_handler;
#[cfg(target_arch = "wasm32")]
mod wasm_stub;

#[cfg(not(target_arch = "wasm32"))]
pub use socket_client::SocketTransport;
#[cfg(not(target_arch = "wasm32"))]
pub use socket_handler::SocketHandler;
#[cfg(target_arch = "wasm32")]
pub use wasm_stub::{SocketHandler, SocketTransport};
