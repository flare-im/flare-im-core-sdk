# Flare IM Bindings Contract

This directory defines the L1 binding contract consumed by
`flare-im-core-client-sdk`.

## Layering

```text
L3 typed platform SDK
  packages/flare-core-*-sdk
L2 runtime adapter
  native artifact loading, thread dispatch, platform bridge
L1 binding contract
  flare-im-core-sdk/bindings
L0 Rust core
  flare-im-core-sdk/src
```

## Current Decision

`bindings/c` is the only universal native L1 boundary.

`bindings/tauri` is active, but it is a desktop shell adapter for Tauri command
IPC. It should not be treated as the native SDK source of truth.

`bindings/uniffi` is archived. It remains in the tree as historical context and
does not participate in the current multi-platform SDK contract.

`bindings/wasm` is planned but not active. The core crate can compile for
`wasm32-unknown-unknown`, but browser IM runtime support is still blocked by Web
transport, HTTP, media, presence, and storage adapters.

## Client SDK Consumption

`flare-im-core-client-sdk` should read `manifest.json` as the binding source of
truth for:

- the active FFI contract version
- which L1 binding each platform should use
- canonical API ids and their C/Tauri entrypoints
- canonical SDK events and their C/Tauri event names
- stable C ABI error codes
- whether a binding is active, shell-only, or archived
- callback, event, memory, and error rules
- where native artifacts should be loaded from

Machine-readable contract files:

```text
manifest.json   binding selection, platform map, ownership rules
apis.json       canonical API surface and entrypoint mapping
events.json     canonical event ids, C event codes, Tauri event names
errors.json     stable error codes and typed-error mapping rules
```

Platform packages may expose idiomatic APIs, but their behavior must stay thin:
validate input, call L1, map output, emit typed events, and dispose resources.

## C ABI Rules

- Handles are opaque `u64` values.
- Complex inputs and outputs use UTF-8 JSON.
- JSON field names stay aligned with core-sdk snake_case serialization.
- Async functions return immediate submit status and invoke callback exactly
  once.
- Callback thread is unspecified.
- Rust-allocated `FlareString`, `FlareBytes`, and `FlareError` values must be
  released with the paired `flare_*_free` function.
- Event subscription returns a subscription handle that must be explicitly
  unsubscribed.

## Platform Map

Use `manifest.json.platformMap`.

Native mobile, Flutter, Harmony, React Native native modules, Node native, and
Unity should use `bindings/c`.

Tauri should use `bindings/tauri`.

Browser Web and pure uni-app Web targets do not load the C ABI. They should use
the TypeScript runtime adapter today. A wasm binding may become active only
after the Web runtime ports listed in `../wasm/README.md` are implemented.

## API Shape

Every platform SDK should preserve the same conceptual modules from
`apis.json`:

```text
client.init
client.login
client.logout
client.connection
client.events
client.conversations
client.messages
client.media
client.presence
client.capabilities
client.dispose
```

The C ABI function names may be lower-level, but L3 SDK packages should expose
canonical platform-friendly names such as `sendMessage`, `listConversations`,
`subscribeEvents`, and `dispose`.

## Event Shape

Every platform SDK should preserve the event ids from `events.json`.

Native SDKs should use `event_type` for fast dispatch and parse `event_json` for
payload. Tauri/Web adapters should use the `im://*` event names listed in
`events.json`.

## Error Shape

Every platform SDK should preserve the typed error mapping from `errors.json`.

Do not parse `FlareError.message`; it is for display and logs only.
