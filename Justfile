
verify:
    cargo clippy -- -D warnings
    cargo fmt --check
    cargo check

agent-full-verify: verify
