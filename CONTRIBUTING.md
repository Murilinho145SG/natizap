# Contributing to natizap

Thanks for helping! natizap is a native desktop WhatsApp client, and every
change should make it faster, more reliable or closer to the phone apps.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- **Read [AGENTS.md](AGENTS.md).** It holds the architecture and the rules
  every change follows: one crate per feature, how crates may depend on each
  other, error handling, file size, comments and tests. It applies to people
  and AI coding assistants alike.
- **Open an issue first** for anything bigger than a small fix, so we can agree
  on the approach before you spend time on it.
- **Behave like the mobile app.** When unsure how something should work, match
  WhatsApp on the phone, not WhatsApp Web.

## Setting up

See [Building](README.md#building) in the README. You need Rust (the toolchain
in `rust-toolchain.toml` installs itself), `nasm` and `libclang`.

To try natizap, link it to a WhatsApp account you are comfortable testing
with: it is an unofficial client, and WhatsApp may restrict accounts that use
one.

## Making a change

1. Keep the change to one feature crate when you can; the `natizap` binary only
   wires crates together and holds no feature logic.
2. Write or update tests that would fail if the behavior broke. Test through a
   crate's public interface, not its internals.
3. Make sure all of these pass with no warnings:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

4. Look at your change in the running app. For anything visual, include a
   screenshot in the pull request.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/) in English, in
the imperative mood, lowercase and without a trailing period:

```
feat(messages): show a document's first page and name

As on the phone, a document's card shows the first page its sender's
phone drew, its file name, and its pages, type and size below.
```

Each commit is one logical change that builds and passes the checks on its
own. Keep refactors, formatting and behavior changes in separate commits.

## Privacy

natizap handles people's private conversations. Never put real names, phone
numbers, messages or screenshots of a real account in code, tests, issues or
pull requests. Use made-up data, as the tests and the README screenshot do.

If you find a security problem, do not open a public issue: report it through
[GitHub's private reporting](https://github.com/Murilinho145SG/natizap/security/advisories/new).

## License

natizap is licensed under the
[GNU Affero General Public License v3.0 or later](LICENSE). By contributing,
you agree that your contributions are licensed under the same terms.
