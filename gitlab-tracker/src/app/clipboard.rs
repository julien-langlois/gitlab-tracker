//! Clipboard actions: clone command, Markdown release checklist.

use super::*;

impl App {
    /// Copies the selected MR's `git clone -b <branch> <ssh url>` command to the clipboard.
    pub fn yank_clone_command(&mut self) {
        let Some(cmd) = self
            .table_state
            .selected()
            .and_then(|i| self.visible_mrs().nth(i))
            .map(|mr| crate::utils::git_clone_command(&mr.web_url, &mr.source_branch))
        else {
            return;
        };
        if self.copy_to_clipboard(cmd) {
            self.flash(format!("✔ {}", tl("git clone command copied")), false);
        }
    }

    /// Copies the visible rows (filter, search and "hide merged" applied) as a
    /// Markdown release checklist: one row per MR, one column per tracked branch.
    pub fn yank_markdown_checklist(&mut self) {
        let count = self.visible_mrs().count();
        let checklist = self.markdown_checklist();
        if self.copy_to_clipboard(checklist) {
            let copied = tn(
                count as u64,
                "Checklist copied ({n} MR)",
                "Checklist copied ({n} MRs)",
            );
            self.flash(format!("✔ {copied}"), false);
        }
    }

    /// Markdown table of the visible MRs and their presence on each tracked branch.
    pub fn markdown_checklist(&self) -> String {
        let escape = |s: &str| s.replace('|', "\\|");
        let mut out = String::from("| MR | Title | State |");
        for b in &self.branches {
            out.push_str(&format!(" {} |", escape(b)));
        }
        out.push_str("\n|---|---|---|");
        out.push_str(&"---|".repeat(self.branches.len()));
        for mr in self.visible_mrs() {
            out.push_str(&format!(
                "\n| [!{}]({}) | {} | {} |",
                mr.id,
                mr.web_url,
                escape(&mr.title),
                mr.state.as_str()
            ));
            for b in &self.branches {
                out.push_str(match self.branch_presence(mr, b) {
                    Some(BranchPresence::Present) => " ✅ |",
                    Some(BranchPresence::CherryPicked) => " 🍒 |",
                    Some(BranchPresence::Copied) => " ≈ |",
                    Some(BranchPresence::Partial) => " ◐ |",
                    Some(BranchPresence::Absent) => " ❌ |",
                    None => " … |",
                });
            }
        }
        out.push('\n');
        out
    }

    /// Copies `text`; on failure, logs and flashes the error. Returns whether it worked.
    pub(super) fn copy_to_clipboard(&mut self, text: String) -> bool {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new()
                .map_err(|e| tracing::warn!(error = %e, "Clipboard unavailable"))
                .ok();
        }
        let result = match &mut self.clipboard {
            Some(clipboard) => clipboard.set_text(text).map_err(|e| e.to_string()),
            None => Err(tl("no clipboard available").to_string()),
        };
        if let Err(e) = &result {
            tracing::warn!(error = %e, "Failed to copy to clipboard");
            self.flash(format!("✘ {}: {e}", tl("Copy failed")), true);
        }
        result.is_ok()
    }
}
