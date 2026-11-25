//! Filters on top of the view: branch pills, effort pills, merged in a period.

use super::*;

/// The sidebar filter at `index`, when it is on.
pub(super) fn filter_on(g: &GuiApp, index: usize) -> Option<&crate::app::ActiveFilter> {
    g.app.filters.iter().find(|f| f.index == index)
}

/// The filters with `index` set to `query` (`None`: off), every other filter
/// of `index` first taken off unless `several`, and those of `apart` too (the
/// effort pills, one at a time).
pub(super) fn with_filter(
    g: &GuiApp,
    index: usize,
    query: Option<String>,
    several: bool,
    apart: &[usize],
) -> Vec<crate::app::ActiveFilter> {
    let mut filters: Vec<_> = g
        .app
        .filters
        .iter()
        .filter(|f| !apart.contains(&f.index) && (several || f.index != index))
        .cloned()
        .collect();
    if let Some(query) = query {
        filters.push(crate::app::ActiveFilter { index, query });
    }
    filters
}

/// "Merged between" as the cockpit's periods (a pill each, a second click
/// takes it off) and two days picked on a calendar: from, to.
pub(super) fn merged_between(g: &mut GuiApp, ui: &mut Ui, index: usize, def: &FilterDef) {
    use crate::merged_range::{MergedRange, Period};
    let theme = g.theme;
    let today = chrono::Local::now().date_naive();
    let current = filter_on(g, index)
        .and_then(|f| MergedRange::parse(&f.query))
        .unwrap_or_default();
    let on = current != MergedRange::default();
    let mut set: Option<Option<MergedRange>> = None;
    ui.add_space(4.0);
    ui.label(muted(&theme, clean_label(def)));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for period in Period::ALL {
            let range = period.range(today);
            let picked = current == range;
            if pill_toggle(ui, &theme, tl(period.label()), theme.tones.purple, picked).clicked() {
                set = Some((!picked).then_some(range));
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let range = (current.from, current.to);
        if let Some(day) = super::super::calendar::date_button(
            ui,
            &theme,
            "merged_from",
            current.from,
            tl("From"),
            range,
        ) {
            let to = current.to.filter(|to| *to >= day);
            set = Some(Some(MergedRange {
                from: Some(day),
                to,
            }));
        }
        ui.label(muted(&theme, "→"));
        if let Some(day) = super::super::calendar::date_button(
            ui,
            &theme,
            "merged_to",
            current.to,
            tl("To"),
            range,
        ) {
            let from = current.from.filter(|from| *from <= day);
            set = Some(Some(MergedRange {
                from,
                to: Some(day),
            }));
        }
        if on
            && ui
                .add(egui::Button::new(muted(&theme, "✕")).frame(false))
                .tip(tl("Every merge day"))
                .clicked()
        {
            set = Some(None);
        }
    });
    if let Some(range) = set {
        let filters = with_filter(g, index, range.map(|r| r.query()), false, &[]);
        g.pending.push(Action::SetFilters(filters));
    }
}

/// "Absent from branch" as a pill per tracked branch: the branches are known,
/// nothing to type. Several can be on (absent from each); a second click on
/// one takes it off.
pub(super) fn branch_pills(g: &mut GuiApp, ui: &mut Ui, index: usize, def: &FilterDef) {
    let theme = g.theme;
    ui.add_space(4.0);
    ui.label(muted(&theme, clean_label(def)));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for branch in g.app.branches.clone() {
            let this = crate::app::ActiveFilter {
                index,
                query: branch.clone(),
            };
            let on = g.app.filters.contains(&this);
            let response = pill_toggle(ui, &theme, &branch, theme.tones.red, on)
                .tip(tf("The MRs not on {branch} yet", &[("branch", &branch)]));
            if response.clicked() {
                let mut filters: Vec<_> = g
                    .app
                    .filters
                    .iter()
                    .filter(|f| **f != this)
                    .cloned()
                    .collect();
                if !on {
                    filters.push(this);
                }
                g.pending.push(Action::SetFilters(filters));
            }
        }
    });
    ui.add_space(2.0);
}

/// Review effort as three pills; a second click on the active one clears it.
pub(super) fn effort_pills(g: &mut GuiApp, ui: &mut Ui, pills: &[(usize, Effort)]) {
    let theme = g.theme;
    if pills.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(muted(&theme, tl("Review effort")));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let all: Vec<usize> = pills.iter().map(|(i, _)| *i).collect();
        for &(index, effort) in pills {
            let (label, tone) = tones::effort(effort, &theme.tones);
            let on = filter_on(g, index).is_some();
            if pill_toggle(ui, &theme, label, tone, on).clicked() {
                let filters = with_filter(g, index, (!on).then(String::new), false, &all);
                g.pending.push(Action::SetFilters(filters));
            }
        }
    });
    ui.add_space(6.0);
}
