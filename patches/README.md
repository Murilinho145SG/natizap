# Patched dependencies

Copies of published crates with a fix natizap needs before upstream ships
it. Each is wired through a `[patch]` section of the root `Cargo.toml` and
kept out of the workspace, so it builds with its own lints. Remove the copy
and its patch entry once a release carries the fix.

## wacore-appstate

From [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust) at the
commit the root `Cargo.toml` pins, MIT licensed; its manifest has the
workspace inheritance written out. The only change is in `src/processor.rs` (`process_patch`), marked
`natizap:`, and the test that covered the old behavior.

When a device syncs a collection from version N, WhatsApp's server can
answer with its latest snapshot and every patch after it, older than N
included. The library skips the stale snapshot, then rejects the first
older patch as a version mismatch, so the collection never syncs again
and the client retries forever (`patch version mismatch: expected N+1,
got M`). The copy skips patches the state already went through instead:
nothing from them is applied, so they still cannot roll the state back.
