use std::path::PathBuf;
use std::env;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // 生成头文件到 target 目录
    let output_file = PathBuf::from(&crate_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("flare_im_core_sdk_ffi.h");

    // 读取 cbindgen 配置
    let config_file = PathBuf::from(&crate_dir).join("cbindgen.toml");

    // 尝试生成 C 绑定，如果失败则跳过
    match cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(cbindgen::Config::from_file(&config_file).expect("Failed to read cbindgen.toml"))
        .generate()
    {
        Ok(bindings) => {
            // 写入头文件
            bindings.write_to_file(&output_file);
            println!("cargo:warning=Generated C header: {:?}", output_file);
        }
        Err(e) => {
            println!("cargo:warning=Failed to generate C header: {:?}", e);
            println!("cargo:warning=This is expected during initial build, header will be generated later");
        }
    }

    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
