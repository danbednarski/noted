//! Noted: a single-document Markdown notes app. See CLAUDE.md for the map.
//!
//! This file owns the app state and the per-frame `update` loop; everything
//! else lives in a module named for what it does.

mod cursor;
mod edit;
mod find;
#[cfg(target_os = "macos")]
mod mac_ime;
mod markdown;
mod menu;
mod storage;
mod theme;
mod win95;

use cursor::{edit_id, load_selection, set_cursor, set_selection};
use find::Find;
use menu::AppMenu;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use storage::{atomic_save, Config};
use theme::{apply_style, pal, set_theme_kind, theme_kind, ThemeKind, BODY_SIZE};

/// Debounce between the last keystroke and the write to disk.
const SAVE_DELAY: Duration = Duration::from_millis(500);

fn main() -> eframe::Result<()> {
    let cfg = Config::load();
    set_theme_kind(ThemeKind::from_u8(cfg.theme));

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([560.0, 640.0])
        .with_resizable(false)
        .with_title("noted")
        .with_title_shown(false)
        .with_titlebar_shown(false)
        .with_titlebar_buttons_shown(false)
        .with_fullsize_content_view(true);
    if let Some((x, y)) = cfg.pos {
        viewport = viewport.with_position([x, y]);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "noted",
        options,
        Box::new(|cc| {
            let menu = AppMenu::install(&cc.egui_ctx, theme_kind());
            apply_style(&cc.egui_ctx);
            Ok(Box::new(NotedApp::load(menu)))
        }),
    )
}

pub struct NotedApp {
    text: String,
    path: PathBuf,
    dirty: bool,
    last_change: Instant,
    last_saved: String,
    menu: AppMenu,
    last_pos: Option<(f32, f32)>,
    find: Find,
}

impl NotedApp {
    fn load(menu: AppMenu) -> Self {
        let path = storage::note_path();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Self {
            last_saved: text.clone(),
            text,
            path,
            dirty: false,
            last_change: Instant::now(),
            menu,
            last_pos: None,
            find: Find::default(),
        }
    }

    /// Marks the document as edited by something other than the `TextEdit`.
    fn touch(&mut self) {
        self.dirty = true;
        self.last_change = Instant::now();
    }

    fn current_config(&self) -> Config {
        Config {
            theme: theme_kind() as u8,
            pos: self.last_pos,
        }
    }

    fn poll_menu(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.menu.poll() {
            set_theme_kind(t);
            apply_style(ctx);
            self.current_config().save();
        }
    }

    fn maybe_save(&mut self) {
        if !self.dirty || self.last_change.elapsed() < SAVE_DELAY {
            return;
        }
        if self.text == self.last_saved || atomic_save(&self.path, &self.text).is_ok() {
            self.last_saved = self.text.clone();
            self.dirty = false;
        }
    }

    /// Editor shortcuts that must run *before* the `TextEdit` sees the frame's
    /// input, so the widget never gets the chance to act on them itself.
    fn handle_editor_keys(&mut self, ctx: &egui::Context) {
        let edit_id = edit_id();

        // Cmd/Ctrl + B / I / U / Shift-H wrap (or unwrap) the selection.
        // Highlight needs Shift because macOS owns Cmd-H (Hide).
        let marker = ctx.input_mut(|i| {
            use egui::{Key, Modifiers as M};
            [
                (M::NONE, Key::B, "**"),
                (M::NONE, Key::I, "*"),
                (M::NONE, Key::U, "__"),
                (M::SHIFT, Key::H, "=="),
            ]
            .into_iter()
            .find(|&(extra, key, _)| {
                i.consume_key(M::COMMAND | extra, key) || i.consume_key(M::CTRL | extra, key)
            })
            .map(|(_, _, m)| m)
        });
        if let (Some(m), Some(sel)) = (marker, load_selection(ctx, edit_id)) {
            let (a, b) = edit::toggle_emphasis(&mut self.text, sel, m);
            set_selection(ctx, edit_id, a, b);
            self.touch();
        }

        // Tab / Shift+Tab indent and outdent instead of moving focus.
        let (tab, shift_tab) = ctx.input_mut(|i| {
            let (mut tab, mut shift_tab) = (false, false);
            i.events.retain(|e| match e {
                egui::Event::Key {
                    key: egui::Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } if !(modifiers.command || modifiers.ctrl || modifiers.alt) => {
                    tab |= !modifiers.shift;
                    shift_tab |= modifiers.shift;
                    false
                }
                _ => true,
            });
            (tab, shift_tab)
        });
        if tab || shift_tab {
            if let Some(sel) = load_selection(ctx, edit_id) {
                if let Some((a, b)) = edit::shift_indent(&mut self.text, sel, shift_tab) {
                    set_selection(ctx, edit_id, a, b);
                    self.touch();
                }
            }
        }
    }

    /// Bridges macOS's press-and-hold accent panel into the editor. See [`mac_ime`].
    #[cfg(target_os = "macos")]
    fn apply_accent_panel(&mut self, ctx: &egui::Context) {
        mac_ime::install(ctx);
        if let Some(caret) = mac_ime::take_replacement(&mut self.text) {
            set_cursor(ctx, edit_id(), caret);
            self.dirty = true;
        }
        // Characters the panel took over: the repeats it suppressed while open,
        // and the keystroke that picked an accent we have already applied.
        let mut drop_text = mac_ime::text_events_to_drop();
        if drop_text > 0 {
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    let drop = drop_text > 0 && matches!(e, egui::Event::Text(_));
                    drop_text -= usize::from(drop);
                    !drop
                })
            });
        }
    }
}

