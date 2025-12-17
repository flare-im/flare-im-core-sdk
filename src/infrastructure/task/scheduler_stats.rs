//! 任务调度器内部统计
//!
//! 用于性能监控和限流

use std::collections::VecDeque;
use tokio::time::Instant;

/// 任务执行统计
#[derive(Debug, Clone)]
pub struct TaskExecutionStats {
    /// 任务 ID
    pub task_id: String,
    /// 任务名称
    pub task_name: String,
    /// 执行开始时间
    pub start_time: Instant,
    /// 执行结束时间（如果已完成）
    pub end_time: Option<Instant>,
    /// 是否成功
    pub success: bool,
    /// 重试次数
    pub retry_count: u32,
}

/// 任务调度器内部统计
pub struct TaskSchedulerInternalStats {
    /// 总任务数
    pub total_tasks: u64,
    /// 成功任务数
    pub successful_tasks: u64,
    /// 失败任务数
    pub failed_tasks: u64,
    /// 重试任务数
    pub retried_tasks: u64,
    /// 最近的任务执行记录（用于计算平均执行时间）
    pub recent_executions: VecDeque<TaskExecutionStats>,
    /// 最大记录数
    pub max_recent_records: usize,
    /// 平均执行时间（毫秒）
    pub avg_execution_time_ms: f64,
    /// 最后更新时间
    pub last_update: Instant,
}

impl TaskSchedulerInternalStats {
    pub fn new() -> Self {
        Self {
            total_tasks: 0,
            successful_tasks: 0,
            failed_tasks: 0,
            retried_tasks: 0,
            recent_executions: VecDeque::with_capacity(1000),
            max_recent_records: 1000,
            avg_execution_time_ms: 0.0,
            last_update: Instant::now(),
        }
    }

    /// 记录任务执行
    pub fn record_execution(&mut self, stats: TaskExecutionStats) {
        self.total_tasks += 1;

        if stats.success {
            self.successful_tasks += 1;
        } else {
            self.failed_tasks += 1;
        }

        if stats.retry_count > 0 {
            self.retried_tasks += 1;
        }

        // 更新最近执行记录
        if self.recent_executions.len() >= self.max_recent_records {
            self.recent_executions.pop_front();
        }
        self.recent_executions.push_back(stats);

        // 更新平均执行时间
        self.update_avg_execution_time();

        self.last_update = Instant::now();
    }

    /// 更新平均执行时间
    fn update_avg_execution_time(&mut self) {
        let completed: Vec<_> = self
            .recent_executions
            .iter()
            .filter(|s| s.end_time.is_some())
            .collect();

        if completed.is_empty() {
            self.avg_execution_time_ms = 0.0;
            return;
        }

        let total_ms: f64 = completed
            .iter()
            .map(|s| {
                let duration = s.end_time.unwrap().duration_since(s.start_time);
                duration.as_millis() as f64
            })
            .sum();

        self.avg_execution_time_ms = total_ms / completed.len() as f64;
    }

    /// 获取成功率
    pub fn success_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            return 0.0;
        }
        self.successful_tasks as f64 / self.total_tasks as f64
    }

    /// 获取重试率
    pub fn retry_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            return 0.0;
        }
        self.retried_tasks as f64 / self.total_tasks as f64
    }

    /// 获取最近 N 个任务的执行时间
    pub fn recent_avg_execution_time_ms(&self, n: usize) -> f64 {
        let completed: Vec<_> = self
            .recent_executions
            .iter()
            .rev()
            .filter(|s| s.end_time.is_some())
            .take(n)
            .collect();

        if completed.is_empty() {
            return 0.0;
        }

        let total_ms: f64 = completed
            .iter()
            .map(|s| {
                let duration = s.end_time.unwrap().duration_since(s.start_time);
                duration.as_millis() as f64
            })
            .sum();

        total_ms / completed.len() as f64
    }
}

impl Default for TaskSchedulerInternalStats {
    fn default() -> Self {
        Self::new()
    }
}

/// 任务调度器性能快照
#[derive(Debug, Clone)]
pub struct TaskSchedulerPerformanceSnapshot {
    /// 总任务数
    pub total_tasks: u64,
    /// 成功任务数
    pub successful_tasks: u64,
    /// 失败任务数
    pub failed_tasks: u64,
    /// 重试任务数
    pub retried_tasks: u64,
    /// 成功率
    pub success_rate: f64,
    /// 重试率
    pub retry_rate: f64,
    /// 平均执行时间（毫秒）
    pub avg_execution_time_ms: f64,
    /// 最近 100 个任务的平均执行时间（毫秒）
    pub recent_avg_execution_time_ms: f64,
    /// 最后更新时间
    pub last_update: Instant,
}

impl From<&TaskSchedulerInternalStats> for TaskSchedulerPerformanceSnapshot {
    fn from(stats: &TaskSchedulerInternalStats) -> Self {
        Self {
            total_tasks: stats.total_tasks,
            successful_tasks: stats.successful_tasks,
            failed_tasks: stats.failed_tasks,
            retried_tasks: stats.retried_tasks,
            success_rate: stats.success_rate(),
            retry_rate: stats.retry_rate(),
            avg_execution_time_ms: stats.avg_execution_time_ms,
            recent_avg_execution_time_ms: stats.recent_avg_execution_time_ms(100),
            last_update: stats.last_update,
        }
    }
}
