# Flare IM Core SDK WASM Binding

This package is the browser L1 binding consumed by
`flare-core-typescript-sdk/web`.

It exports:

- `createWasmRuntime()`
- `FlareImWasmRuntime.invoke(operation, requestJson)`

Build:

```bash
cd flare-im-core-sdk/bindings/wasm
npm install
npm run build
```

The generated package is written to `bindings/wasm/pkg`.

Current browser runtime scope is intentionally small but real: session lifecycle,
diagnostics, in-memory conversations, text message build/send/list, and smoke-test
event-friendly responses. Web transport, IndexedDB stores, media host adapters,
presence transport, and capability plugins remain explicit follow-up runtime
ports rather than TypeScript reimplementations.

## Binding Design Rule

`bindings/wasm` must stay a thin platform adapter. Its responsibilities are:

- initialize wasm-bindgen exports
- translate `(operation, requestJson)` into a shared core runtime call
- map core errors into stable JS errors
- expose browser-specific lifecycle/dispose hooks

Conversation, message, sync, delivery, read-state, media, presence, and capability
behavior should live under `flare-im-core-sdk/src`. If behavior is missing in the
browser runtime, add a core facade/port in `src` first, then route this binding to
that facade. The in-memory runtime in this package is only a temporary smoke layer
until the shared core facade is wasm-ready.
