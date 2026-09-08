//! Cmd-F search: the match index, the floating find bar, and its shortcuts.

use crate::cursor::{edit_id, load_selection, set_selection};
use crate::edit::{byte_to_char_idx, char_idx_to_byte};
use crate::markdown::hash_of;
use crate::theme::{pal, theme_kind, ThemeKind};
use crate::win95;
use crate::NotedApp;
use egui::{FontFamily, FontId, Stroke};

fn find_id() -> egui::Id {
    egui::Id::new("find_field")
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    query: String,
    /// Byte ranges of every match, in document order.
    pub hits: Vec<(usize, usize)>,
    current: usize,
    /// The query/text the current `hits` were computed from.
    indexed: Option<(String, u64)>,
    focus_field: bool,
    pub scroll_to_hit: bool,
}

impl Find {
    fn step(&mut self, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let n = self.hits.len() as isize;
        self.current = (((self.current as isize + delta) % n + n) % n) as usize;
        self.scroll_to_hit = true;
    }

    /// The active match while the bar is open.
    pub fn hit(&self) -> Option<(usize, usize)> {
        if self.open {
            self.hits.get(self.current).copied()
        } else {
            None
        }
    }
}

/// All non-overlapping occurrences of `needle`, as byte ranges.
///
/// Smart case: a query typed in all-lowercase matches case-insensitively.
/// Lowercasing is ASCII-only so byte offsets stay valid for the original text.
pub fn find_hits(hay: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let fold = !needle.chars().any(|c| c.is_uppercase());
    let (hay, needle) = if fold {
        (hay.to_ascii_lowercase(), needle.to_ascii_lowercase())
    } else {
        (hay.to_owned(), needle.to_owned())
    };

    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = hay[from..].find(&needle) {
        let s = from + p;
        out.push((s, s + needle.len()));
        from = s + needle.len();
    }
    out
}

impl NotedApp {
    /// Shortcuts that work regardless of which field has focus. Runs before any
    /// widget so the keys never reach the editor or the find field.
    pub fn handle_find_keys(&mut self, ctx: &egui::Context) {
        let cmd = egui::Modifiers::COMMAND;
        let cmd_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);

        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::F)) {
            // Seed the query from the editor selection, the way every other
            // macOS app does.
            if let Some((a, b)) = load_selection(ctx, edit_id()) {
                if a != b {
                    let sel = &self.text
                        [char_idx_to_byte(&self.text, a)..char_idx_to_byte(&self.text, b)];
                    if !sel.contains('\n') && sel.chars().count() <= 128 {
                        self.query_changed(sel.to_owned());
                    }
                }
            }
            self.find.open = true;
            self.find.focus_field = true;
        }

        if !self.find.open {
            return;
        }

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.close_find(ctx);
            return;
        }

        let field_focused = ctx.memory(|m| m.has_focus(find_id()));
        // Shift variants first: `consume_key` ignores extra modifiers.
        let prev = ctx.input_mut(|i| {
            i.consume_key(cmd_shift, egui::Key::G)
                || (field_focused && i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter))
        });
        let next = ctx.input_mut(|i| {
            i.consume_key(cmd, egui::Key::G)
                || (field_focused && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        });
        if prev {
            self.find.step(-1);
        } else if next {
            self.find.step(1);
        }
    }

    fn close_find(&mut self, ctx: &egui::Context) {
        self.find.open = false;
        // Leave the cursor on the match the user landed on.
        if let Some((s, e)) = self.find.hits.get(self.find.current).copied() {
            set_selection(
                ctx,
                edit_id(),
                byte_to_char_idx(&self.text, s),
                byte_to_char_idx(&self.text, e),
            );
        }
        ctx.memory_mut(|m| m.request_focus(edit_id()));
    }

    fn query_changed(&mut self, q: String) {
        self.find.query = q;
        self.find.indexed = None;
    }

    /// Recomputes matches when the query or the document changed, and parks the
    /// active match on the first hit at or after the cursor.
    pub fn refresh_hits(&mut self, ctx: &egui::Context) {
        if !self.find.open {
            return;
        }
        let stamp = (self.find.query.clone(), hash_of(&self.text));
        if self.find.indexed.as_ref() == Some(&stamp) {
            return;
        }
        let fresh_query = self.find.indexed.as_ref().map(|(q, _)| q) != Some(&stamp.0);
        self.find.indexed = Some(stamp);
        self.find.hits = find_hits(&self.text, &self.find.query);

        if self.find.hits.is_empty() {
            self.find.current = 0;
            return;
        }
        if fresh_query {
            let from = load_selection(ctx, edit_id())
                .map(|(a, _)| char_idx_to_byte(&self.text, a))
                .unwrap_or(0);
            self.find.current = self
                .find
                .hits
                .iter()
                .position(|&(s, _)| s >= from)
                .unwrap_or(0);
            self.find.scroll_to_hit = true;
        } else {
            self.find.current = self.find.current.min(self.find.hits.len() - 1);
        }
    }

    pub fn draw_find_bar(&mut self, ctx: &egui::Context) {
        if !self.find.open {
            return;
        }
        let p = pal();
        let win95 = theme_kind() == ThemeKind::Win95;
        // Clear of the Win95 caption bar / the Mac fullsize content inset.
        let top = if win95 { 28.0 } else { 12.0 };

        let mut close = false;
        let mut step = 0isize;
        let area = egui::Area::new(egui::Id::new("find_bar"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, top))
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(p.face)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .stroke(if win95 {
                        Stroke::NONE
                    } else {
                        Stroke::new(1.0, p.syntax)
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                        ui.horizontal(|ui| {
                            let mut q = self.find.query.clone();
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut q)
                                    .id(find_id())
                                    .desired_width(150.0)
                                    .font(FontId::new(15.0, FontFamily::Proportional))
                                    .text_color(p.text)
                                    .hint_text(
                                        egui::RichText::new("find").color(p.syntax).size(15.0),
                                    ),
                            );
                            if q != self.find.query {
                                self.query_changed(q);
                            }
                            if self.find.focus_field {
                                field.request_focus();
                                self.find.focus_field = false;
                            }

                            let count = if self.find.query.is_empty() {
                                String::new()
                            } else if self.find.hits.is_empty() {
                                "none".to_owned()
                            } else {
                                format!("{}/{}", self.find.current + 1, self.find.hits.len())
                            };
                            ui.label(egui::RichText::new(count).color(p.syntax).size(13.0));

                            if ui.button("‹").on_hover_text("Previous (⇧⏎)").clicked() {
                                step = -1;
                            }
                            if ui.button("›").on_hover_text("Next (⏎)").clicked() {
                                step = 1;
                            }
                            if ui.button("×").on_hover_text("Close (esc)").clicked() {
                                close = true;
                            }
                        });
                    });
            });

        if win95 {
            win95::raised(
                &ctx.layer_painter(area.response.layer_id),
                area.response.rect,
            );
        }
        if step != 0 {
            self.find.step(step);
        }
        if close {
            self.close_find(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_is_smart_case_and_non_overlapping() {
        assert_eq!(find_hits("aXa xax", "xa"), vec![(1, 3), (4, 6)]);
        assert_eq!(find_hits("aXa xax", "Xa"), vec![(1, 3)]);
        assert_eq!(find_hits("aaaa", "aa"), vec![(0, 2), (2, 4)]);
        assert!(find_hits("anything", "").is_empty());
        // Byte offsets stay valid around multi-byte characters.
        assert_eq!(find_hits("é far", "far"), vec![(3, 6)]);
    }
}
