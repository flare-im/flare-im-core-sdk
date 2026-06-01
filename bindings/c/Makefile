# Flare IM C FFI — workspace `flare-im-core-sdk`，产物在 `$(WORKSPACE_ROOT)/target/`。
# Flutter 示例：`../examples/flare-core-flutter`（`make flutter-sync` 拷 dylib / .a / .so）。
#
# 常用：make host | make flutter-sync | make flutter-sync-ios-device
# 可选：CARGO_TARGET_DIR=...  ANDROID_NDK_ROOT=...（Android）

SHELL := /bin/bash
MAKEFILE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
WORKSPACE_ROOT := $(abspath $(MAKEFILE_DIR)/../..)
# 可由 flare-im-core-client-sdk/tools/sync_native.sh 覆盖
FLUTTER_EXAMPLE_DIR ?= $(abspath $(WORKSPACE_ROOT)/../examples/flare-core-flutter)
FLUTTER_EXAMPLE_DIR := $(abspath $(FLUTTER_EXAMPLE_DIR))
FLUTTER_IOS_FFI := $(FLUTTER_EXAMPLE_DIR)/ios/FFI/build
FLUTTER_ANDROID_JNI := $(FLUTTER_EXAMPLE_DIR)/android/app/src/main/jniLibs/arm64-v8a
FLUTTER_MACOS_RUNNER := $(FLUTTER_EXAMPLE_DIR)/macos/Runner

PACKAGE := flare-im-core-sdk-ffi
LIB_NAME := flare_im_core_sdk_ffi
MANIFEST := $(MAKEFILE_DIR)/Cargo.toml
CARGO ?= cargo
PROFILE ?= release
CARGO_TARGET_DIR ?= $(WORKSPACE_ROOT)/target

# 未指定 --target 的 host 构建：target/<profile>/（不是 target/<triple>/<profile>/）
HOST_OUT := $(CARGO_TARGET_DIR)/$(PROFILE)
HOST_TRIPLE := $(shell rustc -vV | sed -n 's/^host: //p')

# $(1) = 交叉编译 triple
define triple_out
$(CARGO_TARGET_DIR)/$(1)/$(PROFILE)
endef
define triple_lib_a
$(call triple_out,$(1))/lib$(LIB_NAME).a
endef
define triple_lib_so
$(call triple_out,$(1))/lib$(LIB_NAME).so
endef

OHOS_TARGET := aarch64-unknown-linux-ohos
# OpenHarmony SDK / NDK 根目录（含 native/llvm/bin/aarch64-unknown-linux-ohos-clang）
OHOS_NDK_HOME ?=
HARMONY_LIBS_DIR ?= $(abspath $(WORKSPACE_ROOT)/../flare-im-core-client-sdk/packages/harmony/arkts/flare-im-harmony-arkts/libs/arm64-v8a)

.PHONY: all help help-flutter host host-flutter flutter-sync flutter-sync-ios \
	flutter-sync-ios-universal flutter-sync-ios-device flutter-sync-android \
	flutter-sync-harmony setup-targets ios-device ios-sim ios-sim-x86 ios-all android ohos \
	all-mobile dist dist-ios dist-android clean check

all: host

help:
	@echo "FFI $(PACKAGE)  workspace=$(WORKSPACE_ROOT)"
	@echo "  make host / make              本机 release（产物在 target/$(PROFILE)/）"
	@echo "  make flutter-sync           host-flutter +（macOS）iOS 模拟器 .a +（可选）Android .so"
	@echo "  make flutter-sync-ios-device  真机 .a → Flutter ios/FFI/build/"
	@echo "  make flutter-sync-ios-universal  lipo 双模拟器 .a"
	@echo "  make ios-all | make android | make ohos | make dist-ios"
	@echo "  make flutter-sync-harmony  需 OHOS_NDK_HOME，拷贝 .so 至 client-sdk 鸿蒙 libs"
	@echo "  make help-flutter           打印 Flutter 路径"
	@echo "  target dir: $(CARGO_TARGET_DIR)"

help-flutter:
	@echo "$(FLUTTER_EXAMPLE_DIR)"
	@test -d "$(FLUTTER_EXAMPLE_DIR)" && echo "(存在)" || echo "(不存在)"

