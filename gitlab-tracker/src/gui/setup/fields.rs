//! The form's fields: a new project's URL, project and tracker, then the tokens.

use super::*;

impl ProjectForm {
    pub(super) fn new_fields(&mut self, ui: &mut Ui, theme: &Theme) {
        let report = self.report.clone();
        group(ui, theme, tl("GitLab"), None, |ui| {
            let url = input(
                ui,
                theme,
                Input {
                    label: tl("GitLab URL"),
                    hint: "https://gitlab.com",
                    help: tl("https://gitlab.com or your own instance"),
                    mono: true,
                    ..Input::new(self.problem(Slot::Url))
                },
                &mut self.url,
            );
            ui.add_space(12.0);
            let known = self.token_known(Kind::Gitlab, &self.url, !url.has_focus());
            let problem = self.problem(Slot::Token).cloned();
            let mut token = std::mem::take(&mut self.token);
            token_input(
                ui,
                theme,
                Input {
                    label: tl("Personal access token"),
                    hint: if known == Some(true) {
                        tl("Saved — leave empty to keep it")
                    } else {
                        "glpat-…"
                    },
                    help: tl("Scope read_api; saved in the OS keyring, never in a file."),
                    ..Input::new(problem.as_ref())
                },
                &mut token,
                known,
            );
            self.token = token;
        });
        ui.add_space(16.0);
        group(ui, theme, tl("Project"), None, |ui| {
            input(
                ui,
                theme,
                Input {
                    label: tl("Project"),
                    hint: tl("group/project or 4217"),
                    help: tl("Its numeric id (Settings › General) or its path, group/project"),
                    mono: true,
                    ..Input::new(self.problem(Slot::Project))
                },
                &mut self.project_id,
            );
            ui.add_space(12.0);
            // GitLab's values once checked: what an empty field stands for.
            let name_hint = report
                .as_ref()
                .map_or(tl("GitLab's name"), |r| r.name.as_str())
                .to_string();
            let branch_hint = report
                .as_ref()
                .and_then(|r| r.default_branch.clone())
                .unwrap_or_else(|| tl("The default branch").to_string());
            let branches_problem = self.problem(Slot::Branches).cloned();
            ui.columns(2, |columns| {
                // Not justified: a wrapped help would be stretched to the edge.
                let top = Layout::top_down(Align::Min);
                let (left, right) = columns.split_at_mut(1);
                left[0].with_layout(top, |ui| {
                    input(
                        ui,
                        theme,
                        Input {
                            label: tl("Name"),
                            optional: true,
                            hint: &name_hint,
                            help: tl("Shown in the tab"),
                            ..Input::new(None)
                        },
                        &mut self.name,
                    );
                });
                right[0].with_layout(top, |ui| {
                    input(
                        ui,
                        theme,
                        Input {
                            label: tl("Tracked branches"),
                            optional: true,
                            hint: &branch_hint,
                            help: tl("Comma-separated, in the table's order"),
                            mono: true,
                            ..Input::new(branches_problem.as_ref())
                        },
                        &mut self.branches,
                    );
                });
            });
        });

        // The ticket tracker is optional (Redmine today, others later): folded
        // behind one row until asked for.
        #[cfg(feature = "redmine")]
        {
            ui.add_space(16.0);
            // Unfolded by a problem of its own: it must show.
            let open = self.tracker_open
                || self.problem(Slot::RedmineUrl).is_some()
                || self.problem(Slot::RedmineToken).is_some();
            if !open {
                // Folded with a URL typed: said on the row, never hidden away.
                let url = normalize(&self.redmine_url);
                let (title, detail, icon) = if url.is_empty() {
                    (
                        tl("Link a ticket tracker").to_string(),
                        tl("Optional · Redmine").to_string(),
                        Icon::Plus,
                    )
                } else {
                    (
                        tl("Ticket tracker").to_string(),
                        format!("Redmine · {url}"),
                        Icon::ArrowDown,
                    )
                };
                if fold_row(ui, theme, &title, &detail, icon) {
                    self.tracker_open = true;
                }
                return;
            }
            let mut fold = false;
            let mut remove = false;
            group_with(
                ui,
                theme,
                |ui| {
                    // The caption folds it back, the values kept; the cross
                    // removes the tracker.
                    fold = caption_toggle(ui, theme, tl("Ticket tracker"), tl("optional"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        remove = super::super::widgets::icon_button_tip(
                            ui,
                            tl("Remove the ticket tracker"),
                            Icon::Close,
                            &theme.icon_button(),
                            22.0,
                            super::super::tip::Tip::new().hint(tl(
                                "Remove the ticket tracker: the project is added without one.",
                            )),
                        )
                        .clicked();
                    });
                },
                |ui| {
                    let url = input(
                        ui,
                        theme,
                        Input {
                            label: tl("Redmine URL"),
                            hint: "https://redmine.example.com",
                            help: tl("Links the MRs to their tickets; empty: no tracker"),
                            mono: true,
                            ..Input::new(self.problem(Slot::RedmineUrl))
                        },
                        &mut self.redmine_url,
                    );
                    if !self.redmine_url.trim().is_empty() {
                        ui.add_space(12.0);
                        let known =
                            self.token_known(Kind::Redmine, &self.redmine_url, !url.has_focus());
                        let problem = self.problem(Slot::RedmineToken).cloned();
                        let mut token = std::mem::take(&mut self.redmine_token);
                        token_input(
                            ui,
                            theme,
                            Input {
                                label: tl("Redmine API key"),
                                hint: if known == Some(true) {
                                    tl("Saved — leave empty to keep it")
                                } else {
                                    ""
                                },
                                help: tl("My account › API access key; saved in the OS keyring."),
                                ..Input::new(problem.as_ref())
                            },
                            &mut token,
                            known,
                        );
                        self.redmine_token = token;
                    }
                },
            );
            if remove {
                self.redmine_url.clear();
                self.redmine_token = Zeroizing::default();
            }
            if fold || remove {
                self.tracker_open = false;
            }
        }
    }

