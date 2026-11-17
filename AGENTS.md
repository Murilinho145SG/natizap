# AGENTS.md

Rules for AI agents and contributors working in this repository. They are
binding: follow them on every change, and update this file in the same commit
when a rule changes.

## Language

English only, everywhere: code identifiers, comments, doc comments,
documentation, commit messages, branch names, log and error messages.

## Workspace layout

Cargo workspace (resolver 3, edition 2024). Every crate lives in `crates/` and
is picked up automatically by `members = ["crates/*"]`.

| Crate     | Role                       |
| --------- | -------------------------- |
| `natizap` | The binary (entry point).  |

Keep this table current when a crate is added, renamed or removed.

- Shared metadata (`version`, `edition`, `rust-version`, `publish`) is
  inherited with `x.workspace = true`.
- Every dependency is declared once in the root `[workspace.dependencies]` and
  pulled into crates with `dep.workspace = true`. Never pin a version inside a
  member crate.
- Every crate has `[lints] workspace = true`. Do not weaken lints per crate; if
  an exception is truly needed, use a narrow `#[expect(lint, reason = "...")]`
  at the item, never `#[allow]` at the crate root.
- Split a new crate out only when it has its own reason to change and a stable
  interface. Do not create crates, modules or traits "for later".

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
