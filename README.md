# Flare IM Core SDK

English · **[中文](README.zh-CN.md)**

> ## ℹ️ This is communication infrastructure, not a turnkey IM product
>
> Up front, so you don't discover it only after cloning and failing to log in:
> **the open-source part ships no account system** (no sign-up/login, friend
> relationships, group roles/approval/muting, or moments/feed).
>
> What it does ship is a complete, pluggable authentication contract — both
> paths live on the open-source side:
>
> - **`CoreJwtTokenValidator`** — validates JWTs locally. Hand-sign a token and
>   you can run a demo / POC **without any user system at all**.
> - **`HttpHookTokenValidator`** — POSTs the token to your own endpoint. **This
>   is the entry point for wiring in your own user system.**
>
> Business rules work the same way: `flare-im-core/crates/flare-im-hooks`
> exposes 9 extension points (PreSend / PostSend / Delivery / Recall /
> MessageRead / MessageReaction / ConversationLifecycle / ConversationMember /
> GetConversationParticipants).
>
> To go to production you implement your own user system and plug it in via the
> contracts above — the same "bring your own identity" model as Sendbird /
> Twilio Conversations, except Flare can be self-hosted and its protocol and
> core are auditable.
>
> For the exact boundary, see [GOVERNANCE.md](GOVERNANCE.md).