    pub(super) fn token_fields(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        entry: &ProjectEntry,
        gitlab: bool,
        redmine: bool,
    ) {
        let name = entry.name.as_deref().unwrap_or(&entry.project_id);
        ui.add(
            egui::Label::new(muted(
                theme,
                tf(
                    "{project} has no token in the OS keyring for the instances below. Type them once: they are saved in the keyring, and every project on the same instance uses them.",
                    &[("project", name)],
                ),
            ))
            .wrap(),
        );
        ui.add_space(14.0);
        if gitlab {
            group(ui, theme, tl("GitLab"), Some(&entry.gitlab_url), |ui| {
                let problem = self.problem(Slot::Token).cloned();
                let mut token = std::mem::take(&mut self.token);
                token_input(
                    ui,
                    theme,
                    Input {
                        label: tl("Personal access token"),
                        hint: "glpat-…",
                        help: tl("Personal access token, scope read_api."),
                        ..Input::new(problem.as_ref())
                    },
                    &mut token,
                    Some(false),
                );
                self.token = token;
            });
            ui.add_space(16.0);
        }
        if redmine {
            let url = entry
                .tracker
                .as_ref()
                .map(|t| t.url.as_str())
                .unwrap_or_default();
            group(ui, theme, tl("Redmine"), Some(url), |ui| {
                let problem = self.problem(Slot::RedmineToken).cloned();
                let mut token = std::mem::take(&mut self.redmine_token);
                token_input(
                    ui,
                    theme,
                    Input {
                        label: tl("Redmine API key"),
                        hint: "",
                        help: tl("Empty: the project opens without its tickets."),
                        ..Input::new(problem.as_ref())
                    },
                    &mut token,
                    Some(false),
                );
                self.redmine_token = token;
            });
        }
    }
}
