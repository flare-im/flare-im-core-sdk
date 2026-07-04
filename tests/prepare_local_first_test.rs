//! prepare（本地半段登录）后、connect 之前，本地优先 API 必须可用：
//! 热启动路径依赖 prepare → bootstrap_startup_home 本地出图 → 后台 connect。

use flare_im_core_sdk::client::IMClient;
use flare_im_core_sdk::client::lifecycle::LoginDbKind;
use flare_im_core_sdk::model::StartupHomeSyncRequest;
use flare_im_core_sdk::storage::in_memory_empty_im_provider;

#[tokio::test]
async fn prepare_then_bootstrap_startup_home_serves_local_before_connect() {
    let client = IMClient::new();
    client.init(None, None).await.expect("init");
    client
        .prepare(
            "hot-start-user",
            LoginDbKind::IndexedDb(in_memory_empty_im_provider()),
        )
        .await
        .expect("prepare must succeed without network");

    client
        .conversation_async()
        .await
        .expect("conversation api must be assembled by prepare");

    let response = client
        .bootstrap_startup_home(StartupHomeSyncRequest {
            conversation_limit: 50,
            start_background_convergence: false,
            ..Default::default()
        })
        .await
        .expect("bootstrap_startup_home must serve local snapshot before connect");

    assert!(!response.served_from_local, "empty local store");
    assert!(
        response.degraded_reason.is_some(),
        "offline cold sync should degrade, not fail"
    );
}
