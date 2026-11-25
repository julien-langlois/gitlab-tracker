# Contributing

Thanks for wanting to help. gitlab-tracker is a desktop app first (the window),
with an optional terminal UI, that **reads** GitLab to tell a team what waits on
whom. This page says how to propose a change so it can be merged quickly.

- **A bug**: open an [issue](https://github.com/julien-langlois/gitlab-tracker/issues/new/choose)
  with the bug template, or a pull request with the fix and a test.
- **A feature or a visible change**: open an issue first and describe the need.
  Anything that shows on screen is agreed on a mockup before code is written,
  so no work is thrown away.
- **A vulnerability**: never in a public issue, see [SECURITY.md](SECURITY.md).

By contributing you agree that your work is published under the
[MIT licence](LICENSE), and to follow the [code of conduct](CODE_OF_CONDUCT.md).

## What the project will and won't do

These choices are deliberate; a change that goes against them will be declined.

- **Read-only towards GitLab.** No call that writes (approve, merge, rebase,
  comment) for now.
- **No surveillance metrics.** People metrics are about what waits on someone
  (reviews, threads, load), never about when or how much someone works.
- **Ticket trackers are optional** (Redmine today): never required by a flow.
- **Add value over GitLab's own pages** rather than copy them.

## Building

Rust 1.95 or newer (`rust-version` in
[`gitlab-tracker/Cargo.toml`](gitlab-tracker/Cargo.toml)). On Linux: `libdbus-1-dev pkg-config` (Debian/Ubuntu names).

```bash
cargo run -- --demo gui                   # the window on demo data, no GitLab needed
cargo run -- --demo                       # the terminal UI on the same data
cargo run --features stats,redmine -- gui # optional features
cargo build --no-default-features         # terminal-only build, no window dependencies
```

The demo (`--demo`) is read-only and covers every page: a new page or state
gets demo data (`gitlab-tracker/src/demo/`) so it can be seen and tested
without a GitLab instance. Use a scratch `XDG_CONFIG_HOME` when testing so your
own configuration stays untouched.

The code is organised as described in [ARCHITECTURE.md](ARCHITECTURE.md): read
the section of the area you change.

## Before opening a pull request

The CI runs these; they must all pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

And, depending on the change:

| You… | Also |
| :--- | :--- |
| fix a bug | add a test that fails without the fix |
| add text to the window | add the row to **all** `gitlab-tracker/i18n/*.tsv` catalogs (same keys; an English copy is fine if you don't speak the language, say so in the PR) |
| add a tooltip | use `gui/tip.rs` (`.tip(..)` / `.rich(..)`), not `on_hover_text` |
| change something visible | attach a screenshot (light and dark theme if colours changed); text must read at WCAG AA |
| read a new GitLab endpoint or GraphQL type | update the permissions table in [docs/gitlab-token.md](docs/gitlab-token.md) and the README list, and the contract fixtures (`gitlab-tracker-gitlab/tests/`) |
| change a database schema | add a new numbered migration; never edit a shipped one |
| change what a page computes or draws | run the load test, in release mode: `cargo test --release -p gitlab-tracker --all-features load_ -- --ignored --nocapture` (every page with ~500 MRs, frame budgets in `gui/load_tests.rs`) |
| add a setting | give it a default, document it in [docs/configuration.md](docs/configuration.md) |
| add a dependency | `cargo deny check` passes (licences, advisories) |

What a minor version may and may not change is in
[docs/stability.md](docs/stability.md).

## Commits

[Conventional Commits](https://www.conventionalcommits.org/). The subject line
becomes a line of the release notes as is (git-cliff), so write it for users,
saying what changes for them, in the present tense:

```text
feat(gui): a pill in the title bar says when a newer version is out
fix(gui): every status badge reads at WCAG AA in both themes
```

Use `feat!:` or a `BREAKING CHANGE:` footer for anything the stability policy
calls breaking. The body says why, and any limit or opt-out. Don't edit
`CHANGELOG.md`: it is generated at release time.
