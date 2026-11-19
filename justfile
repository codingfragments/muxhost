# muxhost — justfile

default:
    @just --list

# format, lint, and test — run before committing
check: fmt-check clippy test

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

# every feature combination a consumer can pin (CI runs the same matrix)
clippy:
    cargo clippy --all-targets -- -D warnings
    cargo clippy --all-targets --features topology -- -D warnings
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test
    cargo test --features topology
    cargo test --all-features
