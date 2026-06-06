# Flare IM WebAssembly binding — run from this directory: `make help`
#
# Requires: rustup target add wasm32-unknown-unknown; optional npm + wasm-pack for `make build`

SHELL := /bin/bash
MAKEFILE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
include $(MAKEFILE_DIR)/../mk/common.mk

PACKAGE := flare-im-core-sdk-wasm
WASM_TARGET := wasm32-unknown-unknown

.PHONY: help all codegen codegen-check fmt check check-thin test build deps verify clean

all: check

help:
	@echo "flare-im-core-sdk-wasm ($(MAKEFILE_DIR))"
	@echo "  make codegen        Regenerate bindings from contract/ (parent)"
	@echo "  make codegen-check  Verify generated artifacts are fresh"
	@echo "  make check          cargo check --target $(WASM_TARGET)"
	@echo "  make check-thin     check without default features (no smoke runtime)"
	@echo "  make test           cargo test (host target)"
	@echo "  make build          wasm-pack build → pkg/ (needs npm install)"
	@echo "  make deps           npm install (wasm-pack)"
	@echo "  make verify         codegen-check + check + check-thin + test"
	@echo "  make clean          cargo clean + remove pkg/"
	@echo "  workspace: $(WORKSPACE_ROOT)"

codegen:
	@$(CODEGEN)

codegen-check:
	@$(CODEGEN_CHECK)

fmt:
	@$(CARGO) fmt --manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE) --check

check:
	@echo "[wasm] check $(WASM_TARGET)"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) check \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE) --target $(WASM_TARGET)

check-thin:
	@echo "[wasm] check $(WASM_TARGET) (no default features)"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) check \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE) \
		--target $(WASM_TARGET) --no-default-features

test:
	@echo "[wasm] test (host)"
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) test \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE)

deps:
	@if [ -f "$(MAKEFILE_DIR)/package-lock.json" ]; then \
		cd "$(MAKEFILE_DIR)" && npm ci; \
	else \
		cd "$(MAKEFILE_DIR)" && npm install; \
	fi

build: deps
	@echo "[wasm] wasm-pack → pkg/"
	cd "$(MAKEFILE_DIR)" && npm run build

verify: codegen-check check check-thin test

clean:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) clean \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE)
	rm -rf "$(MAKEFILE_DIR)/pkg"
