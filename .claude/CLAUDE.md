# gitlab-tracker — working rules

Desktop-first GitLab MR tracker in Rust (egui window by default, ratatui TUI optional).
Aiming at a **professional-grade 1.x**: every change must look like it ships tomorrow.
Contributor internals live in `ARCHITECTURE.md`; user docs in `README.md` + `docs/`.
Read the relevant section of `ARCHITECTURE.md` before touching an area.

## Product stance (non-negotiable)

- **The GUI is the product.** The TUI is optional and keeps working, but features are
  designed for the window first. Behaviour lives in `App` / `actions::dispatch`, never in
  a view: both frontends go through the same `Action`s.
- **Read-only towards GitLab.** No write call (approve, rebase, merge, comment…) until
  "phase 2" is explicitly opened; it will need an optional write token, confirmations
  and a docs update.
- **Value over the GitLab UI.** Don't rebuild what GitLab already shows well (branches
  housekeeping and Todos were dropped for that reason).
- **No surveillance metrics.** People metrics are about what waits on someone (reviews,
  threads, load), never when or how much they work (no working hours, off-hours,
  weekend activity, per-person productivity ranking).
- **Review metrics are draft-aware.** Teams open MRs as drafts when work starts. Every
  review/wait KPI starts at "up for review" (`up_for_review_at()` / `utils::waits_from`),
  skips bots, and bumps `REVIEW_ACTIVITY_VERSION` if the timeline reading changes.
  See `docs/how-it-works.md#the-review-timeline-and-drafts`.
- **Ticket trackers are optional add-ons** (Redmine today, others later, behind
  features). UI presents them generically ("Ticket tracker · Optional · Redmine"),
  folded/secondary, never blocking the GitLab path.
- **Passive, calm UI.** No modal for non-urgent information (update notice = a pill +
  a bell entry, no release notes, no auto-update).

## Workflow

- **New feature with a visible surface → mockup first.** Publish a mockup (artifact)
  and wait for the maintainer's go before writing code. Ask before starting each phase
  of a multi-phase feature.
- **GUI change → hand over a screenshot** of the result (full absolute path), taken
  with the debug-build hooks in `gitlab-tracker/src/gui/mod.rs`
  (`GITLAB_TRACKER_GUI_SCREENSHOT`, `_PAGE`, `_SIZE`, `_KEYS`, `_HOVER`, `_TOUR`…) on
  `--demo gui`. Build with `--features gui` first, use a fresh scratch
  `XDG_CONFIG_HOME`, run with `</dev/null`, one capture at a time. English captures
  for docs: `LANGUAGE=en LC_ALL=C.UTF-8 LANG=C.UTF-8`. If a temporary patch was needed
  to reach the area, restore it and say what was forced.
- **The demo (`--demo`) must show every feature.** A new page, state or case gets
  canned demo data (`src/demo/`), so it can be seen, screenshotted and render-tested
  without a GitLab instance.
- Never test against the maintainer's real config or tokens: use a scratch
  `XDG_CONFIG_HOME`. Never record contract fixtures with a personal token without asking.

## Code rules

- **Crate boundaries:** `-core`, `-gitlab`, `-store` never depend on `ratatui`,
  `crossterm`, `egui` or the binary. GitLab tasks emit `GitlabEvent`, mapped to
  `AppEvent` in `app_event.rs`.
- **State mutations** only through `App::apply_event` (event bus). Views never mutate
  `App`; they record `Action`s dispatched after the frame.
- **Never block the UI thread** (no network, no big I/O, no `block_on` beyond tiny local
  saves). A frame must not grow with the number of MRs (render rows in view only,
  memoize analyses via `gui/memo.rs`).
- **Errors:** `thiserror` enums in libraries and fallible binary code, never
  `Result<_, String>`; `anyhow` only in `main` / `cli` / `demo`. Best-effort
  enrichments degrade to `Option`/empty + `tracing::warn!`, never fail the MR row.
  No `unwrap()`/`expect()` on data coming from GitLab, the network, files or the user.
- **Database schema:** a change is always a **new numbered migration**
  (`tracker.db`: store crate, `stats.db`: stats crate). Never edit a shipped migration.
  A DB from a newer version is refused, never written.
- **Secrets:** tokens via `gitlab-tracker-core` `secrets` (env → keyring → prompt),
  zeroized, never logged, never in panic/crash reports or screenshots.
- **File size:** a source file stays under ~1,000 lines. Past that, split it into a
  folder of the same name, one file per part (the parent keeps the types and the
  frame); see `ARCHITECTURE.md`. Canned demo data (`demo/orbit_api.rs`) is exempt.
