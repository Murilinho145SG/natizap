# natizap

Rust workspace. See [AGENTS.md](AGENTS.md) for contribution rules.

## Development

```sh
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
