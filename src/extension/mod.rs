//! SDK extension installation.
//!
//! Business SDKs such as `flare-social-sdk` should integrate through this
//! boundary: register sync tasks, interceptors, capability plugins, profile
//! providers, or notification handlers without changing the IM core API.

use std::sync::Arc;

use crate::core::SyncTask;
use crate::shared::error::Result;

pub mod capability;
pub mod middleware;

use capability::SdkCapabilityPlugin;
use middleware::{EventInterceptor, MessageInterceptor};

#[derive(Default)]
pub struct ExtensionRegistry {
    pub sync_tasks: Vec<Arc<dyn SyncTask>>,
    pub message_interceptors: Vec<Arc<dyn MessageInterceptor>>,
    pub event_interceptors: Vec<Arc<dyn EventInterceptor>>,
    pub capability_plugins: Vec<Arc<dyn SdkCapabilityPlugin>>,
}

impl ExtensionRegistry {
    pub fn add_sync_task(&mut self, task: Arc<dyn SyncTask>) {
        self.sync_tasks.push(task);
    }

    pub fn add_message_interceptor(&mut self, interceptor: Arc<dyn MessageInterceptor>) {
        self.message_interceptors.push(interceptor);
    }

    pub fn add_event_interceptor(&mut self, interceptor: Arc<dyn EventInterceptor>) {
        self.event_interceptors.push(interceptor);
    }

    pub fn add_capability_plugin(&mut self, plugin: Arc<dyn SdkCapabilityPlugin>) {
        self.capability_plugins.push(plugin);
    }
}

pub trait SdkExtension: Send + Sync {
    fn namespace(&self) -> &str;
    fn install(&self, registry: &mut ExtensionRegistry) -> Result<()>;
}
