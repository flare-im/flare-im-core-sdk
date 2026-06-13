//! Small lifecycle helper for binding-owned background tasks.
//!
//! Binding layers often need one active forwarding task per SDK session
//! (for example EventBus -> JS/WebView).  Replacing the task must cancel the
//! old one first, otherwise logout/switch-account can leave stale callbacks
//! alive outside the core SDK.

use std::sync::{Arc, Mutex};

type CancelFn = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone, Default)]
pub struct SessionTaskSlot {
    cancel: Arc<Mutex<Option<CancelFn>>>,
}

impl SessionTaskSlot {
    pub fn replace(&self, cancel: impl FnOnce() + Send + 'static) {
        self.clear();
        let mut guard = self.cancel.lock().expect("session task slot poisoned");
        *guard = Some(Box::new(cancel));
    }

    pub fn clear(&self) {
        let cancel = {
            let mut guard = self.cancel.lock().expect("session task slot poisoned");
            guard.take()
        };
        if let Some(cancel) = cancel {
            cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionTaskSlot;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn replace_cancels_previous_task() {
        let slot = SessionTaskSlot::default();
        let cancelled = Arc::new(AtomicUsize::new(0));

        let first = cancelled.clone();
        slot.replace(move || {
            first.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(cancelled.load(Ordering::SeqCst), 0);

        let second = cancelled.clone();
        slot.replace(move || {
            second.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(cancelled.load(Ordering::SeqCst), 1);

        slot.clear();
        assert_eq!(cancelled.load(Ordering::SeqCst), 2);
    }
}
