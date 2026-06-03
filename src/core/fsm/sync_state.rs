//! 同步状态机 — 控制单会话/会话列表同步流程（飞书级 IM 基线：Idle ↔ Syncing ↔ CatchingUp/Error）

use crate::shared::error::{FlareError, Result};

/// 同步状态（与 [crate::core::event] 的 SyncStateChanged 回调一致，供 UI 展示）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// 空闲（未在拉取）
    Idle,
    /// 同步中（拉取中）
    Syncing,
    /// 追赶中（断点续传、补全历史）
    CatchingUp,
    /// 错误（可重试）
    Error,
}

/// 触发同步状态转移的事件（与 wire 层 `flare_proto::common::Sync` 区分）
#[derive(Clone, Debug)]
pub enum SyncTransition {
    /// 发起同步
    SyncRequested,
    /// 收到首包（有数据）
    DataReceived,
    /// 本批无更多（has_more=false）
    BatchDone,
    /// 全部同步完成
    SyncDone,
    /// 同步失败
    SyncFailed,
    /// 重置为空闲（如重试前）
    Reset,
}

/// 同步 FSM
pub struct SyncFsm;

impl SyncFsm {
    pub fn transition(from: SyncState, event: &SyncTransition) -> Result<SyncState> {
        use SyncState as S;
        use SyncTransition as E;

        let next = match (from, event) {
            (S::Idle, E::SyncRequested) => S::Syncing,
            (S::Syncing, E::DataReceived) => S::CatchingUp,
            (S::Syncing, E::BatchDone) | (S::Syncing, E::SyncDone) => S::Idle,
            (S::Syncing, E::SyncFailed) => S::Error,
            (S::CatchingUp, E::DataReceived) => S::CatchingUp,
            (S::CatchingUp, E::BatchDone) => S::Syncing,
            (S::CatchingUp, E::SyncDone) => S::Idle,
            (S::CatchingUp, E::SyncFailed) => S::Error,
            (S::Error, E::Reset) | (S::Error, E::SyncRequested) => S::Syncing,
            (S::Idle, E::Reset) => S::Idle,
            _ => {
                return Err(FlareError::system(format!(
                    "invalid sync transition: {:?} + {:?}",
                    from, event
                )));
            }
        };
        Ok(next)
    }
}
