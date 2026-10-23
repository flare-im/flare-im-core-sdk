use super::*;
use flare_plugin_protocol::{
    PluginManifest,
    ui::{ClientPlugin, NativeCapability, UiActionTarget, UiCatalog},
};
use serde_json::json;
use std::sync::{
    Mutex as SyncMutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

struct Fixture {
    api: CapabilityApi,
    catalog: Arc<SyncMutex<UiCatalog>>,
    calls: Arc<SyncMutex<Vec<Value>>>,
    user: Arc<RwLock<String>>,
    hold: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    // Read only by the durable-command tests, which need SQLite storage.
    #[cfg_attr(not(feature = "storage-sqlite"), allow(dead_code))]
    drop_next: Arc<AtomicBool>,
    #[cfg_attr(not(feature = "storage-sqlite"), allow(dead_code))]
    business_error: Arc<SyncMutex<Option<String>>>,
    #[cfg_attr(not(feature = "storage-sqlite"), allow(dead_code))]
    observation: Arc<SyncMutex<Option<Value>>>,
    vote_result: Arc<SyncMutex<Value>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let manifest = PluginManifest::from_json(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../flare-proto/flare-plugin-protocol/tests/fixtures/plugin-ui-manifest.json"
        )))
        .unwrap();
        let catalog = Arc::new(SyncMutex::new(UiCatalog {
            tenant_id: "201".into(),
            version: 1,
            valid_until_ms: chrono::Utc::now().timestamp_millis() + 60000,
            plugins: vec![ClientPlugin {
                plugin_id: manifest.plugin_id,
                version: manifest.version,
                manifest_digest: format!("sha256:{}", "a".repeat(64)),
                grant_revision: 1,
                extensions: manifest.client_extensions,
                operations: ["example.poll.vote".into(), "example.poll.details".into()].into(),
                configuration: Default::default(),
            }],
            unavailable: vec![],
        }));
        let calls = Arc::new(SyncMutex::new(Vec::new()));
        let hold = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let drop_next = Arc::new(AtomicBool::new(false));
        let drop_response = drop_next.clone();
        let business_error = Arc::new(SyncMutex::new(None::<String>));
        let next_error = business_error.clone();
        let observation = Arc::new(SyncMutex::new(None::<Value>));
        let observed = observation.clone();
        let vote_result = Arc::new(SyncMutex::new(json!({"status":"recorded"})));
        let voted = vote_result.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (c, q, h, e, r) = (
            catalog.clone(),
            calls.clone(),
            hold.clone(),
            entered.clone(),
            release.clone(),
        );
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(socket);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                assert!(line.starts_with("POST /api/v1/capabilities/dispatch "));
                let mut len = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).await.unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    let (name, value) = header.trim().split_once(':').unwrap();
                    if name.eq_ignore_ascii_case("content-length") {
                        len = value.trim().parse().unwrap();
                    }
                }
                assert!(len <= 32768);
                let mut bytes = vec![0; len];
                reader.read_exact(&mut bytes).await.unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(body["expectedTenantId"], "201");
                assert_eq!(body["expectedUserId"], "u1");
                q.lock().unwrap().push(body.clone());
                if body["capabilityId"] != flare_plugin_protocol::ui::UI_CATALOG_OPERATION
                    && drop_response.swap(false, Ordering::SeqCst)
                {
                    continue;
                }
                let data = if body["capabilityId"]
                    == flare_plugin_protocol::ui::UI_CATALOG_OPERATION
                {
                    if h.load(Ordering::SeqCst) {
                        e.notify_one();
                        r.notified().await;
                    }
                    serde_json::to_value(c.lock().unwrap().clone()).unwrap()
                } else if body["capabilityId"] == "example.poll.lookup" {
                    observed.lock().unwrap().clone().unwrap_or_else(||json!({
                            "commandId":"stable-command","capabilityId":"example.poll.vote",
                            "requestDigest":flare_plugin_protocol::runtime::configuration_digest(&body["payload"]).unwrap(),
                            "outcome":"committed","result":{"status":"recorded"}
                        }))
                } else {
                    assert_eq!(body["capabilityId"], "example.poll.vote");
                    assert_eq!(body["conversationId"], "conversation-1");
                    assert_eq!(body["payload"], json!({"pollId":"poll-1"}));
                    voted.lock().unwrap().clone()
                };
                let error_code = next_error.lock().unwrap().take();
                let result = CapabilityDispatchResult {
                    request_id: body["requestId"].as_str().unwrap().into(),
                    success: error_code.is_none(),
                    plugin_id: "example.poll".into(),
                    capability_id: body["capabilityId"].as_str().unwrap().into(),
                    data: error_code.map_or(data, |code| json!({"errorCode":code})),
                    error: None,
                };
                let raw = serde_json::to_vec(&json!({"code":0,"data":result})).unwrap();
                let mut socket = reader.into_inner();
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    raw.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&raw).await;
            }
        });
        let user = Arc::new(RwLock::new("u1".into()));
        let api = CapabilityApi::new(
            "http://127.0.0.1:1",
            user.clone(),
            "201",
            Arc::new(HttpRequestContext::new()),
        )
        .with_http_transport(HttpClient::new(base));
        Self {
            api,
            catalog,
            calls,
            user,
            hold,
            entered,
            release,
            drop_next,
            business_error,
            observation,
            vote_result,
            server,
        }
    }
}
#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn plugin_command_public_api_reconciles_lost_response_without_replacing_intent() {
    use crate::domain::PluginCommandStore;
    use crate::infrastructure::persistence::sqlite::{SqlitePluginCommandRepo, init_schema};
    let mut f = Fixture::new().await;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init_schema(&pool).await.unwrap();
    let store = Arc::new(SqlitePluginCommandRepo::new(pool));
    f.api = f.api.clone().with_plugin_command_store(Some(store.clone()));
    assert!(f.api.prepare_plugin_command(json!({"slot":"other","expectedUserId":"other-user","command":{"commandId":"one","capabilityId":"example.create","conversationId":null,"payload":{}}})).await.is_err());
    assert!(
        f.api
            .pending_plugin_command(json!({"slot":"other","expectedUserId":"other-user"}))
            .await
            .is_err()
    );
    let command = json!({"slot":"example.create","command":{"commandId":"stable-command","capabilityId":"example.poll.vote","conversationId":"conversation-1","payload":{"pollId":"poll-1"}}});
    let submit = json!({"slot":"example.create","commandId":"stable-command"});
    f.api.prepare_plugin_command(command.clone()).await.unwrap();
    assert!(
        f.calls.lock().unwrap().is_empty(),
        "prepare commits without networking"
    );
    f.drop_next.store(true, Ordering::SeqCst);
    assert!(f.api.submit_plugin_command(submit.clone()).await.is_err());
    let pending = f
        .api
        .pending_plugin_command(json!({"slot":"example.create"}))
        .await
        .unwrap();
    assert!(pending["record"]["response"].is_null());
    assert!(
        f.api
            .acknowledge_plugin_command(submit.clone())
            .await
            .is_err()
    );
    let mut changed = command.clone();
    changed["command"]["commandId"] = json!("second-command");
    assert!(f.api.prepare_plugin_command(changed).await.is_err());
    let completed = f.api.submit_plugin_command(submit.clone()).await.unwrap();
    assert_eq!(completed["response"]["data"], json!({"status":"recorded"}));
    assert_eq!(
        f.api.submit_plugin_command(submit.clone()).await.unwrap(),
        completed
    );
    assert_eq!(
        f.calls.lock().unwrap().len(),
        2,
        "recorded responses do not resubmit"
    );
    for request in f.calls.lock().unwrap().iter() {
        assert_eq!(request["payload"], command["command"]["payload"]);
        assert_eq!(request["conversationId"], "conversation-1");
    }
    let mut forged = json!({"slot":"example.create","userId":"other"});
    assert!(f.api.pending_plugin_command(forged.clone()).await.is_err());
    forged.as_object_mut().unwrap().remove("userId");
    *f.user.write().await = String::new();
    assert!(f.api.pending_plugin_command(forged).await.is_err());
    assert!(f.api.submit_plugin_command(submit.clone()).await.is_err());
    assert_eq!(f.calls.lock().unwrap().len(), 2);
    assert!(
        store
            .pending(&crate::domain::PluginCommandScope {
                tenant_id: "201".into(),
                user_id: "u1".into(),
                slot: "example.create".into()
            })
            .await
            .unwrap()
            .is_some()
    );
    *f.user.write().await = "u1".into();
    f.api.acknowledge_plugin_command(submit).await.unwrap();
    assert!(
        f.api
            .pending_plugin_command(json!({"slot":"example.create"}))
            .await
            .unwrap()["record"]
            .is_null()
    );
}
#[tokio::test]
async fn plugin_command_without_durable_storage_denies_before_network() {
    let f = Fixture::new().await;
    assert!(f.api.prepare_plugin_command(json!({"slot":"example","command":{"commandId":"one","capabilityId":"example.create","conversationId":null,"payload":{}}})).await.is_err());
    assert!(f.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn plugin_command_keeps_captured_identity_across_a_storage_await() {
    use crate::domain::{
        PluginCommand, PluginCommandRecord, PluginCommandScope, PluginCommandStore,
    };
    struct SwitchingStore(Arc<RwLock<String>>);
    #[async_trait::async_trait]
    impl PluginCommandStore for SwitchingStore {
        async fn prepare(
            &self,
            _: &PluginCommandScope,
            _: &PluginCommand,
        ) -> Result<PluginCommandRecord> {
            Err(FlareError::system("unexpected prepare"))
        }
        async fn pending(&self, scope: &PluginCommandScope) -> Result<Option<PluginCommandRecord>> {
            assert_eq!(scope.user_id, "u1");
            *self.0.write().await = "u2".into();
            Ok(Some(PluginCommandRecord {
                command: PluginCommand {
                    reconciliation_capability_id: None,
                    command_id: "stable".into(),
                    capability_id: "example.poll.vote".into(),
                    conversation_id: Some("conversation-1".into()),
                    payload: json!({"pollId":"poll-1"}),
                    terminal_error_codes: vec![],
                },
                response: None,
            }))
        }
        async fn complete(
            &self,
            _: &PluginCommandScope,
            _: &PluginCommand,
            _: Value,
        ) -> Result<PluginCommandRecord> {
            Err(FlareError::system("unexpected completion"))
        }
        async fn acknowledge(&self, _: &PluginCommandScope, _: &str) -> Result<()> {
            Err(FlareError::system("unexpected acknowledgement"))
        }
    }
    let mut f = Fixture::new().await;
    f.api = f
        .api
        .clone()
        .with_plugin_command_store(Some(Arc::new(SwitchingStore(f.user.clone()))));
    let error = f
        .api
        .submit_plugin_command(
            json!({"slot":"example.create","commandId":"stable","expectedUserId":"u1"}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(ErrorCode::NotConnected));
    assert!(
        f.calls.lock().unwrap().is_empty(),
        "a storage await must not rebind an old command to the next account"
    );
    assert!(!f.server.is_finished());
}

#[tokio::test]
async fn capability_grpc_dispatch_identity_preconditions_match_http() {
    let mut f = Fixture::new().await;
    f.api.http = None;
    for (tenant, user) in [(Some("202"), None), (None, Some("u2"))] {
        let error = f
            .api
            .dispatch("example.poll.vote", json!({}), None, tenant, user)
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            Some(ErrorCode::InvalidParameter),
            "identity precondition fails before connecting to the unavailable endpoint"
        );
    }
}
fn message() -> Value {
    json!({"pluginId":"example.poll","contributionId":"example.poll.card","schemaVersion":1,"summary":"Poll history","payload":{"pollId":"poll-1","question":"Question","tenantId":"forged","userId":"forged","permissions":["admin"]}})
}
fn invoke(platform: &str) -> Value {
    json!({"message":message(),"platform":platform,"actionId":"vote","userGesture":true,"conversationId":"conversation-1","nativeAllowlist":[]})
}
#[tokio::test]
async fn plugin_ui_native_orchestration_is_identical_and_conversation_bound_on_six_platforms() {
    let f = Fixture::new().await;
    for platform in ["web", "tauri", "android", "ios", "flutter", "harmonyos"] {
        let presentation = f
            .api
            .present_plugin_message(
                json!({"message":message(),"platform":platform,"locale":"en-US"}),
            )
            .await
            .unwrap();
        assert_eq!(presentation["available"], true);
        assert_eq!(presentation["title"], "Poll");
        let result = f.api.invoke_plugin_action(invoke(platform)).await.unwrap();
        assert_eq!(result["kind"], "operation");
        assert_eq!(result["response"]["data"]["status"], "recorded");
    }
    assert_eq!(
        f.calls.lock().unwrap().len(),
        18,
        "present and every click each fetch a fresh catalog"
    );
    f.catalog.lock().unwrap().plugins.clear();
    let fallback = f
        .api
        .present_plugin_message(json!({"message":message(),"platform":"ios"}))
        .await
        .unwrap();
    assert_eq!(fallback["available"], false);
    assert!(fallback["actions"].as_array().unwrap().is_empty());
    assert!(f.api.invoke_plugin_action(invoke("ios")).await.is_err());
    assert_eq!(
        f.calls.lock().unwrap().len(),
        20,
        "revoked actions never dispatch"
    );
}
#[tokio::test]
async fn plugin_ui_native_bridge_needs_current_grant_gesture_and_explicit_allowlist() {
    let f = Fixture::new().await;
    {
        let mut catalog = f.catalog.lock().unwrap();
        let plugin = &mut catalog.plugins[0];
        plugin
            .extensions
            .native_capabilities
            .push(NativeCapability::ClipboardWrite);
        plugin.extensions.contributions[0].schema.actions[1].target = UiActionTarget::Native {
            capability: NativeCapability::ClipboardWrite,
        };
    }
    assert!(f.api.invoke_plugin_action(invoke("ios")).await.is_err());
    let mut input = invoke("ios");
    input["nativeAllowlist"] = json!(["clipboardWrite"]);
    input["userGesture"] = json!(false);
    assert!(f.api.invoke_plugin_action(input.clone()).await.is_err());
    input["userGesture"] = json!(true);
    let result = f.api.invoke_plugin_action(input.clone()).await.unwrap();
    assert_eq!(result["kind"], "host");
    assert_eq!(result["target"]["capability"], "clipboardWrite");
    assert_eq!(result["payload"], json!({"pollId":"poll-1"}));
    for (key, value) in [
        ("tenantId", json!("202")),
        ("catalog", json!({})),
        ("conversationId", json!("../other")),
    ] {
        let mut bad = input.clone();
        bad[key] = value;
        assert!(f.api.invoke_plugin_action(bad).await.is_err());
    }
    f.catalog.lock().unwrap().valid_until_ms = 1;
    assert!(f.api.invoke_plugin_action(input).await.is_err());
    assert_eq!(
        f.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["capabilityId"] == "example.poll.vote")
            .count(),
        0
    );
}
#[tokio::test]
async fn plugin_ui_native_pending_work_is_cancelled_on_logout() {
    let f = Fixture::new().await;
    f.hold.store(true, Ordering::SeqCst);
    let api = f.api.clone();
    let call = tokio::spawn(async move { api.invoke_plugin_action(invoke("android")).await });
    f.entered.notified().await;
    *f.user.write().await = String::new();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), call)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_err());
    f.release.notify_one();
    assert_eq!(
        f.calls.lock().unwrap().len(),
        1,
        "logout aborts before business dispatch"
    );
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn plugin_command_transient_business_failure_remains_reconcilable() {
    use crate::infrastructure::persistence::sqlite::{SqlitePluginCommandRepo, init_schema};
    let mut f = Fixture::new().await;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init_schema(&pool).await.unwrap();
    f.api = f
        .api
        .clone()
        .with_plugin_command_store(Some(Arc::new(SqlitePluginCommandRepo::new(pool))));
    let slot = json!({"slot":"example.create"});
    let submit = json!({"slot":"example.create","commandId":"stable"});
    f.api.prepare_plugin_command(json!({"slot":"example.create","command":{"commandId":"stable","capabilityId":"example.poll.vote","conversationId":"conversation-1","payload":{"pollId":"poll-1"},"terminalErrorCodes":["INSUFFICIENT_FUNDS"]}})).await.unwrap();
    *f.business_error.lock().unwrap() = Some("UNAVAILABLE".into());
    assert!(f.api.submit_plugin_command(submit.clone()).await.is_err());
    assert!(
        f.api.pending_plugin_command(slot.clone()).await.unwrap()["record"]["response"].is_null()
    );
    assert!(
        f.api
            .acknowledge_plugin_command(submit.clone())
            .await
            .is_err()
    );
    *f.business_error.lock().unwrap() = Some("INSUFFICIENT_FUNDS".into());
    let result = f.api.submit_plugin_command(submit.clone()).await.unwrap();
    assert_eq!(result["response"]["success"], false);
    f.api.acknowledge_plugin_command(submit).await.unwrap();
    assert!(f.api.pending_plugin_command(slot).await.unwrap()["record"].is_null());
    assert_eq!(f.calls.lock().unwrap().len(), 2);
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn plugin_command_query_observation_is_persisted_without_second_mutation() {
    use crate::infrastructure::persistence::sqlite::{SqlitePluginCommandRepo, init_schema};
    let mut f = Fixture::new().await;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init_schema(&pool).await.unwrap();
    f.api = f
        .api
        .clone()
        .with_plugin_command_store(Some(Arc::new(SqlitePluginCommandRepo::new(pool))));
    let command = json!({"slot":"create","command":{"commandId":"stable-command","capabilityId":"example.poll.vote","reconciliationCapabilityId":"example.poll.lookup","conversationId":"conversation-1","payload":{"pollId":"poll-1"}}});
    let request = json!({"slot":"create","commandId":"stable-command","expectedUserId":"u1"});
    f.api.prepare_plugin_command(command).await.unwrap();
    f.drop_next.store(true, Ordering::SeqCst);
    assert!(f.api.submit_plugin_command(request.clone()).await.is_err());
    // A wrong receipt cannot become a locally successful command.
    *f.observation.lock().unwrap() = Some(
        json!({"commandId":"stable-command","capabilityId":"example.poll.vote","requestDigest":"sha256:wrong","outcome":"committed","result":{}}),
    );
    assert!(
        f.api
            .reconcile_plugin_command(request.clone())
            .await
            .is_err()
    );
    assert!(
        f.api
            .acknowledge_plugin_command(request.clone())
            .await
            .is_err()
    );
    let digest =
        flare_plugin_protocol::runtime::configuration_digest(&json!({"pollId":"poll-1"})).unwrap();
    *f.observation.lock().unwrap() = Some(
        json!({"commandId":"stable-command","capabilityId":"example.poll.vote","requestDigest":digest,"outcome":"notFound","result":null}),
    );
    let absent = f
        .api
        .reconcile_plugin_command(request.clone())
        .await
        .unwrap();
    assert_eq!(absent["outcome"], "notFound");
    assert!(absent["record"]["response"].is_null());
    assert!(
        f.api
            .acknowledge_plugin_command(request.clone())
            .await
            .is_err()
    );
    *f.observation.lock().unwrap() = None;
    let observed = f
        .api
        .reconcile_plugin_command(request.clone())
        .await
        .unwrap();
    assert_eq!(
        observed["record"]["response"]["capabilityId"],
        "example.poll.vote"
    );
    assert_eq!(
        observed["record"]["response"]["data"],
        json!({"status":"recorded"})
    );
    let count = f.calls.lock().unwrap().len();
    assert_eq!(
        f.api.submit_plugin_command(request.clone()).await.unwrap(),
        observed["record"]
    );
    assert_eq!(
        f.calls.lock().unwrap().len(),
        count,
        "durable observation prevents re-submit"
    );
    assert_eq!(
        f.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c["capabilityId"] == "example.poll.vote")
            .count(),
        1
    );
    assert!(
        f.api
            .reconcile_plugin_command(
                json!({"slot":"create","commandId":"stable-command","expectedUserId":"other"})
            )
            .await
            .is_err()
    );
    assert!(
        f.api
            .reconcile_plugin_command(
                json!({"slot":"create","commandId":"stable-command","response":{"success":true}})
            )
            .await
            .is_err()
    );
    f.api.acknowledge_plugin_command(request).await.unwrap();
}

