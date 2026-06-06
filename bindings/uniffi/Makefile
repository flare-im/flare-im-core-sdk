# UniFFI binding (standalone workspace) — `make -C bindings/uniffi check`

SHELL := /bin/bash
MAKEFILE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
include $(MAKEFILE_DIR)/../mk/common.mk

PACKAGE := flare-im-core-sdk-uniffi
MANIFEST := $(MAKEFILE_DIR)/Cargo.toml

.PHONY: help all codegen codegen-check fmt check verify clean

all: check

help:
	@echo "flare-im-core-sdk-uniffi (workspace: $(MAKEFILE_DIR))"
	@echo "  make codegen / codegen-check  contract pipeline (parent bindings/)"
	@echo "  make check                    cargo check (local manifest)"
	@echo "  make verify                   codegen-check + check"

codegen:
	@$(CODEGEN)

codegen-check:
	@$(CODEGEN_CHECK)

fmt:
	@$(CARGO) fmt --manifest-path "$(MANIFEST)" --check

check:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) check \
		--manifest-path "$(MANIFEST)" -p $(PACKAGE)

verify: codegen-check check

clean:
	@$(CARGO) clean --manifest-path "$(MANIFEST)" -p $(PACKAGE)
