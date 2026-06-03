# Flare IM Core SDK Tauri Binding

`bindings/tauri` is the active desktop shell adapter for Tauri apps.

It is not the universal native SDK boundary. Mobile, Flutter, React Native
native modules, Harmony, Node native, Unity, and generic native desktop wrappers
should use `bindings/c`.

## Contract

Canonical API, event, and error contracts live in:

```text
../contract/manifest.json
../contract/apis.json
../contract/events.json
../contract/errors.json
```

Tauri command names and `im://*` event names must stay aligned with those files.

## Shape

```text
bindings/tauri/
  src/
    commands/
      lifecycle.rs
      message.rs
      conversation/
      media.rs
      presence.rs
      capability.rs
      call_signal.rs
      rich_doc_v2.rs
    convert.rs      SdkEvent -> im://* event mapping
    model.rs        Tauri IPC payload models
    state.rs        IMClient session holder
    lib.rs          im_invoke_handler()
```

## Usage

Register the shared state and invoke handler:

```rust
tauri::Builder::default()
    .manage(flare_im_core_sdk_tauri::SdkState::new())
    .invoke_handler(flare_im_core_sdk_tauri::im_invoke_handler())
    .run(tauri::generate_context!())?;
```

Frontend calls use snake_case payload fields, matching core-sdk serialization:

```ts
await invoke("sdk_init", {
  args: {
    environment: "development",
    sdk_config: { ws_url: "ws://localhost:60051" }
  }
});

await invoke("sdk_login", {
  user_id: "u1",
  token: "..."
});
```

## Events

`sdk_login` installs an EventBus forwarder. Events are emitted with the
`im://*` names listed in `../contract/events.json`, including:

```text
im://connected
im://disconnected
im://state
im://sync_state_changed
im://message
im://message_batch
im://send_ack
im://send_failed
im://message_recalled
im://message_edited
im://message_reaction_changed
im://message_deleted
im://message_read_receipt
im://message_burn_scheduled
im://message_burned
im://message_hard_deleted
im://message_pinned
im://message_unpinned
im://message_marked
im://message_unmarked
im://presence_changed
im://typing
im://call_signal
im://message_custom_event
im://notification
im://conversation_created
im://conversation_updated
im://conversation_deleted
im://unread_count_changed
im://conversations_synced
im://sync_started
im://sync_finished
im://sync_failed
im://sync_progress
im://sync_completed
im://extension
```

## Rules

- Do not implement IM business rules in this crate.
- Keep command payloads snake_case at the Rust boundary.
- Keep event mapping in `convert.rs` exhaustive for `SdkEvent`.
- Add or rename commands only with matching updates to `../contract/apis.json`.
- Add or rename events only with matching updates to `../contract/events.json`.
- Keep callback/UI-thread decisions in the host app or client SDK, not here.
