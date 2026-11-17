# natizap

Native desktop WhatsApp client built with Rust and GPUI. See [AGENTS.md](AGENTS.md) for contribution rules.

## Development

Building needs `nasm` and `libclang` (for the bundled FFmpeg), plus the usual
GPUI dependencies. On Debian/Ubuntu:

```sh
sudo apt install nasm libclang-dev
```

If `LIBCLANG_PATH` points at a cross-compiling clang (ESP32 toolchains export
one), FFmpeg's bindings are generated for the wrong target and the build
fails with a pointer-size assertion. Unset it, or point it at the system one,
for natizap: `LIBCLANG_PATH=/usr/lib/llvm-21/lib cargo build`.

```sh
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
