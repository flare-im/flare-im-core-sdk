# Flare IM C FFI — 静态库（staticlib）构建
#
# 本 crate 属于 workspace：`flare-im-core-sdk`，产物在 workspace 根目录的 `target/`，
# 而非 `bindings/c/target/`。
#
# 用法（在 `bindings/c` 目录）:
#   make                    # 本机 host triple，release 静态库
#   make ios-sim            # Apple Silicon 模拟器 (aarch64-apple-ios-sim)
#   make ios-device         # 真机 (aarch64-apple-ios)
#   make android            # aarch64-linux-android（需 ANDROID_NDK_ROOT）
#   make all-mobile         # iOS 三目标 + Android（需 NDK）
#   make clean
#
# 可选: export CARGO_TARGET_DIR=/path/to/custom-target

SHELL := /bin/bash
MAKEFILE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
# workspace 根目录（bindings/c -> ../..）
WORKSPACE_ROOT := $(abspath $(MAKEFILE_DIR)/../..)

PACKAGE := flare-im-core-sdk-ffi
LIB_NAME := flare_im_core_sdk_ffi
MANIFEST := $(MAKEFILE_DIR)/Cargo.toml

CARGO ?= cargo
PROFILE ?= release

# 未设置时使用 workspace 默认 target 目录
CARGO_TARGET_DIR ?= $(WORKSPACE_ROOT)/target

# 静态库相对 $(CARGO_TARGET_DIR) 的路径
# $(1) = rust triple
define static_lib_path
$(CARGO_TARGET_DIR)/$(1)/$(PROFILE)/lib$(LIB_NAME).a
endef

HOST_TRIPLE := $(shell rustc -vV | sed -n 's/^host: //p')

.PHONY: all help setup-targets host ios-device ios-sim ios-sim-x86 ios-all android \
	all-mobile dist dist-ios dist-android clean check

help:
	@echo "Flare FFI staticlib ($(PACKAGE))"
	@echo "  make / make host     release staticlib for host ($(HOST_TRIPLE))"
	@echo "  make ios-device      aarch64-apple-ios"
	@echo "  make ios-sim         aarch64-apple-ios-sim"
	@echo "  make ios-sim-x86     x86_64-apple-ios (Intel 模拟器)"
	@echo "  make ios-all         上述三个 iOS 目标"
	@echo "  make android         aarch64-linux-android (需 ANDROID_NDK_ROOT)"
	@echo "  make all-mobile      ios-all + android"
	@echo "  make dist            dist-ios（复制 iOS 三目标 .a 到 dist/）"
	@echo "  make dist-android    复制 aarch64-linux-android .a（需先 make android）"
	@echo "  make clean           cargo clean -p $(PACKAGE)"
	@echo "Workspace target dir: $(CARGO_TARGET_DIR)"

all: host

host:
	@echo "[FFI] host $(HOST_TRIPLE) ($(PROFILE))"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" \
		-p $(PACKAGE)
	@echo "-> $(call static_lib_path,$(HOST_TRIPLE))"

setup-targets:
	rustup target add aarch64-apple-ios || true
	rustup target add aarch64-apple-ios-sim || true
	rustup target add x86_64-apple-ios || true
	rustup target add aarch64-linux-android || true

ios-device: setup-targets
	@echo "[FFI] aarch64-apple-ios ($(PROFILE))"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" \
		--target aarch64-apple-ios \
		-p $(PACKAGE)
	@echo "-> $(call static_lib_path,aarch64-apple-ios)"

ios-sim: setup-targets
	@echo "[FFI] aarch64-apple-ios-sim ($(PROFILE))"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" \
		--target aarch64-apple-ios-sim \
		-p $(PACKAGE)
	@echo "-> $(call static_lib_path,aarch64-apple-ios-sim)"

ios-sim-x86: setup-targets
	@echo "[FFI] x86_64-apple-ios ($(PROFILE))"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" \
		--target x86_64-apple-ios \
		-p $(PACKAGE)
	@echo "-> $(call static_lib_path,x86_64-apple-ios)"

ios-all: ios-device ios-sim ios-sim-x86

android: setup-targets
	@if [ -z "$$ANDROID_NDK_ROOT" ]; then \
		echo "[ERROR] 请设置 ANDROID_NDK_ROOT（Android NDK 根目录）"; \
		echo "  例如: export ANDROID_NDK_ROOT=\"\$$HOME/Library/Android/sdk/ndk/26.1.10909125\""; \
		exit 2; \
	fi
	@echo "[FFI] aarch64-linux-android ($(PROFILE))"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" \
		--target aarch64-linux-android \
		-p $(PACKAGE)
	@echo "-> $(call static_lib_path,aarch64-linux-android) (若需 .so 见 cdylib 同目录)"

all-mobile: ios-all android

# 将静态库复制到 bindings/c/dist/<triple>/，便于打包脚本引用
dist: dist-ios

dist-ios: ios-all
	@set -e; \
	for t in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do \
		src="$(CARGO_TARGET_DIR)/$$t/$(PROFILE)/lib$(LIB_NAME).a"; \
		if [ -f "$$src" ]; then \
			mkdir -p "$(MAKEFILE_DIR)/dist/$$t"; \
			cp -f "$$src" "$(MAKEFILE_DIR)/dist/$$t/"; \
			echo "[dist] $$src -> dist/$$t/"; \
		fi; \
	done

dist-android: android
	@src="$(CARGO_TARGET_DIR)/aarch64-linux-android/$(PROFILE)/lib$(LIB_NAME).a"; \
	if [ -f "$$src" ]; then \
		mkdir -p "$(MAKEFILE_DIR)/dist/aarch64-linux-android"; \
		cp -f "$$src" "$(MAKEFILE_DIR)/dist/aarch64-linux-android/"; \
		echo "[dist] $$src -> dist/aarch64-linux-android/"; \
	else \
		echo "[dist-android] 未找到 $$src，请先成功执行 make android"; \
		exit 1; \
	fi

check:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) check --manifest-path "$(MANIFEST)" -p $(PACKAGE)

clean:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) clean -p $(PACKAGE)
	rm -rf "$(MAKEFILE_DIR)/dist"
