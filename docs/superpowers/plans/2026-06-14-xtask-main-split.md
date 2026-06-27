# xtask main.rs Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Reduce `xtask/src/main.rs` to a small CLI/orchestration shell by moving verification, spec composition, code generation, snapshot, and shared naming helpers into focused modules.

**Architecture:** Keep behavior identical and move code by responsibility. Each move is protected by a layout test in `xtask/tests/module_layout_test.rs`, followed by `cargo test -p xtask`, `cargo xtask verify`, and `cargo xtask codegen-check`.

**Tech Stack:** Rust 2024, `anyhow`, `serde_json`, `regex`, existing `cargo xtask` commands.

---

### Task 1: Completed Baseline Modules

**Files:**
- Existing: `xtask/src/context.rs`
- Existing: `xtask/src/fs_util.rs`
- Existing: `xtask/src/verify/{mod.rs,spec.rs,core_contract.rs,structure.rs}`
- Existing: `xtask/src/codegen/{mod.rs,docs.rs,bridge.rs,wire_boundary.rs}`
- Existing: `xtask/tests/module_layout_test.rs`

- [x] Move workspace path/context helpers out of `main.rs`.
- [x] Move filesystem/upsert/command helpers out of `main.rs`.
- [x] Move `verify_spec`, `verify_core_contract`, and `verify_structure` entrypoints out of `main.rs`.
- [x] Move docs, bridge, and wire boundary generation out of `main.rs`.
- [x] Verify with `rtk cargo test -p xtask`, `rtk cargo xtask verify`, and `rtk cargo xtask codegen-check`.

### Task 2: Split Shared Spec/Core Contract Model

**Files:**
- Create: `xtask/src/spec_model.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/tests/module_layout_test.rs`

- [x] Add a layout test that requires `src/spec_model.rs` and rejects `struct CoreContract`, `struct ClientSpecOverlay`, and `struct ExpandedClientSpec` in `main.rs`.
- [x] Move `CoreAbiRef`, `CoreContract`, `ClientSpecOverlay`, `ExpandedClientSpec`, `native_local_operation_ids`, `compose_expanded_client_spec`, `enrich_native_bindings`, `native_binding_for`, `merge_object`, and message-builder expansion helpers into `spec_model.rs`.
- [x] Export only the APIs needed by `main.rs` and `verify/core_contract.rs`.
- [x] Run `rtk cargo test -p xtask`.

### Task 3: Split Naming And Spec Helpers

**Files:**
- Create: `xtask/src/codegen/naming.rs`
- Create: `xtask/src/spec_query.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/src/codegen/mod.rs`
- Modify: `xtask/tests/module_layout_test.rs`

- [x] Add layout tests that require `codegen/naming.rs` and `spec_query.rs`.
- [x] Move `pascal_case`, `snake_case`, `screaming_snake`, `camel_const`, `upper_first`, `lower_first`, `json_quote`, and platform identifier helpers into `codegen/naming.rs`.
- [x] Move `child_arr`, `all_spec_models`, `all_spec_enums`, `spec_model_names`, `spec_enum_names`, `spec_enum_map`, list type helpers, and `find_model` into `spec_query.rs`.
- [x] Run `rtk cargo test -p xtask`.

### Task 4: Split Contract Generators

**Files:**
- Create: `xtask/src/codegen/typescript_contract.rs`
- Create: `xtask/src/codegen/platform_contract.rs`
- Modify: `xtask/src/codegen/mod.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/tests/module_layout_test.rs`

- [x] Add layout tests that reject `emit_typescript_contract_files` and `emit_platform_contract_files` in `main.rs`.
- [x] Move TypeScript model/listener/callback contract generation into `typescript_contract.rs`.
- [x] Move Dart/Kotlin/Swift/ArkTS/Cangjie model/listener/callback contract generation into `platform_contract.rs`.
- [x] Keep `GeneratedTextTarget` shared until all generators are moved.
- [x] Run `rtk cargo test -p xtask`, `rtk cargo xtask codegen-check`.

### Task 5: Split API Generators

**Files:**
- Create: `xtask/src/codegen/platform_api.rs`
- Modify: `xtask/src/codegen/mod.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/tests/module_layout_test.rs`

- [x] Add a layout test that rejects `emit_platform_api_files` in `main.rs`.
- [x] Move platform API generation and API type mapping helpers into `platform_api.rs`.
- [x] Keep platform-specific API emitters in the same file unless it becomes too large after extraction.
- [x] Run `rtk cargo test -p xtask`, `rtk cargo xtask codegen-check`.

### Task 6: Split Adapter Generators

**Files:**
- Create: `xtask/src/codegen/typescript_adapter.rs`
- Create: `xtask/src/codegen/platform_adapter.rs`
- Modify: `xtask/src/codegen/mod.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/tests/module_layout_test.rs`

- [x] Add layout tests that reject `emit_typescript_adapter_files` and `emit_platform_adapter_files` in `main.rs`.
- [x] Move TypeScript adapter generation, wire codec generation, and event adapter helpers into `typescript_adapter.rs`.
- [x] Move Kotlin/Swift/ArkTS/Cangjie map/connection/message-builder adapter generation into `platform_adapter.rs`.
- [x] Run `rtk cargo test -p xtask`, `rtk cargo xtask codegen-check`.

### Task 7: Split Codegen Orchestration And Snapshot

**Files:**
- Create: `xtask/src/codegen/orchestrator.rs`
- Create: `xtask/src/codegen/snapshot.rs`
- Modify: `xtask/src/codegen/mod.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/tests/module_layout_test.rs`

- [x] Add layout tests that reject `run_codegen`, `generated_output_snapshot`, and `GENERATED_OUTPUT_PATHS` in `main.rs`.
- [x] Move `run_codegen` into `codegen/orchestrator.rs`.
- [x] Move `FileDigest`, `generated_output_snapshot`, `file_digest`, `should_skip_snapshot_file`, and `GENERATED_OUTPUT_PATHS` into `codegen/snapshot.rs`.
- [x] Run `rtk cargo test -p xtask`, `rtk cargo xtask verify`, `rtk cargo xtask codegen-check`.

### Task 8: Final Main Shell

**Files:**
- Modify: `xtask/src/main.rs`
- Modify: `xtask/tests/module_layout_test.rs`

- [x] Add a final layout test that fails if `main.rs` exceeds 400 lines or contains `fn emit_`, `fn verify_`, or large generator constants.
- [x] Keep only `main`, `run`, `print_help`, `clean`, CLI routing, and tiny command helpers in `main.rs`.
- [x] Run `rtk cargo fmt --all -- --check`.
- [x] Run `rtk cargo test -p xtask`.
- [x] Run `rtk cargo xtask verify`.
- [x] Run `rtk cargo xtask codegen-check`.
