use anyhow::Result;
use std::{fs, path::Path};

use crate::{core_root, emit_errors, fail};

pub(crate) fn verify_network_reconnect_gate(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    let sdk_root = core_root(root);

    require_contains_all(
        &mut errors,
        &root.join("docs/client-network-reconnect.md"),
        "network reconnect doc",
        &[
            "client.connection.notifyNetworkChange",
            "connection.notify_network_change",
            "active reconnect",
            "single-flight",
        ],
    );

    require_contains_all(
        &mut errors,
        &sdk_root.join("src/client/im_client/session_lifecycle.rs"),
        "network reconnect runtime",
        &[
            "pub async fn notify_network_change",
            "reconnect_snapshot",
            "reconnect_current_engine",
            "try_begin_network_reconnect",
            "finish_network_reconnect",
            "network change reconnect already in flight; coalescing event",
            "network change reported; proactively reconnecting SDK session",
        ],
    );

    require_contains_all(
        &mut errors,
        &sdk_root.join("bindings/contract/direct_invoke.json"),
        "network reconnect direct invoke contract",
        &[
            "\"route\": \"connection.notify_network_change\"",
            "NetworkChangeEvent",
            "client.notify_network_change(event).await",
        ],
    );

    require_contains_all(
        &mut errors,
        &root.join("sdk-spec/modules/connection.json"),
        "network reconnect SDK API contract",
        &[
            "\"name\": \"notifyNetworkChange\"",
            "\"operation\": \"connection.notify_network_change\"",
            "\"request\": \"NetworkChangeRequest\"",
            "\"response\": \"NetworkChangeResponse\"",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join("sdk-spec/models/connection.json"),
        "network reconnect SDK model contract",
        &[
            "\"name\": \"NetworkChangeRequest\"",
            "\"name\": \"NetworkInterfaceKind\"",
            "\"wifi\"",
            "\"cellular\"",
            "\"ethernet\"",
            "\"name\": \"available\"",
            "\"name\": \"interface\"",
            "\"type\": \"NetworkInterfaceKind\"",
            "\"name\": \"NetworkChangeResponse\"",
            "\"name\": \"reconnected\"",
        ],
    );

    require_contains_all(
        &mut errors,
        &root.join("packages/flare-core-flutter-sdk/lib/src/adapter/codec/wire_codec.dart"),
        "Flutter network reconnect codec",
        &[
            "Map<String, Object?> networkChangeRequestToMap(NetworkChangeRequest request)",
            "'interface': request.interface!.name",
            "NetworkChangeResponse networkChangeResponseFromJson",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join(
            "packages/flare-core-apple-sdk/Sources/FlareCoreAppleSDK/Adapter/Codec/WireCodec.swift",
        ),
        "Apple network reconnect codec",
        &[
            "func networkChangeRequestToMap(_ request: NetworkChangeRequest)",
            "\"interface\": wrapSendable(request.interface?.rawValue)",
            "func networkChangeResponseToMap(_ request: NetworkChangeResponse)",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join(
            "packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/model/command/NetworkChangeRequest.kt",
        ),
        "Android network reconnect model",
        &["val `interface`: NetworkInterfaceKind? = null"],
    );
    require_contains_all(
        &mut errors,
        &root.join(
            "packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/adapter/codec/WireCodec.kt",
        ),
        "Android network reconnect codec",
        &[
            "fun networkInterfaceKindWireValue(value: NetworkInterfaceKind): String",
            "request.`interface`?.let { put(\"interface\", networkInterfaceKindWireValue(it)) }",
            "fun networkChangeResponseFromJson(value: Any?): NetworkChangeResponse",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join(
            "packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/adapter/module/DefaultConnectionApi.kt",
        ),
        "Android network reconnect adapter",
        &[
            "override suspend fun notifyNetworkChange(request: NetworkChangeRequest): NetworkChangeResponse",
            "networkChangeResponseFromJson(invokeMap(bridge, NativeCallMap.CONNECTION_NOTIFY_NETWORK_CHANGE, networkChangeRequestToMap(request)))",
        ],
    );
    require_contains_all(
        &mut errors,
        &root
            .join("packages/flare-core-harmony-arkts-sdk/src/main/ets/adapter/codec/WireCodec.ets"),
        "ArkTS network reconnect codec",
        &[
            "export function networkChangeRequestToMap(request: NetworkChangeRequest)",
            "interface: request.interface",
            "export function networkChangeResponseFromJson(value: Object | undefined): NetworkChangeResponse",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join(
            "packages/flare-core-harmony-arkts-sdk/src/main/ets/adapter/module/DefaultConnectionApi.ets",
        ),
        "ArkTS network reconnect adapter",
        &[
            "async notifyNetworkChange(request: NetworkChangeRequest): Promise<NetworkChangeResponse>",
            "networkChangeResponseFromJson(raw)",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join("packages/flare-core-harmony-cangjie-sdk/src/adapter/codec/WireCodec.cj"),
        "Cangjie network reconnect codec",
        &[
            "public func networkInterfaceKindWireValue(value: NetworkInterfaceKind): String",
            "jsonPutString(json, \"interface\", networkInterfaceKindWireValue(value))",
            "public func networkChangeResponseFromJson(raw: String): NetworkChangeResponse",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join(
            "packages/flare-core-harmony-cangjie-sdk/src/adapter/module/DefaultConnectionApi.cj",
        ),
        "Cangjie network reconnect adapter",
        &[
            "public func notifyNetworkChange(request: NetworkChangeRequest): NetworkChangeResponse",
            "networkChangeResponseFromJson(raw)",
        ],
    );

    require_contains_all(
        &mut errors,
        &sdk_root.join("src/client/im_client/tests.rs"),
        "network reconnect regression test",
        &[
            "network_change_is_noop_without_session",
            "network_change_reconnect_is_single_flight",
            "notify_network_change(NetworkChangeEvent",
        ],
    );

    emit_errors("network reconnect gate", errors)
}

fn require_contains_all(errors: &mut Vec<String>, path: &Path, label: &str, needles: &[&str]) {
    let Ok(text) = fs::read_to_string(path) else {
        fail(
            errors,
            format!("{label} missing or unreadable: {}", path.display()),
        );
        return;
    };

    for needle in needles {
        if !text.contains(needle) {
            fail(
                errors,
                format!("{label} missing `{needle}` in {}", path.display()),
            );
        }
    }
}
