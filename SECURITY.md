# Security policy

gitlab-tracker handles GitLab (and optionally Redmine) access tokens and reads
your projects' merge requests, pipelines and discussions. Security reports are
taken seriously and handled before any other work.

## Supported versions

Only the **latest release** gets security fixes. A fix ships as a new patch
release of the current minor version; updating within a major version never
asks you to change your setup ([stability policy](docs/stability.md)).

## Reporting a vulnerability

**Please do not open a public issue for a vulnerability.**

Report it privately, by either:

- GitHub's private vulnerability reporting: the repository's
  [**Security** tab › **Report a vulnerability**](https://github.com/julien-langlois/gitlab-tracker/security/advisories/new);
- or an email to **julien@julienlanglois.fr**, with `[gitlab-tracker security]`
  in the subject.

Include what you can of:

- the version (`gitlab-tracker --version`), your OS and how it was installed
  (.deb, AppImage, .dmg, .msi, `cargo install`);
- the GitLab edition and version (gitlab.com or self-managed);
- the steps to reproduce, and what an attacker gains.

Never send a real token: if one is needed to reproduce, describe its
permissions instead. Revoke any token you think was exposed.

## What to expect

- An acknowledgement within **5 working days**.
- An assessment (accepted or not, and why) within **15 working days**.
- For an accepted report: a fix released as soon as it is ready, a GitHub
  security advisory crediting you (unless you prefer not to be named), and a
  line in the changelog.

Please give us a reasonable time to release the fix before disclosing it.

## Scope

In scope, for example:

- a token leaving its storage (OS keyring, environment) other than towards the
  GitLab / tracker URL it was configured for: logs, crash reports, exports,
  screenshots, the stand-up text, files on disk;
- content coming from GitLab (MR titles, descriptions, notes, job logs, branch
  names) that makes the app run code, open unexpected URLs or files, or write
  outside its own directories;
- the app sending anything to a host other than the configured GitLab / tracker
  instances and the GitHub release check described under *New versions* in
  [The desktop window](docs/desktop-window.md);
- installers or release artifacts that do not match the tagged source.

Out of scope:

- what a token's own permissions allow on GitLab (the app only reads; see
  [the token's permissions](docs/gitlab-token.md));
- someone with access to your unlocked session or user account;
- vulnerabilities in GitLab itself: report those to
  [GitLab](https://about.gitlab.com/security/disclosure/).

## Supply chain

Dependencies are checked by `cargo deny` (RustSec advisories, licences, sources)
on every change and every week. Each release carries a CycloneDX SBOM of the
binary.
