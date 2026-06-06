# Tauri desktop binding adapter — `make -C bindings/tauri check`

SHELL := /bin/bash
MAKEFILE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
include $(MAKEFILE_DIR)/../mk/common.mk

PACKAGE := flare-im-core-sdk-tauri

.PHONY: help all codegen codegen-check fmt check verify clean

all: check

help:
	@echo "flare-im-core-sdk-tauri"
	@echo "  make codegen / codegen-check  contract pipeline (parent)"
	@echo "  make check                    cargo check"
	@echo "  make verify                   codegen-check + check"

codegen:
	@$(CODEGEN)

codegen-check:
	@$(CODEGEN_CHECK)

fmt:
	@$(CARGO) fmt --manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE) --check

check:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) check \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE)

verify: codegen-check check

clean:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) clean \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE)
