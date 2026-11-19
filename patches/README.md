# Patched dependencies

Copies of published crates with a fix natizap needs before upstream ships
it. Each is wired through `[patch.crates-io]` in the root `Cargo.toml` and
kept out of the workspace, so it builds with its own lints. Remove the copy
and its patch entry once a release carries the fix.

## wacore-appstate 0.7.0

From [whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust), MIT
licensed. The only change is in `src/processor.rs` (`process_patch`), marked
`natizap:`, and the test that covered the old behavior.

When a device syncs a collection from version N, WhatsApp's server can
answer with its latest snapshot and every patch after it, older than N
included. The library skips the stale snapshot, then rejects the first
older patch as a version mismatch, so the collection never syncs again
and the client retries forever (`patch version mismatch: expected N+1,
got M`). The copy skips patches the state already went through instead:
nothing from them is applied, so they still cannot roll the state back.

## wacore 0.7.0

From [whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust), MIT
licensed. The changes are in `src/voip/rtp.rs` and `src/voip/audio.rs`
(`accepts_rtp_payload_type`), marked `natizap:`, with a test.

A phone that places a call sends its voice as MLOW on RTP payload type 122
(`mlow-1`) instead of 120. The release accepts only 120 and 121 and drops
every frame on 122 before decrypting it, so a call answered in natizap was
silent while the caller still heard natizap. The copy takes 122 as plain
MLOW, as upstream main already does.

The copy leaves out the crate's `testdata/` folders, some 20 MB of codec
vectors only its own tests read, keeping the three `*_seed.bin` tables the
MLOW codec builds with; natizap never runs the crate's tests.
