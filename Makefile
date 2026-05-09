.PHONY: install demo run test fmt build help

REPO ?= sample-repos/resume-interview

install:
	cargo install --path . --force

demo:
	cargo run -- sample-repos/resume-interview

run:
	cargo run -- $(REPO)

test:
	cargo test

fmt:
	cargo fmt

build:
	cargo build

help:
	@echo "make install          Install brf locally"
	@echo "make demo             Run bundled demo repo"
	@echo "make run REPO=path    Run against your repo"
	@echo "make test             Run test suite"
	@echo "make fmt              Format Rust code"
	@echo "make build            Build debug binary"