[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`flare-im-core-sdk` is Flare IM's unified client core. The Rust core owns the
SDK lifecycle, local message/conversation projection, the sync-task entry
points, the event system, media records, capability packs, and extension
events; the C, Tauri, UniFFI, and Wasm bindings only translate the
ABI / IPC / language boundary.

## Core boundaries

| Layer | Responsibility |
| --- | --- |
| `flare-core` | Transport, connection, framing, negotiation, heartbeat, and other base communication primitives |
| `flare-proto` | The single wire contract — `DataPacket`, `Message`, `SyncRes`, `CapabilityPacket`, etc. |
| `flare-im-core-sdk` | Client IM behavior, offline local state, projection, the outbound queue, event routing, extension-capability entry points |
| bindings | Boundary adapters for C/Tauri/UniFFI/Wasm — they never re-implement core semantics |

## Current production modules

`src/lib.rs` re-exports only the current public contract. SDK behavior is placed
by boundary across `client`, `application`, `domain`, `core`,
`infrastructure`, `platform`, and `extension`; the old prototype route facade
is no longer the production contract.

| Module | Responsibility |
| --- | --- |
| `client/` | `IMClient` lifecycle, typed APIs, builder, login session, the cross-platform SDK entry |
| `application/` | Use-case orchestration, message/sync/presence/capability adapters, projection updates |
| `domain/` | Business invariants — messages, conversations, sync cursors, pending sends |
| `core/` | Event bus, dispatcher, reliable queue, sync orchestrator |
| `infrastructure/` | protobuf codec, packet sender, socket/http transport, memory/sqlite persistence |
| `platform/` | Media, transport, and host-capability ports |
| `extension/` | Capability registry, middleware, RTC/SFU capability-id helpers |

## Quick start

```rust
use flare_im_core_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = IMClient::new();
    client.init(Some("example".into()), None).await?;

    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: "insecure-secret".to_string(),
        issuer: "flare-im-core".to_string(),
        user_id: "user_a".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login("user_a", Some(&token), LoginDbKind::Sqlite, |_, _| {})
        .await?;

    let conversation = apis
        .conversation_api
        .get_one("peer_b", &flare_im_core_sdk::model::conversation::ConversationType::Single)
        .await?;
    let message = apis
        .message_build_api
        .create_text(&conversation.conversation_id, "hello", false)
        .await?;
    let ack = apis.message_api.send_no_oss(message).await?;
    println!("sent: {:?}", ack);
    Ok(())
}
```

## Binding runtime

Bindings all enter the current typed SDK through the `BindingRequest` of the
`bindings/shared` crate. This shared runtime only wires up routes that have been
explicitly migrated to vNext; the old prototype JSON routes are not revived for
compatibility.

```rust
let response = flare_im_core_sdk_bindings_runtime::invoke_json(
    &client,
    r#"{"route":"sdk.state","params":{}}"#,
).await;
```

Contract sources live in `bindings/contract/*.json`; after editing them, run
`rtk cargo xtask codegen` to regenerate the tables. `call_signal.proto` has been
removed — RTC/SFU is sent via `DataPacket.capability` and `rtc.*` capability ids.

## Event system

Events uniformly use `SdkEvent`:

- `EventBus::subscribe()` provides a typed Rust event stream.
- Message-mutation events come from the `event.proto` oneof payload;
  typing/presence/RTC do not consume `conversation_seq` and travel over the
  DATA realtime/capability channel.
- `bindings/contract/events.json` maintains cross-platform event ids, C codes,
  and `im://*` names.
- Custom business events go through `ExtensionEvent` or `MessageEvent::Custom`;
  their payload bytes are opaque to the core.

## Outbox and sync

- On send, the message is first written to a bounded pending outbox; while
  offline it is only enqueued, and it is sent immediately only once `Ready` and
  the transport is connected.
- Both `SendAck` accepted and error converge the local message state and remove
  the pending entry.
- When a realtime downstream message reveals a `conversation_seq` gap, the gap
  is recorded and back-filled via a single-conversation sync request.
- Conversation-list pagination uses the server's string cursor; the local
  adapter only parses the numeric watermark when needed.
- `SyncRes` / `EventEnvelope` fields are currently authoritative on
  `max_conversation_seq`, `next_cursor`, and the oneof payload.

## Bindings

| Binding | Directory | Verification |
| --- | --- | --- |
| C ABI | `bindings/c` | `cargo check -p flare-im-core-sdk-ffi --all-targets` |
| Tauri | `bindings/tauri` | `cargo check -p flare-im-core-sdk-tauri` |
| UniFFI | `bindings/uniffi` | `cargo check --manifest-path bindings/uniffi/Cargo.toml`; `cargo test --manifest-path bindings/uniffi/Cargo.toml` |
| Wasm | `bindings/wasm` | `cargo check -p flare-im-core-sdk-wasm --target wasm32-unknown-unknown` |

## Development verification

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-features --all-targets
rtk cargo test --workspace --all-features
rtk cargo check --manifest-path bindings/uniffi/Cargo.toml --all-features --tests
rtk cargo test --manifest-path bindings/uniffi/Cargo.toml --all-features
rtk cargo check -p flare-im-core-sdk-wasm --target wasm32-unknown-unknown --all-features
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Server integration testing

The local full-stack server is started by
`../flare-im-core/scripts/start_server.sh` and can be checked with
`../flare-im-core/scripts/check_services.sh`. The current live-gateway E2E needs
to be rebuilt on top of `SocketTransport` / `PacketSender` / the typed API; do
not restore the old `memory://local` route-facade tests.

```bash
rtk bash ../flare-im-core/scripts/check_services.sh
```

## License

Apache License 2.0. The underlying `flare-core` is MIT — see the license file in
its repository.

---

## Next steps

| What you want | Where to go |
|---|---|
| **Run it in five minutes** | [QUICKSTART](https://github.com/flare-im/flare-im-core-server/blob/main/QUICKSTART.md) — start the server, hand-sign a token, get a call through, **no self-built user system required** |
| Wire in your own user system | Implement `TokenValidator` (`CoreJwtTokenValidator` for local verification / `HttpHookTokenValidator` to call your endpoint) |
| Add your own business rules | The 9 `flare-im-hooks` extension points: PreSend / PostSend / Delivery / Recall / MessageRead / MessageReaction / ConversationLifecycle / ConversationMember / GetConversationParticipants |
| Build a UI | [`@flare-im/vue-ui`](https://www.npmjs.com/package/@flare-im/vue-ui) — 107 components, one contract consistent across four platforms |
| Report a security issue | [SECURITY.md](SECURITY.md) — **please do not open a public issue** |

## When you need an account system and social features

The open-source part is **communication infrastructure**. If what you need is a
ready-made account system, friend relationships, group governance (roles /
join approval / muting), or a moments/feed, those live in the commercial
modules — building this layer yourself usually takes months and is all
communication-unrelated, repetitive work.

Enterprise scenarios additionally cover SSO / org directory / audit export /
data residency / SLA support.

Inquiries: `flare1522@163.com`

> For the boundary and its immutable guarantees, see
> [GOVERNANCE](https://github.com/flare-im/flare-im-core-server/blob/main/GOVERNANCE.md).
> In short: **what is already open-sourced will not be taken back, and the auth
> and hooks contracts stay open forever — they will never be crippled to force
> payment.**
