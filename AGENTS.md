# AGENTS.md

Rules for AI agents and contributors working in this repository. They are
binding: follow them on every change, and update this file in the same commit
when a rule changes.

## Language

English only, everywhere: code identifiers, comments, doc comments,
documentation, commit messages, branch names, log and error messages.

## Project overview

natizap is a native desktop WhatsApp client written in Rust with
[GPUI](https://gpui.rs/), the GPU-accelerated UI framework behind Zed.

**Why it exists:** WhatsApp Web and the official Windows client are slow,
memory-hungry and unreliable; only the mobile apps are good. natizap aims to
give the desktop the same quality: instant startup, smooth scrolling through
long chats, low memory use, and an interface that never makes the user wait.

**How it works:** natizap pairs with the user's phone as a linked device
(QR code or pairing code), exactly like WhatsApp Web, and speaks the
WhatsApp multi-device protocol directly. Messages stay end-to-end encrypted;
keys and history are stored locally on the user's machine.

**Scope: full feature parity.** natizap targets everything WhatsApp offers,
not a messaging-only subset: chats, groups, communities, channels, status,
media, stickers, polls, and **voice and video calls**, including group calls.
Calls are a flagship feature: the official desktop calls are poorly made, and
natizap must do them properly (low latency, stable audio/video, correct device
handling).

**Product principles** (use them to settle design decisions):

- **Performance is the feature.** Never block the UI thread; protocol, crypto,
  storage and media work run in the background. Large chat lists and message
  histories must render lazily and stay smooth.
- **Behave like the mobile app.** When in doubt, match how WhatsApp mobile
  behaves, not WhatsApp Web.
- **Local first.** What has been synced is readable offline and opens
  instantly from local storage.

**Caveat:** this is an unofficial client using a reverse-engineered protocol.
WhatsApp may change the protocol or restrict accounts that use unofficial
clients. Keep protocol code isolated so protocol changes land in one crate.

### Planned feature crates

Crates are created when their feature is implemented, not before. The
expected split, for orientation:

| Crate                      | Feature                                                    |
| -------------------------- | ---------------------------------------------------------- |
| `natizap-session`          | Device pairing (QR/code) and connection state.             |
| `natizap-chats`            | Chat list: ordering, unread counts, pin, archive, mute.    |
| `natizap-calls`            | Voice and video calls, 1:1 and group; audio/video devices. |
| `natizap-contacts`         | Contacts, profiles, presence, blocking.                    |
| `natizap-groups`           | Groups and communities: metadata and administration.       |
| `natizap-status`           | Status updates: view, post, privacy.                       |
| `natizap-channels`         | Channels: follow, read, react.                             |
| `natizap-notifications`    | Native desktop notifications.                              |

Each feature crate owns its GPUI views, logic and local cache together
(colocation); `natizap-ui` holds only components that several features
share. Local data lives in the platform's per-user data directory, one
file per owner (`session.db` for protocol keys, `chats.db` for the chat
list, `messages.db` for messages, `notifications.conf` for notification
settings, `stickers.db` for favorite stickers, `media/` for downloaded
attachments, `media/thumbs/` for the small copies media grids draw,
`media/stickers/` for favorite stickers' files and `updates/` for
downloaded update packages).

Local stores are migrated, never dropped and rebuilt: WhatsApp sends the
history only once, right after linking, so lost data cannot be synced
again. Each store keeps an append-only `MIGRATIONS` list.

Features talk to each other only through the binary: the chat list emits
`ChatSelected`, the binary opens that chat in `MessageLog` and tells
`ChatList` it is on screen; `MessageLog` emits `MessageSent`, which the
binary forwards to the chat list's preview. Photos and videos open in the
conversation's carousel; when it lands on a video, `ConversationView` emits
`PlayRequested`, which the binary answers with a `PlayerView` handed back
through `ConversationView::show_player`. Voice notes and audio files play
inside their bubble; both players share `natizap-playback`. `MessageLog`
also emits `ForwardRequested`, which the binary answers with the chats'
`ChatPicker`, handing the chosen chats back to `MessageLog::forward`. The
conversation emits `ProfileRequested` from its header, which the binary
answers by showing the contacts' `ProfilePanel` beside it. The panel emits
`TileOpened` and `MediaRequested`, which the binary answers with
`ConversationView::show_tile` and `ConversationView::browse_media`. `Notifier` hears
new messages itself; the binary tells it whether the window is focused,
which chat `MessageLog` has open and which chats `ChatList` has muted, and
it chimes for messages anywhere else. While the window is in the
background it also posts system notifications; clicking one emits
`NotificationOpened`, which the binary answers by opening that chat. The
binary draws the navigation rail (`natizap-ui`'s `Rail`) itself: its
Settings button opens the notifications' `NotificationSettings` dialog, and
your avatar opens the contacts' `OwnProfile` in place of the chat list,
which emits `OwnProfileClosed` to go back. The binary hands the conversation the stickers'
`StickerPicker` to show from its composer, sends what it emits as
`StickerPicked` through `MessageLog::send_sticker`, tells the conversation
which stickers are favorites, and passes its `StickerFavoriteToggled` to
the `StickerShelf`. The `Updater` stands alone: the binary only shows it in
the title bar.

The binary builds the features once per run and keeps them in its `Shell`,
not in the window: closing the window leaves natizap running in the tray
(`natizap-tray`'s `Tray`, whose `TrayAction`s reopen it or quit), still
connected and notifying, and opening it again builds a fresh `Root` on the
same features. Without a tray, closing quits. Only one natizap runs per
data directory: a later launch asks the running one to show itself and
exits.

## Architecture: one crate per feature

The project is **modular**: every feature is its own crate. This is the main
organizing rule of the codebase.

- **Feature crates** (`crates/natizap-<feature>`) own everything about one
  feature: its types, logic, errors, persistence and tests. A change to a
  feature should land in its crate and nowhere else.
- **The binary crate** (`crates/natizap`) only wires feature crates together:
  configuration, startup, and connecting features to each other. It holds no
  feature logic.
- **Shared crates** hold only what several features genuinely need (for
  example `natizap-core` for common domain types, or a crate for a shared
  infrastructure concern). Create one only when a second feature needs the same
  thing; never move code there "just in case". A shared crate must be more
  stable than the crates that use it.
- **Dependency direction:** `natizap` → feature crates → shared crates. A
  feature crate must not depend on another feature crate. If two features need
  to talk, define the contract (a trait or plain types) in a shared crate and
  let the binary connect them.
- **Public surface:** each crate exposes a small, documented API from its
  `lib.rs`; everything else is `pub(crate)` or private. Other crates never
  reach into a crate's internals.
- **Adding a feature** means adding a crate: `cargo new --lib
  crates/natizap-<feature>`, inherit workspace metadata and lints, register
  any dependency in the root `[workspace.dependencies]`, and add a row to the
  table below.

### Crates

| Crate             | Kind    | Role                                                     |
| ----------------- | ------- | -------------------------------------------------------- |
| `natizap`         | binary  | Entry point; wires the feature crates together.          |
| `natizap-session` | feature | Pairing lifecycle and the pairing screen.                 |
| `natizap-stickers` | feature | Favorite stickers synced with the phone, and their picker. |
| `natizap-protocol` | shared  | Owns the WhatsApp client; fans its events out to features. |
| `natizap-chats`    | feature | Chat list synced from WhatsApp, cached in SQLite, and its view. |
| `natizap-contacts` | feature | Profiles of people and businesses, and your own, editable. |
| `natizap-messages` | feature | Message store, the open conversation and sending.         |
| `natizap-notifications` | feature | Chimes for messages arriving outside the chat on screen. |
| `natizap-playback` | shared | Media engine: static FFmpeg decoding, cpal sound, clock. |
| `natizap-player`   | feature | The video player the media carousel plays videos on.     |
| `natizap-profiles` | shared  | Profile pictures: fetched on sight, cached on disk.        |
| `natizap-ui`       | shared  | Theme, fonts, icons, window frame and the component library. |
| `natizap-tray`     | feature | The tray icon that reopens or quits natizap.             |
| `natizap-update`   | feature | Finds, downloads and installs new releases from GitHub.   |

Keep this table current when a crate is added, renamed or removed.

### Runtime model

- GPUI owns the main thread and its own executor. The protocol client
  (`whatsapp-rust`) needs Tokio, which runs on worker threads registered with
  `gpui_tokio::init`; spawn Tokio work with `gpui_tokio::Tokio::spawn`.
- Only `natizap-protocol` talks to the client's lifecycle. Features call
  `Protocol::subscribe` with the event kinds they need and translate those
  events into their own types; `whatsapp-rust` types do not leak into a
  feature's public API.
- Heavy event processing (history sync, persistence) runs on GPUI's
  background executor; only the resulting state is applied to GPUI entities
  with `entity.update`, followed by `cx.notify()`. Never block the UI thread
  on protocol, storage or network work.
- GPUI comes from Zed's Git repository pinned to one commit. The root
  `[patch.crates-io]` mirrors the patches Zed applies at that commit; update
  both together when bumping the pin.
- `whatsapp-rust` is used without its `simd` feature, which needs nightly.
- `patches/` holds copies of published crates carrying a fix upstream has
  not released, wired through `[patch.crates-io]` and excluded from the
  workspace; `patches/README.md` says what each changes and why. Keep the
  change minimal and marked `natizap:`, and drop the copy once a release
  carries the fix.
- Media plays inside natizap: FFmpeg is compiled from source and linked
  statically (`ffmpeg-next`'s `build` feature), decoding runs on a dedicated
  thread, and sound goes through `cpal`. Never hand media to system players
  or link system codec libraries.

### Visual design

- Every color, font and icon comes from `natizap-ui` (`theme`, `Icon`);
  feature crates never hard-code hex colors or font names, so the look
  changes in one place.
- Palette: near-black graphite surfaces (`DEEP`, `PANEL`, `PANE`, `RAISED`,
  `LINE`) with a single orange accent (`ACCENT`) reserved for unread counts
  and primary actions. `MINT` and `ALERT` only signal state.
- Floating surfaces (bubbles, menus, the composer field) are glass:
  `theme::glass` gives them a faint light rim (`EDGE`) and a soft shadow.
  There is no backdrop blur: GPUI cannot blur behind an element, and GNOME
  does not blur behind windows.
- Type: Figtree (`UI_FONT`) for interface text; Bricolage Grotesque
  (`DISPLAY_FONT`) only for the wordmark, headings and avatar initials.
- Emoji come from the bundled `Natizap Emoji` font, the fallback of
  `base_font()` set at the window root; never draw emoji as images or pick
  font weights to dodge monochrome glyphs. Rebuild the font with
  `scripts/build-emoji-font.py` (needs fontTools) to update it.
- `natizap-ui` is the component library: one module per component under
  `components/` (button, badge, tooltip, …). Features build with these
  instead of styling their own buttons, badges or menus.
- The components window (`Gallery`, Ctrl+Alt+G) renders every component in
  every state. It is where the look is reviewed, and it is the one exception
  to "no dead code": a component may exist before a feature uses it as long
  as the gallery shows it.
- The app draws its own title bar and window frame (`window_frame`), since
  GNOME on Wayland leaves decorations to the client.

## Workspace conventions

Cargo workspace (resolver 3, edition 2024). Every crate lives in `crates/` and
is picked up automatically by `members = ["crates/*"]`.

- Shared metadata (`version`, `edition`, `rust-version`, `publish`) is
  inherited with `x.workspace = true`.
- Every dependency is declared once in the root `[workspace.dependencies]` and
  pulled into crates with `dep.workspace = true`. Never pin a version inside a
  member crate.
- Every crate has `[lints] workspace = true`. Do not weaken lints per crate; if
  an exception is truly needed, use a narrow `#[expect(lint, reason = "...")]`
  at the item, never `#[allow]` at the crate root.
- Do not create crates, modules or traits for features that do not exist yet.

## Design rules (untangle principles)

The guiding question for every change is: **should these pieces change
together?** Pieces that change together live together; pieces that do not are
not tied; everything depends on what changes less. These rules come from the
`untangle-codebase` skill and apply to new code from day one, so the codebase
never needs a big untangling pass.

### No hidden links

- **No copies** of logic, constants or lists. One source of truth; derive the
  rest from it.
- **No global mutable state** or files shared implicitly between modules. Pass
  dependencies explicitly (function arguments, struct fields).
- **Colocate by domain, not by layer.** A feature's types, logic, errors and
  tests live in the same module/crate, not spread across `models/`,
  `services/`, `utils/`.
- **Explicit over implicit.** If callers must follow a call order or
  convention, encode it in the type system (constructors, builders, typestate,
  RAII guards) instead of documenting it.

### No needless links

- **Encapsulate.** Keep fields and helpers private; expose the smallest `pub`
  surface. Prefer `pub(crate)` to `pub` inside a crate.
- **Dependency direction.** Stable code never imports more volatile code.
  Domain logic does not depend on I/O, frameworks or transport; those depend on
  it (use traits at the boundary when there is more than one implementation).
- **Caller vocabulary.** A general function must not take parameters only one
  caller understands. Move that logic to the caller.
- **Deep modules.** Small interface, substantial behavior. A module whose
  interface mirrors its internals is a sign it should be merged or redesigned.

### No surplus

- No pass-through wrappers.
- No speculative generality: no extension points, generic parameters, feature
  flags or config for things that do not vary today.
- No dead code. Delete it; Git remembers.

### Special cases are design feedback

An `if` for one value, a parameter for one caller, a swallowed error, a retry
or sleep, a disabled test: each one is a requirement the design does not
account for. Before adding one, look for the design change that makes it
unnecessary. If it must stay, comment on the external reason.

## Rust practices

- Before every commit, all of these must pass with zero warnings:

  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

- **Errors:** library crates define typed errors with `thiserror`; the binary
  may use `anyhow` at the top level. Propagate with `?` and add context. Never
  swallow an error silently.
- **No panics in normal paths:** no `unwrap`/`expect` outside tests. When an
  invariant truly cannot fail, use `expect` with a message that states the
  invariant, under `#[expect(clippy::expect_used, reason = "...")]`.
- **No `unsafe`** (forbidden at workspace level).
- **Types over primitives:** use newtypes and enums to make invalid states
  unrepresentable; prefer `&str`/`&[T]` in parameters and owned types in
  return values; avoid needless `clone`.
- **Logging:** use `tracing`, not `println!`/`eprintln!` (except CLI output
  that is the program's actual product).
- **Async:** do not block inside async code; use the runtime's facilities.
- **Dependencies:** add one only when it earns its weight; prefer well
  maintained crates; enable only the features you use.

## File size

Keep source files at **500 lines or fewer**. Slightly over is acceptable when
splitting would scatter code that changes together; going well beyond that
(roughly 600+) means the file holds more than one concern and must be split
by responsibility, not by arbitrary line ranges.

## Comments and documentation

- Every `pub` item gets a `///` doc comment saying what it is for and any
  contract the type system cannot express (`# Errors`, `# Panics`).
- Every crate root has a `//!` comment describing the crate's role.
- Comments explain **why**, never **what**. Keep comments about things outside
  the code (external APIs, protocol quirks, workarounds and their cause).
  Do not write comments that restate the implementation.

## Tests

- Test behavior through public interfaces, not internals, so refactors do not
  break tests when behavior is unchanged.
- A test must be able to fail on a real regression. Do not write tests that
  mirror the implementation or that cannot fail.
- Unit tests live next to the code in `#[cfg(test)] mod tests`; cross-crate or
  end-to-end tests live in the crate's `tests/` directory.
- Never disable a test to get a change through; fix the cause.

## Commits

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<optional scope>): <imperative summary>

<optional body: why the change was made>
```

- **Types:** `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `style`,
  `build`, `ci`, `chore`, `revert`.
- **Scope:** the crate or area, e.g. `feat(natizap): ...`.
- **Imperative mood**, lowercase, no trailing period, at most ~72 characters:
  `fix(parser): reject empty input`, not `fixed parser` or `fixes parser`.
- **Atomic:** one logical change per commit. It builds, passes the checks
  above and can be reverted on its own. Do not mix refactors, formatting,
  and behavior changes in one commit; split them.
- Breaking changes use `!` after the type/scope and a `BREAKING CHANGE:`
  footer.
