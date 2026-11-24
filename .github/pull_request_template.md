## What and why

<!-- What changes for the user, and the issue it closes (Closes #…). -->

## Screenshot

<!-- For a visible change: before / after, both themes if colours changed. -->

## Checklist

- [ ] `cargo fmt`, `clippy -D warnings` (all features and `--no-default-features`), `cargo test --workspace --all-features` pass
- [ ] A bug fix comes with a test that fails without it
- [ ] New window text is in **all** `gitlab-tracker/i18n/*.tsv` catalogs
- [ ] A new GitLab request updates `docs/gitlab-token.md`, the README list and the contract fixtures
- [ ] A schema change is a new migration; a new setting has a default and is in `docs/configuration.md`
- [ ] The demo (`--demo gui`) shows the change
- [ ] Commit subjects follow Conventional Commits and read as release notes ([CONTRIBUTING.md](../CONTRIBUTING.md#commits))
