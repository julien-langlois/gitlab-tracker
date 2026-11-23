# Stability and upgrades

From 1.0.0, `gitlab-tracker` follows [semantic versioning](https://semver.org/) for
what you use as a **product**: a minor or patch release never asks you to change
your setup or loses your data. Anything that would is a new major version, listed
under **breaking** in the [changelog](https://github.com/julien-langlois/gitlab-tracker/blob/main/CHANGELOG.md).

## What is guaranteed

| Surface | Guarantee within a major version |
| :--- | :--- |
| **Command line** | Subcommands, flags and their meaning stay. New ones may be added. The output of `checklist` keeps its Markdown structure. |
| **Configuration** | A `projects.toml`, `config.json` or `.env` that works keeps working: keys are never removed or renamed, and a new key always has a default. Unknown keys are ignored. |
| **Environment variables** | `GITLAB_URL`, `GITLAB_PROJECT_ID`, `GITLAB_TOKEN` and the `GITLAB_TRACKER_*` variables of [Configuration](configuration.md) keep their names and meaning. |
| **Local data** | `tracker.db` and `stats.db` are migrated forward on first launch, automatically, with their history. A schema change is always a new migration: an older database is never left behind. |
| **Token storage** | Tokens stay where they were saved (OS keyring entry, environment): an upgrade never asks for them again. |
| **GitLab** | The [token permissions](gitlab-token.md) needed do not grow within a major version, except for a new optional feature, which works without them. |

## What is not

- **The look of the window and the terminal UI**: pages, layout, colours,
  wording and keyboard shortcuts improve between minor versions.
- **The library crates** `gitlab-tracker-core`, `gitlab-tracker-stats` and
  `gitlab-tracker-redmine`. They are published on crates.io only because
  `cargo install gitlab-tracker` builds from there: they are internal to the
  application, released with it under the same version, and their Rust API may
  change in any release. Depend on them at an exact version (`=1.2.3`) if you do.
- **Logs** (`RUST_LOG`): their format and messages are for diagnosis, not for
  parsing.
- **Demo data** (`--demo`).

## Going back to an older version

Downgrading within the same database schema works. When a newer version has
migrated `tracker.db` or `stats.db` further than an older one knows, the older
version refuses to open it — saying so in the log — rather
than writing to a schema it does not understand: your data stays intact for when
you update again. To really go back, restore the database from before the
upgrade.

## Upgrading from 0.x

The first launch of 1.0 imports the state of 0.x versions on its own:

- `tracker_<hash>.json`, `merges_<hash>.json`, `owners_<hash>.json` and `gui.json`
  move into `tracker.db` and are renamed `*.imported` (kept, not deleted);
- `stats.db` is migrated in place;
- `projects.toml`, `config.json` and the keyring entries are read as they are.

`gitlab-tracker import-state` runs the same import by hand (every project, or
the one given with `--project`).
