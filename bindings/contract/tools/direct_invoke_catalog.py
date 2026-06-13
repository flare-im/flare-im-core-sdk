"""LEGACY helper — production source of truth is `contract/direct_invoke.json`.

Edit `bindings/contract/direct_invoke.json`, then run `make -C bindings codegen`.
"""

from __future__ import annotations

from typing import Any

# fmt: off
ROUTES: list[dict[str, Any]] = [
    {
        "route": "sync.conversation",
        "result": "unit",
        "body": """
            let conversation_id = dispatch_support::json_string(request, "conversationId")?;
            client.sync_conversation(&conversation_id).await?;
        """,
    },
    {
        "route": "sync.messages",
        "result": "unit",
        "body": """
            let conversation_id = dispatch_support::json_string(request, "conversationId")?;
            let last_seq = dispatch_support::json_u64(request, "lastSeq")?;
            let limit = dispatch_support::optional_i32(request, "limit").unwrap_or(50);
            client.sync_messages(&conversation_id, last_seq, limit).await?;
        """,
    },
    {
        "route": "sync.mark_session_read",
        "result": "unit",
        "body": """
            let conversation_id = dispatch_support::json_string(request, "conversationId")?;
            let read_seq = dispatch_support::json_u64(request, "readSeq")?;
            client.mark_session_read(&conversation_id, read_seq).await?;
        """,
    },
    {
        "route": "presence.get",
        "result": "json",
        "body": """
            let user_id = dispatch_support::json_string(request, "userId")?;
            let dto = client.get_user_presence(&user_id).await?;
        """,
        "json_expr": "dto",
    },
    {
        "route": "presence.batch_get",
        "result": "json",
        "body": """
            let user_ids = dispatch_support::json_vec_string(request, "userIds")?;
            let items = client.batch_get_user_presence(&user_ids).await?;
        """,
        "json_expr": "items",
    },
    {
        "route": "presence.subscribe",
        "result": "unit",
        "body": """
            let user_ids = dispatch_support::json_vec_string(request, "userIds")?;
            client.subscribe_user_presence(user_ids).await?;
        """,
    },
    {
        "route": "connection.get_state",
        "result": "json",
        "body": """
            let state = client.state();
            let (name, code) = sdk_state_json(state);
        """,
        "json_expr": "serde_json::json!({ \"state\": name, \"code\": code })",
    },
    {
        "route": "connection.disconnect",
        "result": "unit",
        "body": """
            session.after_disconnect().await;
            client.disconnect().await?;
        """,
    },
    {
        "route": "sdk.is_connected",
        "result": "json",
        "body": """
            let connected = client.is_connected().await;
        """,
        "json_expr": "serde_json::json!({ \"connected\": connected })",
    },
    {
        "route": "sdk.session_active",
        "result": "json",
        "body": """
            let active = client.session_active_sync();
        """,
        "json_expr": "serde_json::json!({ \"active\": active })",
    },
    {
        "route": "sdk.current_user_id",
        "result": "json",
        "body": """
            let user_id = client.current_user_id().await;
        """,
        "json_expr": "serde_json::json!({ \"userId\": user_id })",
    },
    {
        "route": "sdk.update_access_token",
        "result": "unit",
        "body": """
            let access_token = dispatch_support::json_string(request, "accessToken")?;
            let tenant_id = dispatch_support::optional_string(request, "tenantId");
            client
                .update_access_token(access_token, tenant_id.as_deref())
                .await?;
        """,
    },
    {
        "route": "message_builder.list_catalog",
        "result": "json",
        "body": "",
        "json_expr": "serde_json::json!({ \"entries\": crate::operation::message_build_catalog() })",
        "skip_client": True,
    },
    {
        "route": "diagnostics.sdk_version",
        "result": "json",
        "body": "",
        "json_expr": "serde_json::json!({ \"version\": env!(\"CARGO_PKG_VERSION\") })",
        "skip_client": True,
    },
    {
        "route": "diagnostics.ffi_contract_version",
        "result": "json",
        "body": "",
        "json_expr": "serde_json::json!({ \"version\": crate::contract::BINDING_CONTRACT_VERSION })",
        "skip_client": True,
    },
    {
        "route": "diagnostics.data_root",
        "result": "json",
        "body": """
            let data_root = client.data_root().await;
        """,
        "json_expr": """
            serde_json::json!({
                \"dataRoot\": data_root.as_ref().map(|p| p.display().to_string())
            })
        """,
    },
]
