.PHONY: build release run test check fmt clippy clean install

# Match CI: fail the build on rustc warnings, and do not resolve past Cargo.lock.
export RUSTFLAGS ?= -D warnings

build:
	cargo build --locked

release:
	cargo build --locked --release

run:
	cargo run --locked

test:
	cargo test --locked

test-local:
	cargo test --locked -- --include-ignored

check:
	cargo check --locked --all-targets --all-features

fmt:
	cargo fmt --all

# Clear RUSTFLAGS so `-D warnings` does not turn Clippy's warn lints into errors.
clippy:
	RUSTFLAGS= cargo clippy --locked --all-targets --all-features

clean:
	cargo clean

install:
	cargo install --locked --path .
