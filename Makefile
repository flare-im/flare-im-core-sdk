CARGO ?= cargo
XTASK := $(CARGO) xtask

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
