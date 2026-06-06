# Shared variables for bindings/*/Makefile (include from child: include ../mk/common.mk)

BINDINGS_ROOT ?= $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)
WORKSPACE_ROOT ?= $(abspath $(BINDINGS_ROOT)/..)
WORKSPACE_MANIFEST ?= $(WORKSPACE_ROOT)/Cargo.toml
CARGO ?= cargo
PYTHON ?= python3
CARGO_TARGET_DIR ?= $(WORKSPACE_ROOT)/target

# Contract codegen (single entry at bindings root)
CODEGEN := $(MAKE) -C "$(BINDINGS_ROOT)" codegen
CODEGEN_CHECK := $(MAKE) -C "$(BINDINGS_ROOT)" codegen-check