- **Tests:** unit tests in the file, `#[cfg(test)] mod tests { use super::*; … }`. They
  move to a `tests.rs` beside it (`#[cfg(test)] mod tests;`) when they outgrow the
  code or push the file past the size limit. Tests of the public API only (contract
  replays) go in the crate's `tests/`. No network in tests: fixtures or a local stub.

## GUI rules

- **i18n:** every user-visible string goes through `tl`/`tf`/`tn`/`tc` and gets a row in
  **all five** catalogs `gitlab-tracker/i18n/{de,es,fr,pt-br,zh-hans}.tsv` with identical
  keys (plurals: `@plural\tone|other\tforms`). The catalogs are compiled in: rebuild the
  binary to see them.
- **Tooltips** only through `gui/tip.rs` (`response.tip(..)`, `response.rich(Tip::new()..)`,
  `widgets::icon_button_tip`), never `on_hover_text`. On a `.truncate()` label with our
  tooltip, add `.show_tooltip_when_elided(false)`.
- **Colours** from the theme (`gui/tones.rs`, `gui/colors.rs`), never hard-coded. Text
  and badges must read at **WCAG AA** in light and dark themes.
- **Accessibility:** interactive widgets get an accessible name (screen readers name
  the rail's pages, buttons, tabs — keep it that way). Every action reachable with the
  keyboard; new shortcuts registered in the shortcuts registry and shown in `?`.
- **Fonts:** no glyph that the bundled fonts don't have (render tests fail on it).

## GitLab API

- **Any new (or removed) REST endpoint / GraphQL type** updates, in the same change:
  - the permissions table in `docs/gitlab-token.md` and the one-line list in README
    ("Which token to create"), mapped **verbatim** from GitLab's raw docs
    (`doc/auth/tokens/fine_grained_access_tokens_{rest,graphql,other}.md`), never guessed;
  - the contract fixtures (`gitlab-tracker-gitlab/tests/fixtures`, anonymized).
- Token permissions must not grow within a major version, except for a new optional
  feature that works without them (403 → feature skipped, said in the UI/log).
- Prefer light reads (`updated_after`, sentinel) over full refreshes; keep the full
  cycle as the safety net.
- Calls not made to GitLab or the ticket tracker (e.g. the GitHub update check) are
  listed in the docs for transparency.

## Compatibility (docs/stability.md)

Semver covers the **product**: CLI, `projects.toml` / `config.json` / `.env` keys, env
vars, local databases, token storage. Keys are never removed or renamed and every new
key has a default; anything else is `feat!:` and a new major. Library crates are
internal (pinned `=x.y.z`), no API promise. MSRV is `rust-version` in `Cargo.toml`.

## Docs

- README stays short (~115 lines, GUI first). Details go in `docs/`:
  `desktop-window.md` (every page + screenshot in `docs/assets/desktop/`),
  `terminal.md`, `configuration.md`, `gitlab-token.md`, `how-it-works.md`, `cli.md`,
  `stability.md`. Links leaving `docs/` are absolute GitHub URLs (GitHub Pages).
- New GUI page/feature → section + English screenshot in `docs/desktop-window.md`, one
  line in README Key Features if major. New setting → `docs/configuration.md`.
- Code layout / internals → `ARCHITECTURE.md`.

## Quality gates (must pass before any commit, same as CI)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

Feature sets that must each compile: default (`gui`,`notifications`),
`--no-default-features` (terminal only, no Wayland/X headers), `stats`, `redmine`,
and `gitlab-tracker-core` alone. `cargo deny check` for new dependencies (licences,
advisories, sources). Coverage has a floor per crate (`scripts/coverage-floors.py`,
run in CI): never lower one; raise it when coverage grows. Bug fix → a regression test; GUI page → covered by the headless
render tests (`gui/render_tests.rs`).

## Commits

- Conventional Commits; **the subject is the release-note line** (git-cliff builds
  `CHANGELOG.md` and the GitHub release from it): written for users, describing the
  outcome in the present tense, e.g.
  `feat(gui): a pill in the title bar says when a newer version is out`,
  `fix(gui): every status badge reads at WCAG AA in both themes`.
- Body in prose: what the user sees, why, limits/opt-outs. `feat!:` or a
  `BREAKING CHANGE:` footer for anything the stability policy calls breaking.
- Commits are GPG-signed. One logical change per commit; never edit `CHANGELOG.md` by
  hand (release tooling does it). Don't push or release unless asked.