fn plugin_card(server_id: &str, client_msg_id: &str, envelope: Value) -> crate::model::IMMessage {
    let mut card = crate::model::IMMessage::new(flare_proto::common::Message::default());
    card.server_id = server_id.into();
    card.client_msg_id = client_msg_id.into();
    card.conversation_id = "conversation-1".into();
    card.content = Some(crate::model::Elem::Custom(
        crate::content::message_elem::CustomElem {
            r#type: "flare.plugin.message.v1".into(),
            payload: serde_json::to_vec(&envelope).unwrap(),
            description: String::new(),
            metadata: Default::default(),
        },
    ));
    card
}
#[tokio::test]
async fn plugin_message_state_is_written_from_card_actions_and_overlaid_on_present() {
    use crate::domain::{MessageReader, MessageWriter};
    use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
    let mut f = Fixture::new().await;
    let store = Arc::new(crate::infrastructure::persistence::memory_im::MemoryMessageStore::new());
    store
        .save_one(&plugin_card("server-1", "client-1", message()))
        .await
        .unwrap();
    let mut other_card = message();
    other_card["contributionId"] = json!("example.poll.other");
    store
        .save_one(&plugin_card("server-2", "client-2", other_card))
        .await
        .unwrap();
    let bus = EventBus::new();
    let mut events = bus.subscribe();
    f.api = f.api.clone().with_message_state(store.clone(), bus);
    let present = |message_id: &str| json!({"message":message(),"platform":"ios","locale":"en-US","messageId":message_id});

    let fresh = f
        .api
        .present_plugin_message(present("server-1"))
        .await
        .unwrap();
    assert_eq!(fresh["available"], true);
    assert_eq!(fresh["stateVersion"], 0);
    assert_eq!(fresh["data"], message()["payload"]);

    *f.vote_result.lock().unwrap() = json!({"status":"recorded","messageState":{"version":2,"values":{"status":"closed","question":null}}});
    let mut from_card = invoke("ios");
    from_card["messageId"] = json!("client-1");
    let result = f.api.invoke_plugin_action(from_card).await.unwrap();
    assert_eq!(
        result["response"]["data"]["messageState"]["version"], 2,
        "the action result is returned unchanged"
    );
    match events.try_recv().expect("state change is announced") {
        SdkEvent::Message(MessageEvent::PluginStateChanged {
            conversation_id,
            message_id,
            version,
        }) => {
            assert_eq!(conversation_id, "conversation-1");
            assert_eq!(message_id, "server-1", "the server id is preferred");
            assert_eq!(version, 2);
        }
        other => panic!("unexpected event {other:?}"),
    }
    let presented = f
        .api
        .present_plugin_message(present("server-1"))
        .await
        .unwrap();
    assert_eq!(presented["stateVersion"], 2);
    assert_eq!(presented["data"]["status"], "closed");
    assert!(
        presented["data"].get("question").is_none(),
        "null removes a key"
    );
    let fields = presented["fields"].as_array().unwrap();
    assert!(fields.iter().any(|field| field["value"] == "closed"));
    assert!(fields.iter().all(|field| field["value"] != "Question"));

    // Stale, malformed, non-card, other-card and unknown-message results change nothing.
    for (state, message_id) in [
        (
            json!({"version":1,"values":{"status":"stale"}}),
            Some("server-1"),
        ),
        (
            json!({"version":9,"values":{"nested":{"a":1}}}),
            Some("server-1"),
        ),
        (json!({"version":9,"values":{"status":"ignored"}}), None),
        (
            json!({"version":9,"values":{"status":"other"}}),
            Some("server-2"),
        ),
        (
            json!({"version":9,"values":{"status":"missing"}}),
            Some("missing"),
        ),
    ] {
        *f.vote_result.lock().unwrap() = json!({"status":"recorded","messageState":state});
        let mut request = invoke("ios");
        if let Some(message_id) = message_id {
            request["messageId"] = json!(message_id);
        }
        f.api.invoke_plugin_action(request).await.unwrap();
    }
    assert!(events.try_recv().is_err());
    let stored = store.get("server-1").await.unwrap().unwrap();
    assert_eq!(stored.plugin_message_state().unwrap().version, 2);
    assert!(
        store
            .get("server-2")
            .await
            .unwrap()
            .unwrap()
            .plugin_message_state()
            .is_none()
    );

    // A different card never borrows this message's state.
    let mut foreign = present("server-1");
    foreign["message"]["contributionId"] = json!("example.poll.other");
    assert_eq!(
        f.api
            .present_plugin_message(present("server-2"))
            .await
            .unwrap()["stateVersion"],
        0
    );
    assert_eq!(
        f.api.present_plugin_message(foreign).await.unwrap()["stateVersion"],
        0
    );
    assert!(
        f.api
            .present_plugin_message(present("bad id"))
            .await
            .is_err()
    );
    f.catalog.lock().unwrap().plugins.clear();
    let unavailable = f
        .api
        .present_plugin_message(present("server-1"))
        .await
        .unwrap();
    assert_eq!(unavailable["available"], false);
    assert!(unavailable.get("data").is_none());
    assert_eq!(unavailable["stateVersion"], 2);
}

