PACKAGE_ID=flare-im-core-sdk
CRATE_LIB=flare_im_core_sdk

.PHONY: setup-targets build-desktop build-web build-ios build-android build-all clean

setup-targets:
	rustup target add wasm32-unknown-unknown || true
	rustup target add aarch64-apple-ios || true
	rustup target add aarch64-linux-android || true
	rustup target add armv7-linux-androideabi || true
	rustup target add x86_64-unknown-linux-gnu || true


build-desktop:
	cargo build --release -p $(PACKAGE_ID)
	mkdir -p dist/desktop
	cp -f target/release/lib$(CRATE_LIB).dylib dist/desktop/ 2>/dev/null || true
	cp -f target/release/lib$(CRATE_LIB).a dist/desktop/ 2>/dev/null || true
	cp -f target/release/lib$(CRATE_LIB).rlib dist/desktop/ 2>/dev/null || true

build-web:
	RUSTFLAGS="--cfg wasm_js" cargo build --release --target wasm32-unknown-unknown -p $(PACKAGE_ID)
	mkdir -p dist/web
	cp -f target/wasm32-unknown-unknown/release/$(CRATE_LIB).wasm dist/web/ 2>/dev/null || true

build-ios:
	cargo build --release --target aarch64-apple-ios -p $(PACKAGE_ID)
	mkdir -p dist/ios
	cp -f target/aarch64-apple-ios/release/lib$(CRATE_LIB).a dist/ios/ 2>/dev/null || true

build-android:
	@if [ -z "$$ANDROID_NDK_ROOT" ]; then \
		printf "[ERROR] ANDROID_NDK_ROOT 未设置。请安装 NDK 并导出 ANDROID_NDK_ROOT 环境变量\n"; \
		printf "例如: export ANDROID_NDK_ROOT=\"$$HOME/Library/Android/sdk/ndk/26.1.10909125\"\n"; \
		exit 2; \
	fi
	@rustup target list | grep -q "aarch64-linux-android (installed)" || { \
		printf "[INFO] 安装 Rust Android 目标 aarch64-linux-android...\n"; \
		rustup target add aarch64-linux-android || exit 2; \
	}
	@printf "[INFO] 开始 Android 构建 (aarch64-linux-android)\n"
	cargo build --release --target aarch64-linux-android -p $(PACKAGE_ID)
	mkdir -p dist/android
	cp -f target/aarch64-linux-android/release/lib$(CRATE_LIB).so dist/android/ 2>/dev/null || true

build-all: build-desktop build-web build-ios build-android


clean:
	cargo clean -p $(PACKAGE_ID)
	rm -rf dist
