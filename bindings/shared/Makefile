# Shared bindings runtime — `make -C bindings/shared check`

SHELL := /bin/bash
MAKEFILE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
include $(MAKEFILE_DIR)/../mk/common.mk

PACKAGE := flare-im-core-sdk-bindings-runtime

.PHONY: help all codegen codegen-check fmt check test verify clean

all: check

help:
	@echo "flare-im-core-sdk-bindings-runtime"
	@echo "  make codegen / codegen-check  contract pipeline (parent)"
	@echo "  make check                    cargo check"
	@echo "  make test                     cargo test"
	@echo "  make verify                   codegen-check + check + test"

codegen:
	@$(CODEGEN)

codegen-check:
	@$(CODEGEN_CHECK)

fmt:
	@$(CARGO) fmt --manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE) --check

check:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) check \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE)

test:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) test \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE)

verify: codegen-check check test

clean:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" $(CARGO) clean \
		--manifest-path "$(WORKSPACE_MANIFEST)" -p $(PACKAGE)
