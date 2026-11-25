//! The verdict under the form and its submission (checks first with a dry run).

use super::*;

impl ProjectForm {
    /// The banner above the footer: what the last check found, or why GitLab or
    /// Redmine refused. Nothing when there is nothing to say.
    pub fn has_verdict(&self) -> bool {
        self.report.is_some() || self.problems.iter().any(|p| p.remote || p.slot.is_none())
    }

    pub fn verdict(&self, ui: &mut Ui, theme: &Theme) {
        let refused: Vec<&Problem> = self
            .problems
            .iter()
            .filter(|p| p.remote || p.slot.is_none())
            .collect();
        if !refused.is_empty() {
            banner(ui, theme, theme.tones.red, "!", tl("Check failed"), |ui| {
                for problem in refused {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&problem.text).color(theme.palette.text_secondary),
                        )
                        .wrap(),
                    );
                }
            });
        } else if let Some(report) = &self.report {
            banner(
                ui,
                theme,
                theme.tones.green,
                "✔",
                tl("Connection works"),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(semibold(&report.name, theme.text_size).color(theme.palette.text));
                        ui.label(
                            RichText::new(&report.path)
                                .font(egui::FontId::monospace(theme.text_size - 1.5))
                                .color(theme.palette.text_muted),
                        );
                    });
                    let mut facts: Vec<String> = Vec::new();
                    if let Some(user) = &report.username {
                        facts.push(tf("Signed in as @{user}", &[("user", user)]));
                    }
                    if let Some(branch) = &report.default_branch {
                        facts.push(tf("default branch {branch}", &[("branch", branch)]));
                    }
                    if report.saved_token {
                        facts.push(tl("with the saved token").to_string());
                    }
                    if !facts.is_empty() {
                        ui.add(egui::Label::new(muted(theme, facts.join(" · "))).wrap());
                    }
                    if let Some(url) = &report.redmine {
                        ui.add(
                            egui::Label::new(muted(
                                theme,
                                tf("Redmine accepted the API key · {url}", &[("url", url)]),
                            ))
                            .wrap(),
                        );
                    }
                },
            );
        }
    }

    /// Checks the fields, then checks with GitLab in the background: only that
    /// when `dry_run`, else saving too.
    pub(super) fn submit(&mut self, dry_run: bool) {
        self.problems.clear();
        self.report = None;
        self.checked = Some(self.inputs());
        let (done, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        match self.mode.clone() {
            Mode::New => {
                let checked = match check(&self.url, &self.project_id, &self.name, &self.branches) {
                    Ok(checked) => checked,
                    Err(errors) => {
                        self.problems = errors
                            .into_iter()
                            .map(|(slot, e)| Problem::local(Some(slot), tl(e)))
                            .collect();
                        return;
                    }
                };
                let redmine_url = normalize(&self.redmine_url);
                if !redmine_url.is_empty()
                    && !gitlab_tracker_core::secrets::is_secure_url(&redmine_url)
                {
                    self.problems = vec![Problem::local(Some(Slot::RedmineUrl), tl("The Redmine URL starts with https:// (http:// only for this machine): the token must not cross the network unencrypted."))];
                    return;
                }
                // The fields stay filled: Check, then Add, takes them again.
                let request = NewProject {
                    checked,
                    token: self.token.clone(),
                    redmine_url,
                    redmine_token: self.redmine_token.clone(),
                    make_active: self.make_active,
                };
                tokio::spawn(async move {
                    let result = if dry_run {
                        Done::Checked(check_new(request).await)
                    } else {
                        Done::Saved(add_new(request).await.map(Box::new))
                    };
                    let _ = done.send(result);
                    ctx.request_repaint();
                });
            }
            Mode::Tokens { entry, gitlab, .. } => {
                if gitlab && self.token.trim().is_empty() {
                    self.problems = vec![Problem::local(
                        Some(Slot::Token),
                        tl("Type the GitLab token: nothing can load without it."),
                    )];
                    return;
                }
                // A project saved with http:// before this check existed: refuse to
                // send it a token typed now, rather than let it cross in clear.
                let secure = gitlab_tracker_core::secrets::is_secure_url;
                if !self.token.trim().is_empty() && !secure(&entry.gitlab_url) {
                    self.problems = vec![Problem::local(
                        None,
                        tl("The GitLab URL starts with https:// (http:// only for this machine), without spaces."),
                    )];
                    return;
                }
                if !self.redmine_token.trim().is_empty()
                    && entry.tracker.as_ref().is_some_and(|t| !secure(&t.url))
                {
                    self.problems = vec![Problem::local(
                        None,
                        tl("The Redmine URL starts with https:// (http:// only for this machine): the token must not cross the network unencrypted."),
                    )];
                    return;
                }
                let token = self.token.clone();
                let redmine_token = self.redmine_token.clone();
                tokio::spawn(async move {
                    let _ = done.send(Done::Saved(
                        save_tokens(*entry, token, redmine_token)
                            .await
                            .map(Box::new),
                    ));
                    ctx.request_repaint();
                });
            }
        }
        let busy = if dry_run {
            Busy::Checking
        } else {
            Busy::Saving
        };
        self.task = Some((busy, rx));
    }
}
