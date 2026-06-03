# UniFFI Binding Placeholder

Status: archived.

This crate is not part of the current multi-platform SDK boundary.

Current decision:

- `bindings/c` is the only universal native L1 boundary.
- `bindings/tauri` is the Tauri desktop shell adapter.
- UniFFI is kept only as a placeholder for a future deliberate redesign.

Do not build client SDK packages on top of this directory today. Android, iOS,
Flutter, HarmonyOS, React Native native modules, Node native, and Unity should
consume `bindings/c` through their platform runtime adapters.

The package has its own empty `[workspace]` table so direct cargo commands do
not accidentally inherit the parent workspace while the crate remains excluded.
