
verify:
    cargo clippy -- -D warnings
    cargo fmt --check
    cargo check
    cargo test

agent-full-verify: verify
