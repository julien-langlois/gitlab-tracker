# 🚀 GitLab MR Tracker

[![CI Quality Gate](https://github.com/julien-langlois/gitlab-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab-tracker/actions)
[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker)](https://crates.io/crates/gitlab-tracker)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/gitlab-tracker)](https://crates.io/crates/gitlab-tracker)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.94+-orange.svg)](https://www.rust-lang.org/)

**GitLab MR Tracker** follows your team's GitLab merge requests across your environment branches (`main`, `staging`, `production`…): whether each MR reached each branch — by merge, cherry-pick, rebase or a partial copy — what holds the others back, and who has the next move. It runs as a desktop window, or in the terminal.

![The merge requests page: views and filters, each MR with its next step and its presence on every branch, the inspector](https://github.com/julien-langlois/gitlab-tracker/raw/HEAD/docs/assets/desktop/merge-requests.png)

## ✨ Key Features

* 🔍 **Branch presence** — on which branch each MR landed, even cherry-picked, rebased or partly copied.
* 👉 **Next step** — who has the ball on each MR, and what to do: review, fix the CI, answer threads, merge, deploy.
* 🗂️ **MR page** — why it can't merge, where it is, its discussions, changes, commits, pipelines and timeline.
* ☀️ **Today** — your reviews, your MRs that need you, and your stand-up in one click.
* 🚀 **Promote & Environments** — what is ready to go to the next branch, and the commands to bring it there.
* 📅 **Milestones** — will the release make its date? A forecast from your team's real pace.
* 📈 **Delivery, CI, Team** — DORA measures, flaky jobs and CI budget, review load and who reviews whom.
* 🧭 **Journal, Overlaps, Stacked MRs** — what happened, MRs about to conflict, chains of MRs out of step.
* ✍️ **Signed commits** *(optional)* — warns about unsigned commits, forgives GitLab rebases.
* 🔐 **Read-only and secure** — tokens in the OS keyring; a read-only fine-grained token is enough.
* ⌨️ **Terminal UI** — the same dashboard from the keyboard, for terminals and SSH sessions.

![An MR page: why it can't merge, where it is on each branch, people, ticket and lifecycle](https://github.com/julien-langlois/gitlab-tracker/raw/HEAD/docs/assets/desktop/mr-page.png)

| Page | What it covers |
| :--- | :--- |
| [Desktop window](docs/desktop-window.md) | Every page of the window, with a screenshot. |
| [Terminal UI](docs/terminal.md) | The terminal dashboard: table, filters, inspector, cockpit, stats, shortcuts. |
| [Configuration](docs/configuration.md) | `projects.toml` annotated, first run, token storage, environment variables. |
| [GitLab token](docs/gitlab-token.md) | Which read-only token to create and why. |
| [How it works](docs/how-it-works.md) | Branch presence and its limits, tracked branches, notifications. |
| [Command line](docs/cli.md) | `--project`, `checklist`, `tracker-statuses`. |
| [Signed commits](docs/signed-commits.md) | Checking that every commit of the open MRs is signed. |

## 🚀 Quick start

### Desktop window

The window is **not on crates.io yet**: it depends on the `bevel` UI crates, which are not released. Build it from source with Rust **1.95+** and the `bevel` repository checked out next to this one (`../bevel`):

```bash
git clone git@github.com:julien-langlois/gitlab-tracker.git
cd gitlab-tracker
cargo install --path gitlab-tracker --features gui      # add stats,redmine as needed

gitlab-tracker --demo gui     # try it on demo data, no GitLab needed
gitlab-tracker gui            # your projects
```

On first launch the window asks for the GitLab URL, the project and the token, checks them against GitLab, and keeps the token in the OS keyring (GNOME Keyring / KWallet, macOS Keychain, Windows Credential Manager) — never in a file. More projects open in tabs. Details: [docs/desktop-window.md](docs/desktop-window.md).

### Terminal UI (optional)

```bash
cargo install gitlab-tracker      # Rust 1.94+ — or a binary from the Releases page

gitlab-tracker --demo             # demo data
gitlab-tracker                    # prompts for the project and token on first launch
```

Pre-built binaries: [Releases](https://github.com/julien-langlois/gitlab-tracker/releases). Keys and views: [docs/terminal.md](docs/terminal.md).

### Linux prerequisites

The OS keyring and desktop notifications need D-Bus headers (macOS and Windows need nothing):

| Distribution | Command |
| :--- | :--- |
| Debian / Ubuntu | `sudo apt install libdbus-1-dev pkg-config` |
| Fedora / RHEL | `sudo dnf install dbus-devel pkgconf` |
| Arch | `sudo pacman -S dbus pkgconf` |

## 🔑 Which token to create

`gitlab-tracker` only reads. Create a [fine-grained personal access token](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens/) (GitLab 19.2+) limited to the projects you track, and give **Read** — nothing else — to:

* **Group and project** tab: Deployment, Job, Pipeline, Label, Work Item, Project, Commit, Merge Request, Repository, Repository Tag.
* **User** tab *(optional)*: User — or set `gitlab_username` in `projects.toml` instead.

Step-by-step creation, what each permission is used for, a script to check the token, replacing it and older GitLab versions: **[docs/gitlab-token.md](docs/gitlab-token.md)**.

## ⚙️ Configuration

Projects and their settings live in `projects.toml` (`~/.config/gitlab-tracker/` on Linux, `~/Library/Application Support/gitlab-tracker/` on macOS, `%APPDATA%\gitlab-tracker\` on Windows). The app writes it on first run, and most settings can be changed from its settings dashboard:

```toml
[[project]]
name             = "My Company — Backend"
gitlab_url       = "https://gitlab.my-company.com"
project_id       = "12345678"
active           = true
tracked_branches = ["develop", "staging", "production"]
gitlab_username  = "jdoe"          # enables the "me" views (Today, To review: me)
```

Every setting, the `.env` file, environment variables and which one wins: [docs/configuration.md](docs/configuration.md).

## 🧩 Optional features

| Feature | Default | What it adds |
| :--- | :--- | :--- |
| `gui` | off | The desktop window — source builds only for now |
| `notifications` | on | Desktop notifications (`--no-default-features` drops them) |
| `stats` | off | Local SQLite history: throughput, cycle time, forecasts — [`gitlab-tracker-stats`](gitlab-tracker-stats/README.md) |
| `redmine` | off | Linked Redmine tickets, their time, status transitions — [`gitlab-tracker-redmine`](gitlab-tracker-redmine/README.md) |

Contributors: [`ARCHITECTURE.md`](ARCHITECTURE.md).

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
