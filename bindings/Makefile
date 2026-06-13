SHELL := /bin/bash
MAKEFILE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
WORKSPACE_ROOT := $(abspath $(MAKEFILE_DIR)/..)
CARGO ?= cargo
PYTHON ?= python3
XTASK ?= $(CARGO) run --manifest-path "$(WORKSPACE_ROOT)/Cargo.toml" -p flare-im-core-sdk-codegen --quiet --

# Sub-packages with local Makefiles (run `make help` in each directory)
SUBDIRS := shared wasm c tauri uniffi

.PHONY: help all verify codegen codegen-check fmt check test build clean \
	$(SUBDIRS) subdirs-help

all: verify

help:
	@echo "Flare IM bindings (contract-first)"
	@echo ""
	@echo "Root targets:"
	@echo "  make codegen        Generate all binding artifacts from contract/*.json"
	@echo "  make codegen-check  Verify generated files are fresh"
	@echo "  make fmt            rustfmt check (runtime, core, wasm, c, tauri, uniffi)"
	@echo "  make check          cargo check all binding crates"
	@echo "  make test           runtime + wasm tests"
	@echo "  make build          wasm-pack build (delegates to bindings/wasm)"
	@echo "  make verify         codegen-check + fmt + check + test + build"
	@echo "  make clean          clean all sub-packages"
	@echo ""
	@echo "Per-package (each has its own Makefile):"
	@for d in $(SUBDIRS); do echo "  make -C $$d help"; done
	@echo "  make subdirs-help   Print all sub-package help banners"

subdirs-help:
	@for d in $(SUBDIRS); do \
		echo "======== $$d ========"; \
		$(MAKE) -C "$(MAKEFILE_DIR)/$$d" help || exit 1; \
		echo ""; \
	done

codegen:
	$(XTASK) codegen

codegen-check:
	$(XTASK) check

fmt:
	$(CARGO) fmt --manifest-path "$(WORKSPACE_ROOT)/Cargo.toml" \
		--package flare-im-core-sdk-bindings-runtime \
		--package flare-im-core-sdk \
		--package flare-im-core-sdk-wasm \
		--package flare-im-core-sdk-ffi \
		--package flare-im-core-sdk-tauri \
		--check
	$(CARGO) fmt --manifest-path "$(MAKEFILE_DIR)/uniffi/Cargo.toml" --check

check: core-check runtime-check wasm-check c-check tauri-check uniffi-check

test: runtime-test wasm-test

build:
	$(MAKE) -C "$(MAKEFILE_DIR)/wasm" build

verify: codegen-check fmt check test build

clean:
	@for d in $(SUBDIRS); do $(MAKE) -C "$(MAKEFILE_DIR)/$$d" clean; done

core-check:
	$(CARGO) check --manifest-path "$(WORKSPACE_ROOT)/Cargo.toml" -p flare-im-core-sdk

runtime-check:
	$(MAKE) -C "$(MAKEFILE_DIR)/shared" check

runtime-test:
	$(MAKE) -C "$(MAKEFILE_DIR)/shared" test

wasm-check:
	$(MAKE) -C "$(MAKEFILE_DIR)/wasm" check

wasm-thin-check:
	$(MAKE) -C "$(MAKEFILE_DIR)/wasm" check-thin

wasm-test:
	$(MAKE) -C "$(MAKEFILE_DIR)/wasm" test

wasm-build:
	$(MAKE) -C "$(MAKEFILE_DIR)/wasm" build

c-check:
	$(MAKE) -C "$(MAKEFILE_DIR)/c" check

tauri-check:
	$(MAKE) -C "$(MAKEFILE_DIR)/tauri" check

uniffi-check:
	$(MAKE) -C "$(MAKEFILE_DIR)/uniffi" check
