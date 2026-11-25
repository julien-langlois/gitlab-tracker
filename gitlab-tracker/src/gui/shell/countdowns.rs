//! The two countdowns at the right of the title bar (full refresh, sentinel) and when the last sync ended.

use super::*;

/// The two countdowns at the right of the title bar, each in a block of fixed
/// width (its widest state), with tabular digits: the line never shifts as the
/// seconds tick or a block changes state. Laid out right to left: the full
/// refresh, then the sentinel, then when the last sync ended.
pub(super) fn sync_state(g: &GuiApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.x = 6.0;
    refresh_block(g, ui);
    sentinel_block(g, ui);
    let syncing =
        !g.app.pending_initial_fetches.is_empty() || !g.app.pending_refresh_fetches.is_empty();
    if let (false, Some(at)) = (syncing, g.last_sync) {
        let ago = updated_ago((chrono::Local::now() - at).num_minutes());
        ui.add_space(6.0);
        ui.label(muted(&g.theme, format!("{ago} ·")))
            .tip(at.format("%H:%M:%S").to_string());
    }
}

/// Font of the countdowns' digits.
pub(super) fn clock_font(theme: &Theme) -> egui::FontId {
    fonts::semibold(theme.text_size - 1.0)
}

/// Width of `text` in `font`.
pub(super) fn text_width(ui: &Ui, text: impl Into<String>, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.into(), font, Color32::PLACEHOLDER)
        .size()
        .x
}

/// `mm:ss` with every digit in a cell as wide as the widest digit (tabular
/// figures), so the clock keeps its width from one second to the next.
pub(super) fn clock(ui: &mut Ui, theme: &Theme, secs: u64, color: Color32) -> egui::Response {
    let font = clock_font(theme);
    let text = format!("{:02}:{:02}", secs / 60, secs % 60);
    let cell = ('0'..='9')
        .map(|d| text_width(ui, d, font.clone()))
        .fold(0.0, f32::max);
    let colon = text_width(ui, ":", font.clone());
    let digits = text.chars().filter(char::is_ascii_digit).count() as f32;
    let height = ui.fonts_mut(|f| f.row_height(&font));
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(digits * cell + colon, height), Sense::hover());
    let painter = ui.painter();
    let mut x = rect.left();
    for ch in text.chars() {
        let width = if ch == ':' { colon } else { cell };
        painter.text(
            egui::pos2(x + width / 2.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            ch,
            font.clone(),
            color,
        );
        x += width;
    }
    response
}

/// A block `width` wide, its contents laid out right to left from its right edge.
pub(super) fn fixed_block(ui: &mut Ui, width: f32, contents: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    let mut block = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    block.spacing_mut().item_spacing.x = 4.0;
    contents(&mut block);
}

/// The full refresh: "Next refresh in mm:ss" with a dot (green, amber under a
/// minute, red under 30 s, as in the TUI), or "Syncing · n left" while MRs are
/// read. As wide as the wider of the two.
pub(super) fn refresh_block(g: &GuiApp, ui: &mut Ui) {
    let theme = &g.theme;
    let text_font = egui::FontId::proportional(theme.text_size - 1.0);
    let clock_width = text_width(ui, "88:88", clock_font(theme)) + 4.0;
    let counting =
        text_width(ui, tl("Next refresh in"), text_font.clone()) + clock_width + 4.0 + 2.0 + 7.0;
    let syncing = text_width(ui, tf("Syncing · {n} left", &[("n", "999")]), text_font) + 4.0 + 12.0;
    let pending = g.app.pending_initial_fetches.len() + g.app.pending_refresh_fetches.len();
    fixed_block(ui, counting.max(syncing).ceil(), |ui| {
        if pending > 0 {
            ui.label(muted(
                theme,
                tf("Syncing · {n} left", &[("n", &pending.to_string())]),
            ));
            spinner(ui, 12.0, theme.palette.accent);
            return;
        }
        let secs = g.app.time_left;
        let color = if secs < 30 {
            theme.tones.red.fg
        } else if secs < 60 {
            theme.tones.amber.fg
        } else {
            theme.tones.green.fg
        };
        clock(ui, theme, secs, color);
        ui.label(muted(theme, tl("Next refresh in")));
        ui.add_space(2.0);
        dot(ui, color, 7.0);
    });
}

/// The sentinel's countdown, left of the full refresh's: a ring (a light check)
/// beside the refresh's filled dot, a spinner while a check runs. Nothing when
/// the sentinel is off.
pub(super) fn sentinel_block(g: &GuiApp, ui: &mut Ui) {
    let app = &g.app;
    if app.sentinel_interval_secs == 0 {
        return;
    }
    let theme = &g.theme;
    let color = theme.palette.accent;
    let text_font = egui::FontId::proportional(theme.text_size - 1.0);
    let width = text_width(ui, "88:88", clock_font(theme))
        + 4.0
        + text_width(ui, tl("Sentinel"), text_font)
        + 4.0
        + 2.0
        + 10.0;
    ui.add_space(10.0);
    fixed_block(ui, width.ceil(), |ui| {
        let time = clock(ui, theme, app.sentinel_left, color);
        let caption = ui.label(muted(theme, tl("Sentinel")));
        ui.add_space(2.0);
        let mark = if app.sentinel.in_flight {
            spinner(ui, 10.0, color)
        } else {
            let (rect, ring) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter()
                .circle_stroke(rect.center(), 3.0, Stroke::new(1.5, color));
            ring
        };
        let every = app.sentinel_interval_secs;
        let interval = if every.is_multiple_of(60) {
            tn(every / 60, "Every {n} min", "Every {n} min")
        } else {
            tn(every, "Every {n} s", "Every {n} s")
        };
        let tip = || {
            Tip::new()
                .title(tl("Sentinel"))
                .text(tl(
                    "A light check of what changed on GitLab: MRs, pipelines, comments, \
                     approvals and pushes. Only what changed is read again; the full refresh \
                     still reads everything.",
                ))
                .fact(tl("Interval"), interval.clone())
                .hint(tl("Settings › General › Sentinel interval (0 = off)"))
        };
        for response in [time, caption, mark] {
            response.rich(tip());
        }
    });
}

/// "Updated just now", "… 12 min ago", "… 3h ago", or "… 2 days ago": a window
/// left open over a weekend reads in days, not thousands of minutes.
pub(super) fn updated_ago(minutes: i64) -> String {
    let n = minutes.max(0) as u64;
    if n < 1 {
        tl("Updated just now").to_string()
    } else if n < 60 {
        tn(n, "Updated {n} min ago", "Updated {n} min ago")
    } else if n < 24 * 60 {
        tn(n / 60, "Updated {n}h ago", "Updated {n}h ago")
    } else {
        tn(n / (24 * 60), "Updated {n} day ago", "Updated {n} days ago")
    }
}

// ── Status bar ───────────────────────────────────────────────────────────────
