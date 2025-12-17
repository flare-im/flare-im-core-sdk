use std::env;

fn main() {
    // 只有在启用 ffi feature 时才生成 C 头文件
    if env::var("CARGO_FEATURE_FFI").is_ok() {
        let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

        // 尝试生成 C 头文件，如果失败则只输出警告
        match cbindgen::Builder::new()
            .with_crate(crate_dir)
            .with_config(cbindgen::Config::from_file("cbindgen.toml").unwrap_or_default())
            .generate()
        {
            Ok(bindings) => {
                bindings.write_to_file("target/flare_im_core_sdk.h");
                println!("cargo:warning=Generated C header: target/flare_im_core_sdk.h");
            }
            Err(e) => {
                // cbindgen 解析失败时，只输出警告，不阻止构建
                // 这可能是因为某些 Rust 特性（如 async/await、宏等）导致 cbindgen 无法解析
                println!(
                    "cargo:warning=Failed to generate C bindings: {}. This is non-fatal.",
                    e
                );
                println!(
                    "cargo:warning=You may need to manually create the C header file or use a different approach."
                );
            }
        }

        println!("cargo:warning=Generated C header: target/flare_im_core_sdk.h");
    }

    // 重新生成当源文件变化时
    println!("cargo:rerun-if-changed=src/ffi/");
    println!("cargo:rerun-if-changed=cbindgen.toml");
}
