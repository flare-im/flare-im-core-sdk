CARGO ?= cargo
XTASK := $(CARGO) xtask

# --- artifact build: outputs into ./dist (single source of truth) ---
# This repo only BUILDS artifacts into ./dist. Distribution into the platform apps is
# the client repo's job (flare-im-core-client-sdk `make sync`); the client depends on
# this repo, never the reverse — so nothing here references the client repo.

.DEFAULT_GOAL := help

.PHONY: help
help:
	@$(XTASK) help

.PHONY: verify
verify:
	@$(XTASK) verify

.PHONY: codegen
codegen:
	@$(XTASK) codegen

.PHONY: codegen-check
codegen-check:
	@$(XTASK) codegen-check

.PHONY: core-codegen
core-codegen:
	@$(XTASK) core-codegen

.PHONY: core-codegen-check
core-codegen-check:
	@$(XTASK) core-codegen-check

.PHONY: schema
schema:
	@$(XTASK) schema

.PHONY: schema-check
schema-check:
	@$(XTASK) schema-check

.PHONY: docs
docs:
	@$(XTASK) docs

.PHONY: docs-check
docs-check:
	@$(XTASK) docs-check

.PHONY: build
build:
	@$(XTASK) build

.PHONY: check
check:
	@$(XTASK) check

.PHONY: all
all:
	@$(XTASK) all

.PHONY: clean
clean:
	@$(XTASK) clean

.PHONY: test
test:
	@$(CARGO) test -p xtask

.PHONY: fmt
fmt:
	@$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check:
	@$(CARGO) fmt --all -- --check

# ============================================================================
# Build core-sdk artifacts into ./dist. Two independent steps (no cross-repo call):
#     (1) here:               make dist        # build host+wasm+ios+android -> ./dist
#     (2) in the client repo: make sync        # copy ./dist into each platform app
# Per-platform builds (dist-web/-flutter/-android/-ios) let you build one target
# when another toolchain (Android NDK / iOS) is unavailable.
# ============================================================================
.PHONY: dist
dist: ## Build all core-sdk artifacts into ./dist (host + wasm + ios + android)
	@$(XTASK) build all
	@echo "built artifacts into ./dist — then distribute from flare-im-core-client-sdk with: make sync"

.PHONY: dist-web
dist-web: ## Build only WASM into ./dist/wasm (+ bindings/wasm/pkg for the bundler)
	@$(XTASK) build wasm

.PHONY: dist-flutter
dist-flutter: ## Build host+wasm+ios+android into ./dist (flutter also placed by build)
	@$(XTASK) build host wasm ios-universal android

.PHONY: dist-android
dist-android: ## Build only Android JNI into ./dist/android
	@$(XTASK) build android

.PHONY: dist-ios
dist-ios: ## Build only iOS static libs (+host) into ./dist/ios
	@$(XTASK) build host ios-universal
