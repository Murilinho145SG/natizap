<p align="center">
  <img src="brand/natizap-wordmark.svg" alt="natizap" width="420">
</p>

<p align="center">
  A native desktop WhatsApp client, written in Rust with
  <a href="https://gpui.rs">GPUI</a>, the GPU-accelerated UI framework behind Zed.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: AGPL-3.0-or-later" src="https://img.shields.io/badge/license-AGPL--3.0--or--later-ff7a1a"></a>
  <img alt="Platform: Linux" src="https://img.shields.io/badge/platform-Linux-1a1d22">
  <img alt="Status: early development" src="https://img.shields.io/badge/status-early%20development-1a1d22">
</p>

![natizap showing a conversation](docs/screenshot.png)

<sub>The screenshot uses made-up chats and numbers.</sub>

## Why

WhatsApp Web and the official desktop apps are slow, memory-hungry and
unreliable; only the phone apps feel good. natizap aims to give the desktop
the same quality: instant startup, smooth scrolling through long chats, low
memory use, and an interface that never makes you wait.

It links to your phone as a companion device, exactly like WhatsApp Web, and
speaks the WhatsApp multi-device protocol directly through
[whatsapp-rust](https://github.com/oxidezap/whatsapp-rust). Messages stay
end-to-end encrypted; keys and history live only on your machine.

## What works today

- **Linking** by QR code, with the chat list and history synced from the phone
  and cached locally, so it opens instantly, even offline.
- **Chat list**: search by name or number, unread and group filters, archived
  chats, pins, mutes, unread counts and verified business seals.
- **Conversations**: WhatsApp formatting, clickable links, email addresses,
  phone numbers and Brazilian tax IDs, code blocks with syntax colors,
  replies that jump to the quoted message, reactions, forwarding, starring,
  pinning and deleting, text selection and copying.
- **Media**: photos with a full-size viewer, videos in a built-in player,
  voice notes with their waveform and playback speeds, documents with their
  first page, stickers, shared contacts and polls.
- **Sending**: text, files, photos, documents, pasted files and screenshots,
  recorded voice notes, contacts and polls.
- **Profiles** of people, businesses (seal, hours, banner, links) and groups
  (members and admins), with presence: online, last seen and typing.
- **Notifications**: a chime for new messages when natizap is in the
  background or another chat is open; muted chats stay silent.

## Not yet

Calls, status, channels, desktop notification pop-ups, poll votes and search inside a chat
are on the roadmap. natizap targets full parity with the phone apps, calls
included.

## Caveat

natizap is an unofficial client built on a reverse-engineered protocol. It is
not affiliated with WhatsApp or Meta. WhatsApp may change the protocol at any
time or restrict accounts that use unofficial clients: use it at your own
risk.

## Building

natizap builds FFmpeg from source and links it statically, which needs `nasm`
and `libclang`, plus the usual GPUI dependencies. On Debian or Ubuntu:

```sh
sudo apt install nasm libclang-dev
cargo run --release
```

If `LIBCLANG_PATH` points at a cross-compiling clang (embedded toolchains often
export one), FFmpeg's bindings are generated for the wrong target and the
build fails with a pointer-size assertion. Unset it, or point it at the system
clang, for example `LIBCLANG_PATH=/usr/lib/llvm-21/lib cargo build`.

Before sending a change, all of these must pass with no warnings:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## How it is organized

Every feature is its own crate under `crates/` (`natizap-chats`,
`natizap-messages`, `natizap-contacts`, …), shared code lives in a few small
crates (`natizap-protocol`, `natizap-ui`, `natizap-playback`, …), and the
`natizap` binary only wires them together. [AGENTS.md](AGENTS.md) explains the
architecture and the rules every change follows.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before
opening a pull request, and follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

natizap is free software under the
[GNU Affero General Public License v3.0 or later](LICENSE).

It bundles third-party work under its own licenses: FFmpeg (LGPL-2.1-or-later,
built from source and linked statically), and the Figtree, Bricolage Grotesque,
JetBrains Mono and Noto Color Emoji fonts (SIL Open Font License 1.1).