impl eframe::App for NotedApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b, _] = pal().face.to_array();
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_menu(ctx);

        #[cfg(target_os = "macos")]
        self.apply_accent_panel(ctx);

        // Remember window position for next launch.
        if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
            self.last_pos = Some((r.min.x, r.min.y));
        }

        // The find bar runs before the editor so a query typed this frame is
        // already reflected in the highlighting the editor lays out below.
        self.handle_find_keys(ctx);
        self.draw_find_bar(ctx);
        self.refresh_hits(ctx);
        let hits = if self.find.open {
            self.find.hits.clone()
        } else {
            Vec::new()
        };
        let hit = self.find.hit();

        if ctx.memory(|m| m.has_focus(edit_id())) {
            self.handle_editor_keys(ctx);
        }

        let p = pal();
        let win95 = theme_kind() == ThemeKind::Win95;
        let frame = egui::Frame::NONE
            .fill(p.face)
            .inner_margin(egui::Margin::ZERO);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let full = ui.max_rect();
            let editor_rect = if win95 {
                win95::draw_chrome(ui, full)
            } else {
                full
            };

            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(editor_rect), |ui| {
                let h_pad: f32 = if win95 { 0.0 } else { 28.0 };
                let column = (ui.available_width() - 2.0 * h_pad).max(120.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.add_space(if win95 { 0.0 } else { 32.0 });
                        ui.horizontal(|ui| {
                            ui.add_space(h_pad);
                            ui.allocate_ui_with_layout(
                                egui::vec2(column, ui.available_height()),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| self.show_editor(ui, column, &hits, hit),
                            );
                            ui.add_space(h_pad);
                        });
                        ui.add_space(64.0);
                    });
            });
        });

        #[cfg(target_os = "macos")]
        {
            let focused = ctx.memory(|m| m.has_focus(edit_id()));
            let selection = load_selection(ctx, edit_id())
                .filter(|_| focused)
                .map(|(a, b)| (self.text.as_str(), a, b));
            mac_ime::publish_selection(selection);
        }

        if self.dirty {
            ctx.request_repaint_after(SAVE_DELAY + Duration::from_millis(50));
        }
        self.maybe_save();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.text != self.last_saved {
            let _ = atomic_save(&self.path, &self.text);
        }
        self.current_config().save();
    }
}

impl NotedApp {
    /// The `TextEdit` itself, plus the reactions to what it did this frame:
    /// list continuation after Enter, and scrolling to the active search hit.
    fn show_editor(
        &mut self,
        ui: &mut egui::Ui,
        column: f32,
        hits: &[(usize, usize)],
        hit: Option<(usize, usize)>,
    ) {
        let ctx = ui.ctx().clone();
        let edit_id = edit_id();
        let prev_len = self.text.len();

        let mut layouter = |ui: &egui::Ui, text: &str, wrap: f32| {
            markdown::cached_galley(ui, text, wrap, hits, hit)
        };
        let o = egui::TextEdit::multiline(&mut self.text)
            .id(edit_id)
            .frame(false)
            .hint_text(
                egui::RichText::new("Start writing…")
                    .color(pal().syntax)
                    .size(BODY_SIZE),
            )
            .background_color(pal().field)
            .text_color(pal().text)
            .desired_width(column)
            .desired_rows(32)
            .margin(egui::Margin::ZERO)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui);

        autoscroll_drag(ui, &o.response);

        // Bring the active search match into view. Done from the galley so it
        // works while the find field, not the editor, holds focus.
        if self.find.scroll_to_hit {
            self.find.scroll_to_hit = false;
            if let Some((s, e)) = hit {
                let pos = |byte| {
                    let c = egui::text::CCursor::new(edit::byte_to_char_idx(&self.text, byte));
                    o.galley.pos_from_ccursor(c)
                };
                let r = pos(s)
                    .union(pos(e))
                    .translate(o.galley_pos.to_vec2())
                    .expand(24.0);
                ui.scroll_to_rect(r, None);
            }
        }

        if o.response.changed() {
            self.touch();
            // Exactly one byte grew: a plain keystroke. If it was Enter, carry
            // the list marker onto the new line.
            if self.text.len() == prev_len + 1 {
                if let Some(cr) = o.cursor_range {
                    let caret = cr.primary.ccursor.index;
                    if let Some(c) = edit::continue_list(&mut self.text, caret) {
                        set_cursor(&ctx, edit_id, c);
                    }
                }
            }
        }
    }
}

/// Keeps a drag-selection scrolling while the pointer is past the top or bottom
/// of the view. egui's `TextEdit` only scrolls to the caret after keyboard edits:
/// it snapshots the selection after the pointer has already moved it, so a drag
/// never counts as a change.
fn autoscroll_drag(ui: &egui::Ui, response: &egui::Response) {
    let (Some(pos), true) = (ui.ctx().pointer_interact_pos(), response.dragged()) else {
        return;
    };
    let view = ui.clip_rect();
    let past = if pos.y < view.top() {
        view.top() - pos.y
    } else if pos.y > view.bottom() {
        view.bottom() - pos.y
    } else {
        return;
    };
    // Faster the further out the pointer is, but never stalled at the edge,
    // since a window flush with the menu bar leaves little room above it.
    let speed = 15.0 * (past + 8.0f32.copysign(past));
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    ui.scroll_with_delta(egui::vec2(0.0, speed * dt));
    ui.ctx().request_repaint();
}
