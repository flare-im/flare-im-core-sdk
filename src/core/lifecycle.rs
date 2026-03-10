use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

/// SDK 全局状态机
///
/// ```text
/// Disconnected → Connecting → Connected → Syncing → Ready
///      ↑              │            │          │        │
///      └──────────────┴────────────┴──────────┴────────┘
///                     (disconnect / error)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SdkState {
    Disconnected = 0,
    Connecting   = 1,
    Connected    = 2,
    Syncing      = 3,
    Ready        = 4,
}

impl SdkState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Connecting,
            2 => Self::Connected,
            3 => Self::Syncing,
            4 => Self::Ready,
            _ => Self::Disconnected,
        }
    }
}

impl fmt::Display for SdkState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Connecting   => write!(f, "Connecting"),
            Self::Connected    => write!(f, "Connected"),
            Self::Syncing      => write!(f, "Syncing"),
            Self::Ready        => write!(f, "Ready"),
        }
    }
}

/// 线程安全状态管理器（CAS 无锁实现）
pub struct StateManager {
    state: AtomicU8,
}

impl StateManager {
    pub fn new() -> Self {
        Self { state: AtomicU8::new(SdkState::Disconnected as u8) }
    }

    pub fn get(&self) -> SdkState {
        SdkState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set(&self, s: SdkState) {
        self.state.store(s as u8, Ordering::Release);
    }

    pub fn transition(&self, expected: SdkState, new: SdkState) -> bool {
        self.state.compare_exchange(expected as u8, new as u8, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    pub fn reset(&self) {
        self.state.store(SdkState::Disconnected as u8, Ordering::Release);
    }
}

impl Default for StateManager {
    fn default() -> Self { Self::new() }
}