fn composer_contribution(
    id: &str,
    title: &str,
    extra: Value,
) -> flare_plugin_protocol::manifest::UiContribution {
    let mut raw = json!({
        "id": id, "kind": "composer-action", "schemaVersion": 1, "requiredPermissions": [],
        "schema": {"title": title, "translations": {"en-US": {"title": format!("{title} (en)"), "fields": [], "actions": {}}}}
    });
    for (key, value) in extra.as_object().unwrap() {
        raw[key] = value.clone();
    }
    serde_json::from_value(raw).unwrap()
}
#[tokio::test]
async fn composer_actions_list_authorized_entries_for_the_platform_in_order() {
    use flare_plugin_protocol::manifest::ClientPlatform;
    let f = Fixture::new().await;
    {
        let mut catalog = f.catalog.lock().unwrap();
        let contributions = &mut catalog.plugins[0].extensions.contributions;
        contributions.push(composer_contribution(
            "example.poll.later",
            "以后",
            json!({}),
        ));
        contributions.push(composer_contribution(
            "example.poll.create",
            "投票",
            json!({"icon": "poll", "order": 100}),
        ));
        // Older catalogs or a hand-made projection: the bad icon is dropped, not the entry.
        let mut odd = composer_contribution("example.poll.odd", "问卷", json!({"order": 100}));
        odd.icon = Some("Not An Icon".into());
        contributions.push(odd);
        let mut future = composer_contribution("example.poll.v2", "新版", json!({}));
        future.schema_version = 2;
        contributions.push(future);
        let mut web_only = catalog.plugins[0].clone();
        web_only.plugin_id = "example.webonly".into();
        web_only.extensions.platforms = vec![ClientPlatform::Web];
        web_only.extensions.contributions = vec![composer_contribution(
            "example.webonly.open",
            "网页",
            json!({"icon": "globe", "order": 1}),
        )];
        catalog.plugins.push(web_only);
    }
    let ios = f
        .api
        .composer_actions(ClientPlatform::Ios, "zh-CN")
        .await
        .unwrap();
    let listed: Vec<_> = ios
        .iter()
        .map(|a| {
            (
                a.contribution_id.as_str(),
                a.title.as_str(),
                a.icon.as_deref(),
                a.order,
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("example.poll.create", "投票", Some("poll"), 100),
            ("example.poll.odd", "问卷", None, 100),
            ("example.poll.later", "以后", None, 1000),
        ],
        "composer actions only, by order then title; message cards and unknown versions are not entries"
    );
    let web = f
        .api
        .composer_actions(ClientPlatform::Web, "en-GB")
        .await
        .unwrap();
    assert_eq!(web[0].plugin_id, "example.webonly");
    assert_eq!(web[1].title, "投票 (en)", "language fallback for the title");

    let bound = f
        .api
        .plugin_composer_actions(json!({"platform": "android", "locale": "en-US"}))
        .await
        .unwrap();
    assert_eq!(
        bound["actions"][0],
        json!({"pluginId":"example.poll","contributionId":"example.poll.create","title":"投票 (en)","icon":"poll","order":100})
    );
    assert!(bound["actions"][1].get("icon").is_none());
    for bad in [
        json!({}),
        json!({"platform": "desktop"}),
        json!({"platform": "ios", "tenantId": "202"}),
        json!({"platform": "ios", "locale": "x".repeat(33)}),
    ] {
        assert!(f.api.plugin_composer_actions(bad).await.is_err());
    }
    f.catalog.lock().unwrap().tenant_id = "202".into();
    assert!(
        f.api
            .composer_actions(ClientPlatform::Ios, "")
            .await
            .unwrap()
            .is_empty(),
        "another tenant's catalog lists nothing"
    );
    let calls = f.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .all(|c| c["capabilityId"] == flare_plugin_protocol::ui::UI_CATALOG_OPERATION),
        "listing reads the catalog only"
    );
    assert_eq!(
        calls.len(),
        4,
        "every listing fetches a fresh catalog; bad requests fetch none"
    );
}
