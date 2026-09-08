//! Reading and writing the editor's caret through egui's widget memory.
//!
//! egui speaks *char* indices here. Every function in [`crate::edit`] that
//! mutates text takes and returns char indices for that reason; convert with
//! `char_idx_to_byte` / `byte_to_char_idx` before slicing.
//!
//! State written with `set_*` is picked up by the `TextEdit` on its next
//! `show`, so a `load_*` in the same frame before that still sees the old value.

use egui::text::{CCursor, CCursorRange};

/// Id of the note editor widget.
pub fn edit_id() -> egui::Id {
    egui::Id::new("note_editor")
}

pub fn set_cursor(ctx: &egui::Context, id: egui::Id, char_idx: usize) {
    set_selection(ctx, id, char_idx, char_idx);
}

pub fn set_selection(ctx: &egui::Context, id: egui::Id, a: usize, b: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let range = CCursorRange::two(CCursor::new(a), CCursor::new(b));
        state.cursor.set_char_range(Some(range));
        state.store(ctx, id);
    }
}

/// The selection as sorted char indices (equal if there is no selection).
pub fn load_selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
    let state = egui::TextEdit::load_state(ctx, id)?;
    let [a, b] = state.cursor.char_range()?.sorted();
    Some((a.index, b.index))
}