host:
	@echo "[FFI] host $(HOST_TRIPLE) $(PROFILE)"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" -p $(PACKAGE)
	@echo "-> $(HOST_OUT)/lib$(LIB_NAME).a (+ cdylib 同目录)"

# macOS：cdylib 在 HOST_OUT（cargo 未传 --target）；若曾传 --target=$(HOST_TRIPLE) 则查 triple 子目录
host-flutter: host
	@test -d "$(FLUTTER_EXAMPLE_DIR)" || (echo "[ERROR] 无 Flutter 目录: $(FLUTTER_EXAMPLE_DIR)"; exit 1)
	@if [ "$$(uname -s)" != "Darwin" ]; then echo "[host-flutter] 非 macOS，跳过"; exit 0; fi; \
	mkdir -p "$(FLUTTER_MACOS_RUNNER)"; \
	DY="$(HOST_OUT)/lib$(LIB_NAME).dylib"; \
	if [ ! -f "$$DY" ]; then DY="$(CARGO_TARGET_DIR)/$(HOST_TRIPLE)/$(PROFILE)/lib$(LIB_NAME).dylib"; fi; \
	if [ ! -f "$$DY" ]; then echo "[ERROR] 无 dylib，已查 $(HOST_OUT)/ 与 $(CARGO_TARGET_DIR)/$(HOST_TRIPLE)/$(PROFILE)/"; exit 1; fi; \
	cp -f "$$DY" "$(FLUTTER_MACOS_RUNNER)/" && echo "[flutter] macOS <- $$DY"

flutter-sync-ios: ios-sim
	@test -d "$(FLUTTER_EXAMPLE_DIR)" || (echo "[ERROR] 无 Flutter 目录"; exit 1)
	@mkdir -p "$(FLUTTER_IOS_FFI)"
	@SRC="$(call triple_lib_a,aarch64-apple-ios-sim)"; test -f "$$SRC" || (echo "[ERROR] 无 $$SRC"; exit 1)
	@cp -f "$(call triple_lib_a,aarch64-apple-ios-sim)" "$(FLUTTER_IOS_FFI)/lib$(LIB_NAME).a"
	@echo "[flutter] iOS sim .a -> $(FLUTTER_IOS_FFI)/"

flutter-sync-ios-universal: ios-sim ios-sim-x86
	@test -d "$(FLUTTER_EXAMPLE_DIR)" || (echo "[ERROR] 无 Flutter 目录"; exit 1)
	@mkdir -p "$(FLUTTER_IOS_FFI)"
	@ARM="$(call triple_lib_a,aarch64-apple-ios-sim)"; X86="$(call triple_lib_a,x86_64-apple-ios)"; OUT="$(FLUTTER_IOS_FFI)/lib$(LIB_NAME).a"; \
	if [ -f "$$ARM" ] && [ -f "$$X86" ]; then lipo -create "$$ARM" "$$X86" -output "$$OUT"; \
	elif [ -f "$$ARM" ]; then cp -f "$$ARM" "$$OUT"; else echo "[ERROR] 无 $$ARM"; exit 1; fi; \
	echo "[flutter] iOS sim universal -> $$OUT"

flutter-sync-ios-device: ios-device
	@test -d "$(FLUTTER_EXAMPLE_DIR)" || (echo "[ERROR] 无 Flutter 目录"; exit 1)
	@mkdir -p "$(FLUTTER_IOS_FFI)"
	@cp -f "$(call triple_lib_a,aarch64-apple-ios)" "$(FLUTTER_IOS_FFI)/lib$(LIB_NAME).a"
	@echo "[flutter] iOS device .a -> $(FLUTTER_IOS_FFI)/"

flutter-sync-android: android
	@test -d "$(FLUTTER_EXAMPLE_DIR)" || (echo "[ERROR] 无 Flutter 目录"; exit 1)
	@mkdir -p "$(FLUTTER_ANDROID_JNI)"
	@SO="$(call triple_lib_so,aarch64-linux-android)"; test -f "$$SO" || (echo "[ERROR] 无 $$SO"; exit 1)
	@cp -f "$$SO" "$(FLUTTER_ANDROID_JNI)/"
	@echo "[flutter] Android .so -> $(FLUTTER_ANDROID_JNI)/"

