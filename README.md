# 🚀 GitLab MR Tracker

[![CI Quality Gate](https://github.com/julien-langlois/gitlab_tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab_tracker/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.80+-orange.svg)](https://www.rust-lang.org/)

**GitLab MR Tracker** is a fast, asynchronous Terminal User Interface (TUI) dashboard designed for engineering teams. It provides real-time verification of GitLab Merge Requests across target environment branches (`master`, `preproduction`, `staging`, etc.), handling strict SHA verification as well as cherry-picked commit identification.

![gitlab-tracker demo](assets/demo.gif)

## ✨ Key Features

* 🔐 **OS Keyring Integration (Zero Plain-Text Secrets):** Personal Access Tokens (PAT) can be securely stored directly in your OS secret manager (GNOME Keyring, KWallet, macOS Keychain, or Windows Credential Manager).
* 🏷️ **Dynamic Scoped Labels & Custom Chips:**
  * **Smart Filtering:** Configure specific label prefixes (e.g., `deploy::`, `review::`) to display cleanly as colored chips in the main table grid, while keeping **all** attached tags visible in the side inspector panel.
  * **Customizable Palette:** Map label names or wildcard patterns (e.g., `deploy::*`) to custom terminal colors or standard HEX codes (`#FF5733`) via an XDG-compliant JSON config.
* ⚡ **High Performance & Asynchronous:** Powered by `tokio` and `reqwest`, utilizing non-blocking event loops and bounded concurrent requests via semaphores to protect GitLab API rate limits.
* 🛡️ **Pass-Through Pass Caching:** Core MR metadata (author, milestone, assignee, description, labels) is fetched once and cached locally. Fully deployed MRs bypass network re-queries entirely ("Green Pass").
* 🔍 **Dual Match Verification Engine:**
  * **System 1 (Strict SHA):** Validates precise merge/squash commit SHAs on target branches (resistant to `git reset --hard`).
  * **System 2 (Intelligent Fuzzy Matcher):** Uses a keyword relevance matrix to verify cherry-picked commits deployed across branches.
* 🖥️ **Responsive Flexbox TUI Grid:** Features a dynamic layout engine (`Constraint::Fill`) that seamlessly scales table columns and side panels from 1080p laptop displays to ultra-wide 4K monitors without empty trailing spaces.
* 🌐 **Browser Integration:** Open any selected MR directly in your default browser with a single keypress (`O`).
* 🔔 **Desktop Notifications:** Receives native OS desktop notifications whenever an MR lands on a newly tracked environment branch.
* 📁 **XDG-Compliant Persistence:** Saves tracked dashboard state and UI configurations automatically to platform-standard configuration paths using `directories`.

---

## 🔑 Authentication & Configuration

The application requires your GitLab Project configuration and an API Personal Access Token.

### Step 1: Set up environment variables

1. Copy the provided template to create your local `.env` file:

   ```bash
   cp .env.example .env
   ```

2. Open `.env` and specify your project details:

   ```env
   # Required: Your target GitLab Project ID
   GITLAB_PROJECT_ID=12345678

   # Optional: Custom self-hosted GitLab instance (Defaults to [https://gitlab.com](https://gitlab.com) if omitted)
   GITLAB_URL=[https://gitlab.my-company.com](https://gitlab.my-company.com)

   # Optional: Override token via environment variable (Not recommended for disk storage)
   # GITLAB_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx

   # Optional: Override initial tracked branches for new sessions (comma-separated)
   DEFAULT_BRANCHES="main,develop"

   # Optional: Filter table column tags by prefix (comma-separated)
   TABLE_LABEL_PREFIXES="deploy::,review::"
   ```

---

### 🔄 Settings Resolution Order

Settings are resolved in the following order (highest to lowest priority):

1. **System Environment Variables & Local `.env`** (current directory)
2. **Global `.env`** (`~/.config/gitlab-tracker/.env`)
3. **User Config File** (`~/.config/gitlab-tracker/config.json`)
4. **Built-in Fallback Defaults** (`https://gitlab.com`, `["main"]` for default branch)

---

### Step 2: Keyring PAT Security Layer

Your GitLab personal access token is **never stored in plain text**.

Upon first launch, if no token is found, `gitlab-tracker` will securely prompt
you for your token and store it in your operating system's native secure
keyring (GNOME Keyring, macOS Keychain, or Windows Credential Manager).

```text
 ┌────────────────────────────────────────────────────────┐
 │                   TOKEN LOOKUP ORDER                   │
 ├────────────────────────────────────────────────────────┤
 │ 1. GITLAB_TOKEN environment variable (if set)          │
 │ 2. Native OS Keyring (GNOME Keyring / macOS Keychain)  │
 │ 3. Interactive CLI Prompt (First run fallback)         │
 └────────────────────────────────────────────────────────┘
```

1. **First-Run Onboarding:**
   If no `GITLAB_TOKEN` is found in your `.env` or environment, the application will prompt you interactively in the terminal on its initial launch:

   ```text
   🔑 No GITLAB_TOKEN found in environment or system Keyring.
   Please enter your GitLab Personal Access Token: glpat-xxxxxxxxxxxx
   ✅ Token securely saved to OS Keyring!
   ```

2. **Secure Token Persistence:**
   The token is encrypted and handed off directly to your operating system's native secret manager:
   * **Linux:** GNOME Keyring / KWallet via Secret Service API
   * **macOS:** Apple Keychain Service
   * **Windows:** Windows Credential Manager

3. **Subsequent Launches:**
   You can delete the `GITLAB_TOKEN` entry from your `.env` completely. On subsequent runs, `gitlab_tracker` retrieves the token silently from the OS Keyring without requiring plain-text files or manual re-entry.

---

### Step 3: UI, Default Branches & Label Customization (`config.json`)

On its first launch, the tool automatically generates a `config.json` file inside your OS user configuration directory:

* **Linux:** `~/.config/gitlab-tracker/config.json`
* **macOS:** `~/Library/Application Support/gitlab-tracker/config.json`
* **Windows:** `C:\Users\<User>\AppData\Roaming\gitlab-tracker\config.json`

You can edit this file to adjust default environment branches, label badge colors, and wildcard rules:

```json
{
  "project_id": "12345678",
  "gitlab_url": "[https://gitlab.my-company.com](https://gitlab.my-company.com)",
  "default_branches": [
    "main"
  ],
  "table_label_prefixes": [
    "deploy::",
    "review::"
  ],
  "label_colors": {
    "deploy::*": {
      "bg": "#2E7D32",
      "fg": "white"
    },
    "review::approved": {
      "bg": "magenta",
      "fg": "white"
    },
    "review::*": {
      "bg": "cyan",
      "fg": "black"
    },
    "size::*": {
      "bg": "dark_gray",
      "fg": "white"
    },
    "bug": {
      "bg": "#D32F2F",
      "fg": "white"
    }
  }
}
```

#### 🌿 How Branch Resolution Works:

1. **Active Session Priority:** If `tracker_state.json` exists from a previous run, the app restores your last active layout (columns added/removed via input).
2. **First Run / Fresh Session:** If no state exists, initial branches are loaded from `DEFAULT_BRANCHES` in `.env` if provided, falling back to `default_branches` in `config.json` (defaults to `["main"]`).

---

## 📦 Installation

### Pre-built Binaries

Download the latest pre-compiled binary for your architecture from the [Releases Page](https://github.com/your-username/gitlab_tracker/releases).

### Building from Source & Global Installation

Ensure you have Rust (1.80+) installed:

```bash
git clone [https://github.com/your-username/gitlab_tracker.git](https://github.com/your-username/gitlab_tracker.git)
cd gitlab_tracker

# Build optimized release executable
cargo build --release

# Optional: Install binary globally to ~/.cargo/bin/
cargo install --path .
```

Once installed globally, you can launch the dashboard from any terminal folder by running:

```bash
gitlab_tracker
```

---

## ⌨️ Dashboard Navigation & Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `▲` / `▼` or `k` / `j` | Navigate rows in the table |
| `142` + `Enter` | Add MR ID `!142` to tracking |
| `staging` + `Enter` | Add branch `staging` to target columns |
| `-142` + `Enter` | Remove MR ID `!142` from tracking |
| `-staging` + `Enter` | Remove branch column `staging` |
| `O` | Open selected MR in your default web browser |
| `R` | Force immediate network refresh for all MRs |
| `Del` or `X` | Delete selected MR row |
| `ESC` | Quit dashboard |

---

## 🏗️ Project Architecture

```text
src/
├── main.rs          # Event loop orchestrator & async channel setup
├── app.rs           # State machine & row navigation logic
├── config.rs        # Label filtering, wildcard matching & HEX color parsing
├── models.rs        # Strongly-typed API DTOs & runtime event types
├── gitlab.rs        # Async network handling & rate-limit semaphores
├── storage.rs       # OS Keyring interface & XDG state/config persistence
├── utils.rs         # Fuzzy matching algorithmic utilities
└── ui/              # Flexbox Renderers for Table & Context Inspector
```

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
