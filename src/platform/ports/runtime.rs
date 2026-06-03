//! Runtime ports.

use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::shared::error::Result;

pub type BoxFutureResult = Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>;

#[async_trait]
pub trait RuntimeClock: Send + Sync {
    fn now_ms(&self) -> u64;
    async fn sleep(&self, duration: Duration);
}

pub trait TaskSpawner: Send + Sync {
    fn spawn(&self, name: &'static str, task: BoxFutureResult);
}