flutter-sync: host-flutter
	@if [ "$$(uname -s)" = "Darwin" ]; then $(MAKE) --no-print-directory flutter-sync-ios; \
	else echo "[flutter-sync] 跳过 iOS（非 macOS）"; fi
	@if [ -n "$$ANDROID_NDK_ROOT" ]; then $(MAKE) --no-print-directory flutter-sync-android; \
	else echo "[flutter-sync] 跳过 Android（未设 ANDROID_NDK_ROOT）"; fi
	@echo "[flutter-sync] 完成。真机: make flutter-sync-ios-device"

setup-targets:
	rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios aarch64-linux-android aarch64-unknown-linux-ohos 2>/dev/null || true

ios-device: setup-targets
	@echo "[FFI] aarch64-apple-ios"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" --target aarch64-apple-ios -p $(PACKAGE)
	@echo "-> $(call triple_lib_a,aarch64-apple-ios)"

ios-sim: setup-targets
	@echo "[FFI] aarch64-apple-ios-sim"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" --target aarch64-apple-ios-sim -p $(PACKAGE)
	@echo "-> $(call triple_lib_a,aarch64-apple-ios-sim)"

ios-sim-x86: setup-targets
	@echo "[FFI] x86_64-apple-ios"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" --target x86_64-apple-ios -p $(PACKAGE)
	@echo "-> $(call triple_lib_a,x86_64-apple-ios)"

ios-all: ios-device ios-sim ios-sim-x86

android: setup-targets
	@if [ -z "$$ANDROID_NDK_ROOT" ]; then echo "[ERROR] 请 export ANDROID_NDK_ROOT"; exit 2; fi
	@echo "[FFI] aarch64-linux-android"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" --target aarch64-linux-android -p $(PACKAGE)
	@echo "-> $(call triple_lib_a,aarch64-linux-android) + .so"

ohos: setup-targets
	@if [ -z "$$OHOS_NDK_HOME" ]; then \
		echo "[ERROR] 请 export OHOS_NDK_HOME（OpenHarmony native SDK）"; exit 2; fi
	@OHOS_HOME="$$OHOS_NDK_HOME"; \
	OHOS_CLANG="$$OHOS_HOME/native/llvm/bin/aarch64-unknown-linux-ohos-clang"; \
	test -x "$$OHOS_CLANG" || (echo "[ERROR] 无 $$OHOS_CLANG"; exit 2); \
	echo "[FFI] $(OHOS_TARGET)"; \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$$OHOS_CLANG" \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_AR="$$OHOS_HOME/native/llvm/bin/llvm-ar" \
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) build --$(PROFILE) \
		--manifest-path "$(MANIFEST)" --target $(OHOS_TARGET) -p $(PACKAGE); \
	echo "-> $(call triple_lib_so,$(OHOS_TARGET))"

flutter-sync-harmony: ohos
	@mkdir -p "$(HARMONY_LIBS_DIR)"
	@SO="$(call triple_lib_so,$(OHOS_TARGET))"; test -f "$$SO" || (echo "[ERROR] 无 $$SO"; exit 1)
	@cp -f "$(call triple_lib_so,$(OHOS_TARGET))" "$(HARMONY_LIBS_DIR)/"
	@echo "[harmony] .so -> $(HARMONY_LIBS_DIR)/"

all-mobile: ios-all android

dist: dist-ios

dist-ios: ios-all
	@set -e; for t in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do \
		src="$(CARGO_TARGET_DIR)/$$t/$(PROFILE)/lib$(LIB_NAME).a"; \
		if [ -f "$$src" ]; then mkdir -p "$(MAKEFILE_DIR)/dist/$$t" && cp -f "$$src" "$(MAKEFILE_DIR)/dist/$$t/" && echo "[dist] $$t"; fi; \
	done

dist-android: android
	@src="$(call triple_lib_a,aarch64-linux-android)"; \
	test -f "$$src" && mkdir -p "$(MAKEFILE_DIR)/dist/aarch64-linux-android" && cp -f "$$src" "$(MAKEFILE_DIR)/dist/aarch64-linux-android/" && echo "[dist] android .a" || (echo "[ERROR] 无 $$src"; exit 1)

check:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) check --manifest-path "$(MANIFEST)" -p $(PACKAGE)

clean:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) clean -p $(PACKAGE)
	rm -rf "$(MAKEFILE_DIR)/dist"
