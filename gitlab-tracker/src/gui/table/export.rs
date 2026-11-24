//! The table's export: Markdown / CSV lines of the visible rows.

use super::*;

/// `mrs` as text, for the export: the title, author and status, then the
/// columns shown (or, `all_columns`, every column the window has) and where
/// each MR is on every tracked branch.
pub(in crate::gui) fn export_table(
    g: &GuiApp,
    mrs: &[&crate::models::TrackedMr],
    all_columns: bool,
) -> super::super::export::Table {
    let app = &g.app;
    let theme = &g.theme;
    let tones = &theme.tones;
    let me = app.config.gitlab_username.as_deref();
    let columns: Vec<Col> = if all_columns {
        let offered: Vec<&str> = app.visible_column_defs().iter().map(|c| c.id).collect();
        Col::ALL
            .into_iter()
            .filter(|c| c.window_only() || offered.contains(&c.id()))
            .collect()
    } else {
        wanted_columns(app, g.hide_next_column, !g.queues.is_empty())
    };
    let order = branch_order(app);
    let mut headers = vec![
        tl("Title").to_string(),
        tl("Author").to_string(),
        tl("Status").to_string(),
    ];
    headers.extend(columns.iter().map(|c| c.title().to_string()));
    if all_columns {
        headers.push(tl("Updated").to_string());
    }
    headers.extend(order.iter().map(|(name, _)| name.clone()));
    let rows = mrs
        .iter()
        .map(|mr| {
            let failed = matches!(mr.status, MrStatus::Error(_));
            let mut cells = vec![
                mr.title.clone(),
                split_person(&mr.author).0.to_string(),
                tones::status(mr.state, &mr.mergeability, tones)
                    .0
                    .to_string(),
            ];
            for col in &columns {
                cells.push(match col {
                    Col::Next => NextView::of(&crate::next_step::of(app, mr, me), theme)
                        .map(|n| {
                            let mut text = format!("{}: {}", n.who, n.what);
                            if !n.since.is_empty() {
                                text = format!("{text} · {}", n.since);
                            }
                            text
                        })
                        .unwrap_or_default(),
                    Col::Ci => mr
                        .pipelines
                        .first()
                        .map(|p| tones::pipeline(p.status, tones).0.to_string())
                        .unwrap_or_default(),
                    Col::Effort => mr
                        .diff_stats
                        .as_ref()
                        .map(|s| {
                            let effort =
                                Effort::from_score(s.difficulty(&app.config.complexity_profile));
                            tones::effort(effort, tones).0.to_string()
                        })
                        .unwrap_or_default(),
                    Col::Notes => mr.user_notes_count.to_string(),
                    Col::Activity => activity(app, mr.updated_at.as_deref(), theme).0.to_string(),
                    Col::Labels => mr.labels.join(", "),
                    Col::Milestone => mr.milestone.clone().unwrap_or_default(),
                    Col::Target => mr.target_branch.clone(),
                    Col::Behind => mr
                        .diff_stats
                        .as_ref()
                        .and_then(|s| s.commits_behind)
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                    Col::Ticket => mr
                        .linked_ticket
                        .as_ref()
                        .map(|t| format!("#{} {}", t.id, t.status))
                        .unwrap_or_default(),
                    Col::Queue => super::super::merge_queue::cell(&g.queues, &mr.id, theme)
                        .map(|(text, _, _)| text)
                        .unwrap_or_default(),
                });
            }
            if all_columns {
                cells.push(mr.updated_at.clone().unwrap_or_default());
            }
            cells.extend(
                order
                    .iter()
                    .map(|(branch, _)| match app.branch_presence(mr, branch) {
                        None if !failed => String::new(),
                        presence => tones::presence(presence, failed, tones).1.to_string(),
                    }),
            );
            super::super::export::Row {
                id: mr.id.clone(),
                url: mr.web_url.clone(),
                cells,
            }
        })
        .collect();
    super::super::export::Table { headers, rows }
}

/// A line of the table: a group's heading, or an MR (its index in the visible list).
pub(super) enum Line {
    Heading {
        key: String,
        label: String,
        count: usize,
        tone: Option<Tone>,
        folded: bool,
    },
    Mr(usize),
}

/// The table's lines: the visible MRs, under their group's heading when grouped,
/// folded groups without their MRs.
pub(super) fn lines(g: &GuiApp) -> Vec<Line> {
    let app = &g.app;
    if g.group_by == GroupBy::None {
        return (0..app.visible_mrs().count()).map(Line::Mr).collect();
    }
    let tones = &g.theme.tones;
    let me = app.config.gitlab_username.as_deref();
    let today = chrono::Local::now().date_naive();
    // (key, label, tone, members), groups in the order they first show up.
    let mut groups: Vec<(String, String, Option<Tone>, Vec<usize>)> = Vec::new();
    for (index, mr) in app.visible_mrs().enumerate() {
        let (label, tone) = match g.group_by {
            GroupBy::NextStep => {
                let bucket = crate::next_step::of(app, mr, me).bucket();
                (bucket.label().to_string(), Some(bucket_tone(bucket, tones)))
            }
            GroupBy::Status => {
                let (label, tone) = tones::status(mr.state, &mr.mergeability, tones);
                (label.to_string(), Some(tone))
            }
            GroupBy::Milestone => (
                mr.milestone
                    .clone()
                    .unwrap_or_else(|| tl("No milestone").into()),
                None,
            ),
            GroupBy::Author => (split_person(&mr.author).0.to_string(), None),
            GroupBy::MergedDay => (merge_day(mr.merged_at.as_deref(), today), None),
            GroupBy::None => unreachable!(),
        };
        match groups.iter_mut().find(|grp| grp.1 == label) {
            Some(group) => group.3.push(index),
            None => groups.push((
                format!("{:?}:{label}", g.group_by),
                label,
                tone,
                vec![index],
            )),
        }
    }
    if g.group_by == GroupBy::NextStep {
        // In the order of the flow: me first, done last.
        groups.sort_by_key(|grp| {
            crate::next_step::Bucket::ALL
                .iter()
                .position(|b| b.label() == grp.1)
                .unwrap_or(usize::MAX)
        });
    }
    let mut lines = Vec::new();
    for (key, label, tone, members) in groups {
        let folded = g.folded.contains(&key);
        lines.push(Line::Heading {
            key,
            label,
            count: members.len(),
            tone,
            folded,
        });
        if !folded {
            lines.extend(members.into_iter().map(Line::Mr));
        }
    }
    lines
}

/// The heading of the MRs merged on the day of `merged_at`: "Today",
/// "Yesterday", then "Fri 20/11"; "Not merged" without one.
pub(super) fn merge_day(merged_at: Option<&str>, today: chrono::NaiveDate) -> String {
    let Some(day) = crate::merged_range::merge_day(merged_at) else {
        return tl("Not merged").to_string();
    };
    match (today - day).num_days() {
        0 => tl("Today").to_string(),
        1 => tl("Yesterday").to_string(),
        _ => {
            use chrono::Datelike;
            format!(
                "{} {}",
                crate::snooze::weekday_short(day.weekday()),
                day.format("%d/%m")
            )
        }
    }
}
