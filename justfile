# muxhost — justfile

default:
    @just --list

# format, lint, and test — run before committing
check: fmt-check clippy test

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

clippy:
    cargo clippy --all-targets -- -D warnings

test:
    cargo test
