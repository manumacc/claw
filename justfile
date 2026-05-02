set dotenv-load

dev_home := env("CLAW_HOME", "/tmp/claw-dev")
provider := env("CLAW_PROVIDER", "fake")
log_filter := env("CLAW_LOG", "claw=info,warn")

default:
    @just --justfile {{ quote(justfile()) }} --list

check:
    just --fmt --check --unstable
    cargo fmt -- --check
    cargo check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test

daemon:
    RUST_LOG={{ quote(log_filter) }} CLAW_HOME={{ quote(dev_home) }} CLAW_PROVIDER={{ quote(provider) }} cargo run -- daemon run

chat +prompt:
    RUST_LOG={{ quote(log_filter) }} CLAW_HOME={{ quote(dev_home) }} cargo run -- chat --new --provider {{ quote(provider) }} {{ quote(prompt) }}

resume chat_id +prompt:
    RUST_LOG={{ quote(log_filter) }} CLAW_HOME={{ quote(dev_home) }} cargo run -- chat --chat {{ quote(chat_id) }} {{ quote(prompt) }}

providers:
    RUST_LOG={{ quote(log_filter) }} CLAW_HOME={{ quote(dev_home) }} cargo run -- providers list

[confirm("Delete the development CLAW_HOME directory?")]
reset:
    test {{ quote(dev_home) }} = /tmp/claw-dev || (echo "Refusing to reset non-default CLAW_HOME" >&2; exit 1)
    rm -rf {{ quote(dev_home) }}
