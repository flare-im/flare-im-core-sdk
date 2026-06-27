use std::fs;
use std::path::Path;

#[test]
fn xtask_keeps_context_and_file_utilities_out_of_main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(src.join("context.rs").is_file(), "missing src/context.rs");
    assert!(src.join("fs_util.rs").is_file(), "missing src/fs_util.rs");
    assert!(
        !main.contains("fn workspace_root("),
        "workspace root discovery belongs in context.rs"
    );
    assert!(
        !main.contains("fn files_under("),
        "filesystem traversal belongs in fs_util.rs"
    );
    assert!(
        !main.contains("fn upsert_text_file("),
        "file upsert helpers belong in fs_util.rs"
    );
}

#[test]
fn xtask_keeps_structure_verification_in_verify_module() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("verify").join("structure.rs").is_file(),
        "missing src/verify/structure.rs"
    );
    assert!(
        !main.contains("fn verify_structure("),
        "structure verification belongs in verify/structure.rs"
    );
    assert!(
        !main.contains("const REQUIRED_PATHS:"),
        "required path guard list belongs in verify/structure.rs"
    );
    assert!(
        !main.contains("const RETIRED_PATHS:"),
        "retired path guard list belongs in verify/structure.rs"
    );
}

#[test]
fn xtask_keeps_spec_verification_in_verify_module() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("verify").join("spec.rs").is_file(),
        "missing src/verify/spec.rs"
    );
    assert!(
        !main.contains("fn verify_spec("),
        "sdk-spec validation belongs in verify/spec.rs"
    );
}

#[test]
fn xtask_keeps_core_contract_verification_in_verify_module() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("verify").join("core_contract.rs").is_file(),
        "missing src/verify/core_contract.rs"
    );
    assert!(
        !main.contains("fn verify_core_contract("),
        "core contract parity verification belongs in verify/core_contract.rs"
    );
}

#[test]
fn xtask_keeps_document_generation_in_codegen_module() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("codegen").join("docs.rs").is_file(),
        "missing src/codegen/docs.rs"
    );
    assert!(
        !main.contains("fn emit_doc_files("),
        "documentation generation belongs in codegen/docs.rs"
    );
}

#[test]
fn xtask_keeps_bridge_generation_in_codegen_module() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("codegen").join("bridge.rs").is_file(),
        "missing src/codegen/bridge.rs"
    );
    assert!(
        !main.contains("fn emit_bridge_files("),
        "bridge generation belongs in codegen/bridge.rs"
    );
}

#[test]
fn xtask_keeps_wire_boundary_generation_in_codegen_module() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("codegen").join("wire_boundary.rs").is_file(),
        "missing src/codegen/wire_boundary.rs"
    );
    assert!(
        !main.contains("fn emit_wire_boundaries("),
        "wire boundary generation belongs in codegen/wire_boundary.rs"
    );
    assert!(
        !main.contains("const TS_WIRE_BOUNDARY:"),
        "wire boundary templates belong in codegen/wire_boundary.rs"
    );
}

#[test]
fn xtask_keeps_spec_model_out_of_main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("spec_model.rs").is_file(),
        "missing src/spec_model.rs"
    );
    assert!(
        !main.contains("struct CoreContract"),
        "core contract model belongs in spec_model.rs"
    );
    assert!(
        !main.contains("struct ClientSpecOverlay"),
        "client spec overlay belongs in spec_model.rs"
    );
    assert!(
        !main.contains("struct ExpandedClientSpec"),
        "expanded client spec belongs in spec_model.rs"
    );
}

#[test]
fn xtask_keeps_spec_query_and_naming_helpers_out_of_main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("spec_query.rs").is_file(),
        "missing src/spec_query.rs"
    );
    assert!(
        src.join("codegen").join("naming.rs").is_file(),
        "missing src/codegen/naming.rs"
    );
    for needle in [
        "fn arr(",
        "fn child_arr(",
        "fn all_spec_models(",
        "fn spec_enum_map(",
        "fn find_model(",
        "fn pascal_case(",
        "fn snake_case(",
        "fn json_quote(",
    ] {
        assert!(
            !main.contains(needle),
            "{needle} belongs in spec_query.rs or codegen/naming.rs"
        );
    }
}

