//! Cooperative cancellation at the binding boundary. Cancellation ends the local
//! waiter; it does not undo a remote write or authorize replaying it.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Default)]
struct State {
    next: u64,
    pending: HashMap<u64, (bool, watch::Sender<bool>)>,
}

#[derive(Clone, Default)]
pub struct InvocationRegistry(Arc<Mutex<State>>);

pub struct Invocation {
    registry: InvocationRegistry,
    id: u64,
    cancel: watch::Receiver<bool>,
    cancellable: bool,
}

impl InvocationRegistry {
    pub fn begin(&self, operation: &str) -> Invocation {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.next += 1;
        let id = state.next;
        let (tx, rx) = watch::channel(false);
        // Lifecycle operations temporarily own the engine. Dropping their future
        // could orphan it; they must settle before any new lifecycle is admitted.
        let cancellable = !operation.starts_with("sdk.") && !operation.starts_with("connection.");
        state.pending.insert(id, (cancellable, tx));
        Invocation {
            registry: self.clone(),
            id,
            cancel: rx,
            cancellable,
        }
    }

    pub fn cancel_pending(&self) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut all_cancellable = true;
        for (cancellable, sender) in state.pending.values() {
            if *cancellable {
                sender.send_replace(true);
            } else {
                all_cancellable = false;
            }
        }
        all_cancellable
    }
}

impl Invocation {
    pub fn is_cancellable(&self) -> bool {
        self.cancellable
    }

    pub async fn cancelled(&mut self) {
        loop {
            if *self.cancel.borrow_and_update() {
                return;
            }
            if self.cancel.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Drop for Invocation {
    fn drop(&mut self) {
        self.registry
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_is_scoped_and_does_not_poison_following_calls() {
        let a = InvocationRegistry::default();
        let b = InvocationRegistry::default();
        let mut first = a.begin("message.search_in_conversation");
        let other = b.begin("message.search_in_conversation");
        assert!(a.cancel_pending());
        first.cancelled().await;
        assert!(!*other.cancel.borrow());
        drop(first);
        assert!(!*a.begin("message.search").cancel.borrow());
    }

    #[tokio::test]
    async fn lifecycle_is_not_cancelled_while_it_owns_the_engine() {
        let registry = InvocationRegistry::default();
        let lifecycle = registry.begin("sdk.connect");
        let mut query = registry.begin("message.search");
        assert!(!registry.cancel_pending());
        query.cancelled().await;
        assert!(!*lifecycle.cancel.borrow());
    }
}
