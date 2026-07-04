//! 收敛 drain worker：按注意力优先级顺序排空 [ConvergenceScheduler]，逐个执行实际收敛。
//! 收敛动作由 application 注入的 [ConvergeTarget] 提供（如调 `SessionSyncRunner` 对单会话补齐）。
//!
//! 优先级顺序排空 = 前台/可见会话先补齐（秒展示）；有界并发为后续优化（当前顺序执行，正确优先）。

use std::future::Future;
use std::pin::Pin;

use futures::stream::{self, StreamExt};

use crate::shared::error::Result;

use super::scheduler::{ConvergenceScheduler, ConvergenceTarget};

/// 对单个收敛目标执行实际"拉取+应用"。由 application 实现（降级路径下委托单会话同步）。
pub trait ConvergeTarget: Send + Sync {
    fn converge<'a>(
        &'a self,
        target: ConvergenceTarget,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}

/// 一次排空的结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrainReport {
    pub converged: usize,
    pub failed: usize,
}

pub struct ConvergenceDriver;

impl ConvergenceDriver {
    /// 按当前注意力顺序导出并清空所有待收敛目标。
    /// 批量收敛路径先用它固定优先级顺序，再按有界批大小发送请求。
    pub fn drain_ordered_targets(scheduler: &mut ConvergenceScheduler) -> Vec<ConvergenceTarget> {
        let mut targets = Vec::new();
        while let Some(target) = scheduler.pop_next() {
            targets.push(target);
        }
        targets
    }

    /// 按优先级顺序排空调度器，`max_concurrency` 有界并发执行收敛（`1`=严格顺序）。
    /// 先按注意力优先级弹出全部目标（前台/可见在前），再有界并发发起——高优先先启动，
    /// 且不让长尾无界并发压垮网络/渲染。失败记数不中断其余目标。
    pub async fn drain(
        scheduler: &mut ConvergenceScheduler,
        converger: &dyn ConvergeTarget,
        max_concurrency: usize,
    ) -> DrainReport {
        let targets = Self::drain_ordered_targets(scheduler);
        if targets.is_empty() {
            return DrainReport::default();
        }

        let outcomes = stream::iter(targets.into_iter().map(|target| async move {
            let result = converger.converge(target.clone()).await;
            (target, result)
        }))
        .buffer_unordered(max_concurrency.max(1))
        .collect::<Vec<_>>()
        .await;

        let mut report = DrainReport::default();
        for (target, result) in outcomes {
            match result {
                Ok(()) => report.converged += 1,
                Err(error) => {
                    report.failed += 1;
                    tracing::warn!(?target, error = %error, "收敛目标失败，继续其余目标");
                }
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingConverger {
        order: Mutex<Vec<ConvergenceTarget>>,
        fail_on: Option<ConvergenceTarget>,
    }

    impl ConvergeTarget for RecordingConverger {
        fn converge<'a>(
            &'a self,
            target: ConvergenceTarget,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
            Box::pin(async move {
                self.order.lock().unwrap().push(target.clone());
                if self.fail_on.as_ref() == Some(&target) {
                    return Err(crate::shared::error::FlareError::system("boom"));
                }
                Ok(())
            })
        }
    }

    fn conv(id: &str) -> ConvergenceTarget {
        ConvergenceTarget::Conversation(id.to_string())
    }

    #[tokio::test]
    async fn drains_in_attention_priority_order() {
        let mut s = ConvergenceScheduler::new(16);
        s.enqueue(conv("cold-1"));
        s.enqueue(conv("cold-2"));
        s.enqueue(conv("fg"));
        s.attention_mut().open_timeline("fg");
        s.attention_mut().set_visible(["cold-2"]); // cold-2 → P1，先于 cold-1(P3)

        let converger = RecordingConverger {
            order: Mutex::new(Vec::new()),
            fail_on: None,
        };
        let report = ConvergenceDriver::drain(&mut s, &converger, 1).await;

        assert_eq!(report.converged, 3);
        assert_eq!(report.failed, 0);
        // 顺序(concurrency=1)：fg(P0) → cold-2(P1) → cold-1(P3)
        assert_eq!(
            *converger.order.lock().unwrap(),
            vec![conv("fg"), conv("cold-2"), conv("cold-1")]
        );
        assert_eq!(s.pending_len(), 0);
    }

    #[tokio::test]
    async fn failure_does_not_stop_remaining_targets() {
        let mut s = ConvergenceScheduler::new(16);
        s.enqueue(conv("a"));
        s.enqueue(conv("b"));
        let converger = RecordingConverger {
            order: Mutex::new(Vec::new()),
            fail_on: Some(conv("a")),
        };
        let report = ConvergenceDriver::drain(&mut s, &converger, 4).await;
        assert_eq!(report.converged, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(converger.order.lock().unwrap().len(), 2);
    }

    #[test]
    fn drains_ordered_targets_without_running_network_work() {
        let mut s = ConvergenceScheduler::new(16);
        s.enqueue(conv("cold"));
        s.enqueue(conv("visible"));
        s.enqueue(conv("fg"));
        s.attention_mut().open_timeline("fg");
        s.attention_mut().set_visible(["visible"]);

        let ordered = ConvergenceDriver::drain_ordered_targets(&mut s);

        assert_eq!(ordered, vec![conv("fg"), conv("visible"), conv("cold")]);
        assert_eq!(s.pending_len(), 0);
    }
}
