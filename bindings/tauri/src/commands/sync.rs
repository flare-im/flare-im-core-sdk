//! 同步命令：仅触发 SDK 侧同步，进度/完成/失败由 SDK 通过事件回调通知（im://sync_progress / im://sync_completed / im://sync_failed）

use tauri::State;

use crate::state::SdkState;

/// 触发全量同步会话列表；结果通过 im://conversations_synced / im://sync_completed 等事件回调
#[tauri::command]
pub async fn sdk_sync(state: State<'_, SdkState>) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
            c.sync_conversations().await.map_err(|e| e.to_string())
        })
        })
        .await?;
    Ok(())
}

/// 触发单会话增量同步；结果通过 im://message / im://sync_completed 等事件回调
#[tauri::command]
pub async fn sdk_sync_session_incremental(
    state: State<'_, SdkState>,
    session_id: String,
) -> std::result::Result<u32, String> {
    let n = state
        .with_client(|c| {
            Box::pin(async move {
                c.sync_conversation(&session_id).await.map_err(|e| e.to_string())?;
                Ok::<u32, String>(0u32)
            })
        })
        .await?;
    Ok(n)
}
