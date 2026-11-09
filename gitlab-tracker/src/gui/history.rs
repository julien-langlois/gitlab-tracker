//! Back and forward through the pages, as in a browser: the mouse's side
//! buttons, Alt+← and Alt+→. Each project tab keeps its own history.

use super::{GuiApp, Page};

/// How many steps back are kept.
const DEPTH: usize = 50;

/// A page visited, and the MR it showed (the MR page, or the one selected in
/// the list) or the person (a person's page), to bring back with it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visit {
    page: Page,
    mr: Option<String>,
    person: Option<String>,
}

#[derive(Default)]
pub struct History {
    back: Vec<Visit>,
    forward: Vec<Visit>,
    /// Set while going back or forward: that move is not a new visit.
    replaying: bool,
}

impl History {
    /// Forgets every step: the page shown at startup is the first.
    pub fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }
}

/// Where the window is: the page and, for those that show one, the MR.
fn here(g: &GuiApp) -> Visit {
    let page = g.page();
    let mr = matches!(page, Page::Mr | Page::MergeRequests)
        .then(|| g.app.selected_mr().map(|mr| mr.id.clone()))
        .flatten();
    let person = (page == Page::Person)
        .then(|| g.team.selected.clone())
        .flatten();
    Visit { page, mr, person }
}

/// Called by [`GuiApp::open_page`] before it moves to `page`: the page left
/// becomes a step back, unless nothing changes.
pub fn visit(g: &mut GuiApp, page: Page) {
    if g.history.replaying {
        return;
    }
    let from = here(g);
    let mr_changes = page == Page::Mr
        && g.select_id
            .as_ref()
            .is_some_and(|id| from.mr.as_ref() != Some(id));
    if from.page == page && !mr_changes {
        return;
    }
    push(g, from);
}

/// Called by [`GuiApp::open_person`] before it shows `login`'s page: another
/// person's page is a new visit too.
pub fn visit_person(g: &mut GuiApp, login: &str) {
    if g.history.replaying {
        return;
    }
    let from = here(g);
    if from.page == Page::Person && from.person.as_deref() == Some(login) {
        return;
    }
    push(g, from);
}

/// `from` as the step back, the steps forward forgotten.
fn push(g: &mut GuiApp, from: Visit) {
    let history = &mut g.history;
    if history.back.last() != Some(&from) {
        history.back.push(from);
        if history.back.len() > DEPTH {
            history.back.remove(0);
        }
    }
    history.forward.clear();
}

pub fn back(g: &mut GuiApp) {
    if let Some(to) = g.history.back.pop() {
        let from = here(g);
        g.history.forward.push(from);
        go(g, to);
    }
}

pub fn forward(g: &mut GuiApp) {
    if let Some(to) = g.history.forward.pop() {
        let from = here(g);
        g.history.back.push(from);
        go(g, to);
    }
}

fn go(g: &mut GuiApp, to: Visit) {
    if let Some(id) = to.mr {
        g.select_id = Some(id);
    }
    if let Some(login) = to.person {
        g.team.selected = Some(login);
    }
    g.history.replaying = true;
    g.open_page(to.page);
    g.history.replaying = false;
}

/// The mouse's side buttons, anywhere in the window, and Alt+← / Alt+→ when
/// no text field has the keyboard (there they move by word).
pub fn handle(g: &mut GuiApp, ctx: &egui::Context, typing: bool) {
    use egui::{Key, Modifiers, PointerButton};
    let (back_pressed, forward_pressed) = ctx.input_mut(|i| {
        let back = i.pointer.button_pressed(PointerButton::Extra1)
            || (!typing && i.consume_key(Modifiers::ALT, Key::ArrowLeft));
        let forward = i.pointer.button_pressed(PointerButton::Extra2)
            || (!typing && i.consume_key(Modifiers::ALT, Key::ArrowRight));
        (back, forward)
    });
    if back_pressed {
        back(g);
    } else if forward_pressed {
        forward(g);
    }
}
