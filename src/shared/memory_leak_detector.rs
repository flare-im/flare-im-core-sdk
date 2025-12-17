//! 内存泄漏检测器（开发模式）
//!
//! 在开发模式下检测潜在的内存泄漏

use std::sync::Arc;
use std::time::Duration;

#[cfg(debug_assertions)]
use std::collections::HashMap;
#[cfg(debug_assertions)]
use std::sync::Mutex;
#[cfg(debug_assertions)]
use std::time::Instant;
#[cfg(debug_assertions)]
use tracing::warn;

/// 内存泄漏检测器
///
/// 仅在 debug 模式下启用，用于检测潜在的内存泄漏
#[cfg(debug_assertions)]
pub struct MemoryLeakDetector {
    /// Arc 引用计数追踪
    arc_counts: Arc<Mutex<HashMap<String, (usize, Instant)>>>,

    /// 检测间隔
    check_interval: Duration,

    /// 警告阈值（引用计数超过此值会发出警告）
    warning_threshold: usize,
}

#[cfg(debug_assertions)]
impl MemoryLeakDetector {
    /// 创建新的内存泄漏检测器
    pub fn new(check_interval: Duration, warning_threshold: usize) -> Self {
        Self {
            arc_counts: Arc::new(Mutex::new(HashMap::new())),
            check_interval,
            warning_threshold,
        }
    }

    /// 创建默认的内存泄漏检测器
    pub fn default() -> Self {
        Self::new(Duration::from_secs(60), 100)
    }

    /// 注册 Arc 引用计数
    pub fn register_arc<T>(&self, name: impl Into<String>, arc: &Arc<T>) {
        let name = name.into();
        let count = Arc::strong_count(arc);
        let mut counts = self.arc_counts.lock().unwrap();
        counts.insert(name, (count, Instant::now()));
    }

    /// 检查内存泄漏
    pub fn check(&self) -> Vec<LeakWarning> {
        let mut warnings = Vec::new();
        // MutexGuard 需要 mut 来获取可变引用，即使只是读取
        #[allow(unused_mut)]
        let mut counts = self.arc_counts.lock().unwrap();
        let now = Instant::now();

        for (name, (count, last_seen)) in counts.iter() {
            // 检查引用计数是否过高
            if *count > self.warning_threshold {
                warnings.push(LeakWarning {
                    name: name.clone(),
                    count: *count,
                    age: now.duration_since(*last_seen),
                    reason: format!(
                        "Arc reference count ({}) exceeds threshold ({})",
                        count, self.warning_threshold
                    ),
                });
            }

            // 检查引用计数是否持续增长（可能的内存泄漏）
            // 注意：这里简化实现，实际需要更复杂的追踪逻辑
        }

        warnings
    }

    /// 启动定期检查任务
    pub fn start_periodic_check(&self) -> tokio::task::JoinHandle<()> {
        let detector = Arc::new(self.clone());
        let interval = self.check_interval;

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                interval_timer.tick().await;
                let warnings = detector.check();
                if !warnings.is_empty() {
                    for warning in warnings {
                        warn!(
                            name = %warning.name,
                            count = warning.count,
                            age_secs = warning.age.as_secs(),
                            reason = %warning.reason,
                            "Potential memory leak detected"
                        );
                    }
                }
            }
        })
    }
}

#[cfg(debug_assertions)]
impl Clone for MemoryLeakDetector {
    fn clone(&self) -> Self {
        Self {
            arc_counts: Arc::clone(&self.arc_counts),
            check_interval: self.check_interval,
            warning_threshold: self.warning_threshold,
        }
    }
}

/// 内存泄漏警告
#[cfg(debug_assertions)]
#[derive(Debug, Clone)]
pub struct LeakWarning {
    pub name: String,
    pub count: usize,
    pub age: Duration,
    pub reason: String,
}

/// Release 模式下的空实现
#[cfg(not(debug_assertions))]
pub struct MemoryLeakDetector;

#[cfg(not(debug_assertions))]
impl MemoryLeakDetector {
    pub fn new(_check_interval: Duration, _warning_threshold: usize) -> Self {
        Self
    }

    pub fn default() -> Self {
        Self
    }

    pub fn register_arc<T>(&self, _name: impl Into<String>, _arc: &Arc<T>) {
        // 空实现
    }

    pub fn check(&self) -> Vec<LeakWarning> {
        Vec::new()
    }

    pub fn start_periodic_check(&self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        })
    }
}

#[cfg(not(debug_assertions))]
#[derive(Debug, Clone)]
pub struct LeakWarning {
    pub name: String,
    pub count: usize,
    pub age: Duration,
    pub reason: String,
}