#[test]
fn xtask_keeps_contract_generators_in_codegen_modules() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("codegen").join("typescript_contract.rs").is_file(),
        "missing src/codegen/typescript_contract.rs"
    );
    assert!(
        src.join("codegen").join("platform_contract.rs").is_file(),
        "missing src/codegen/platform_contract.rs"
    );
    assert!(
        !main.contains("fn emit_typescript_contract_files("),
        "TypeScript contract generation belongs in codegen/typescript_contract.rs"
    );
    assert!(
        !main.contains("fn emit_platform_contract_files("),
        "platform contract generation belongs in codegen/platform_contract.rs"
    );
}

#[test]
fn xtask_keeps_platform_api_generation_in_codegen_module() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("codegen").join("platform_api.rs").is_file(),
        "missing src/codegen/platform_api.rs"
    );
    assert!(
        !main.contains("fn emit_platform_api_files("),
        "platform API generation belongs in codegen/platform_api.rs"
    );
    assert!(
        !main.contains("fn platform_api_targets("),
        "platform API target selection belongs in codegen/platform_api.rs"
    );
}

#[test]
fn xtask_keeps_adapter_generators_in_codegen_modules() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("codegen").join("typescript_adapter.rs").is_file(),
        "missing src/codegen/typescript_adapter.rs"
    );
    assert!(
        src.join("codegen").join("platform_adapter.rs").is_file(),
        "missing src/codegen/platform_adapter.rs"
    );
    assert!(
        !main.contains("fn emit_typescript_adapter_files("),
        "TypeScript adapter generation belongs in codegen/typescript_adapter.rs"
    );
    assert!(
        !main.contains("fn emit_platform_adapter_files("),
        "platform adapter generation belongs in codegen/platform_adapter.rs"
    );
}

#[test]
fn xtask_keeps_codegen_orchestration_and_snapshot_in_codegen_modules() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    assert!(
        src.join("codegen").join("orchestrator.rs").is_file(),
        "missing src/codegen/orchestrator.rs"
    );
    assert!(
        src.join("codegen").join("snapshot.rs").is_file(),
        "missing src/codegen/snapshot.rs"
    );
    assert!(
        !main.contains("fn run_codegen("),
        "codegen orchestration belongs in codegen/orchestrator.rs"
    );
    assert!(
        !main.contains("fn generated_output_snapshot("),
        "generated output snapshots belong in codegen/snapshot.rs"
    );
    assert!(
        !main.contains("const GENERATED_OUTPUT_PATHS:"),
        "generated output path list belongs in codegen/snapshot.rs"
    );
}

#[test]
fn xtask_main_stays_a_small_cli_shell() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");
    let main = fs::read_to_string(src.join("main.rs")).expect("failed to read xtask main.rs");

    let line_count = main.lines().count();
    assert!(
        line_count <= 400,
        "xtask main.rs should stay a small CLI shell, got {line_count} lines"
    );
    for needle in [
        "fn emit_",
        "fn verify_",
        "struct FileDigest",
        "struct CoreContract",
    ] {
        assert!(
            !main.contains(needle),
            "{needle} belongs in a focused xtask module, not main.rs"
        );
    }
}

#[test]
fn xtask_owns_core_codegen_without_a_second_codegen_crate() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core_root = manifest_dir
        .parent()
        .expect("xtask should live under the core SDK workspace");
    let src = manifest_dir.join("src");
    let context =
        fs::read_to_string(src.join("context.rs")).expect("failed to read xtask context.rs");
    let workspace = fs::read_to_string(core_root.join("Cargo.toml"))
        .expect("failed to read workspace Cargo.toml");

    assert!(
        src.join("core_codegen.rs").is_file(),
        "core binding/schema codegen belongs in xtask/src/core_codegen.rs"
    );
    assert!(
        !core_root.join("codegen").join("Cargo.toml").exists(),
        "do not reintroduce a standalone codegen crate"
    );
    assert!(
        !core_root
            .join("codegen")
            .join("src")
            .join("main.rs")
            .exists(),
        "do not reintroduce a standalone codegen binary"
    );
    assert!(
        !context.contains("run_core_xtask"),
        "xtask should call core codegen in-process, not spawn another cargo package"
    );
    assert!(
        !workspace.contains("\"codegen\""),
        "workspace members should not include the retired standalone codegen crate"
    );
}
