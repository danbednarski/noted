use eframe::egui;
use egui::{
    text::{LayoutJob, LayoutSection},
    Color32, FontFamily, FontId, Stroke, TextFormat,
};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, PredefinedMenuItem, Submenu};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SynFontStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

// ---- Theme system ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThemeKind {
    CandyCane,
    Win95,
    Dracula,
}

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);

fn theme_kind() -> ThemeKind {
    match ACTIVE_THEME.load(Ordering::Relaxed) {
        1 => ThemeKind::Win95,
        2 => ThemeKind::Dracula,
        _ => ThemeKind::CandyCane,
    }
}

fn set_theme_kind(t: ThemeKind) {
    ACTIVE_THEME.store(
        match t {
            ThemeKind::CandyCane => 0,
            ThemeKind::Win95 => 1,
            ThemeKind::Dracula => 2,
        },
        Ordering::Relaxed,
    );
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

struct Pal {
    face: Color32,  // window / form background
    field: Color32, // text editing surface
    text: Color32,
    text_bold: Color32,
    heading: Color32,
    syntax: Color32,
    code: Color32,
    code_bg: Color32,
    quote: Color32,
    link: Color32,
    cursor: Color32,
    selection: Color32,
    find: Color32,     // background of every search match
    find_cur: Color32, // background of the active search match
}

fn pal() -> Pal {
    match theme_kind() {
        ThemeKind::CandyCane => Pal {
            face: rgb(0xff, 0xff, 0xff),
            field: rgb(0xff, 0xff, 0xff),
            text: rgb(0x15, 0x11, 0x0d),
            text_bold: rgb(0x00, 0x00, 0x00),
            heading: rgb(0xc8, 0x10, 0x2e),
            syntax: rgb(0xc4, 0xa8, 0xab),
            code: rgb(0x6e, 0x1a, 0x2a),
            code_bg: rgb(0xfa, 0xf0, 0xf1),
            quote: rgb(0x6a, 0x60, 0x60),
            link: rgb(0x0a, 0x8a, 0x3a),
            cursor: rgb(0xc8, 0x10, 0x2e),
            selection: rgb(0xff, 0xd9, 0xde),
            find: rgb(0xff, 0xee, 0xb0),
            find_cur: rgb(0xff, 0xb3, 0x4d),
        },
        ThemeKind::Win95 => Pal {
            face: rgb(0xc0, 0xc0, 0xc0),
            field: rgb(0xff, 0xff, 0xff),
            text: rgb(0x00, 0x00, 0x00),
            text_bold: rgb(0x00, 0x00, 0x00),
            heading: rgb(0x00, 0x00, 0x80),
            syntax: rgb(0x80, 0x80, 0x80),
            code: rgb(0x00, 0x00, 0x80),
            code_bg: rgb(0xff, 0xff, 0xff),
            quote: rgb(0x40, 0x40, 0x40),
            link: rgb(0x00, 0x00, 0xff),
            cursor: rgb(0x00, 0x00, 0x00),
            selection: rgb(0xa6, 0xc0, 0xe0),
            find: rgb(0xff, 0xff, 0x00),
            find_cur: rgb(0xff, 0x99, 0x00),
        },
        // Dracula — https://draculatheme.com/contribute (official spec)
        ThemeKind::Dracula => Pal {
            face: rgb(0x28, 0x2a, 0x36),
            field: rgb(0x28, 0x2a, 0x36),
            text: rgb(0xf8, 0xf8, 0xf2),
            text_bold: rgb(0xff, 0xff, 0xff),
            heading: rgb(0xbd, 0x93, 0xf9),  // purple
            syntax: rgb(0x62, 0x72, 0xa4),   // comment
            code: rgb(0xf1, 0xfa, 0x8c),     // yellow
            code_bg: rgb(0x44, 0x47, 0x5a),  // current line
            quote: rgb(0x62, 0x72, 0xa4),
            link: rgb(0x8b, 0xe9, 0xfd),     // cyan
            cursor: rgb(0xff, 0x79, 0xc6),   // pink
            selection: rgb(0x44, 0x47, 0x5a),
            find: rgb(0x55, 0x5c, 0x42),     // dim yellow wash
            find_cur: rgb(0x8a, 0x92, 0x5e), // brighter yellow wash
        },
    }
}

const BODY_SIZE: f32 = 18.5;
const H1_SIZE: f32 = 33.0;
const H2_SIZE: f32 = 27.0;
const H3_SIZE: f32 = 23.0;
const H4_SIZE: f32 = 20.0;

fn main() -> eframe::Result<()> {
    let cfg = Config::load();
    ACTIVE_THEME.store(cfg.theme.min(2), Ordering::Relaxed);

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

// ---- Native macOS menu bar ----

struct AppMenu {
    _menu: Menu,
    candy_id: MenuId,
    win95_id: MenuId,
    dracula_id: MenuId,
    candy_item: CheckMenuItem,
    win95_item: CheckMenuItem,
    dracula_item: CheckMenuItem,
    events: Receiver<MenuEvent>,
}

impl AppMenu {
    fn install(ctx: &egui::Context, initial: ThemeKind) -> Self {
        let menu = Menu::new();

        let app_menu = Submenu::new("noted", true);
        let _ = app_menu.append_items(&[
            &PredefinedMenuItem::about(Some("About noted"), None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ]);

        let candy_item =
            CheckMenuItem::new("Candy Cane", true, initial == ThemeKind::CandyCane, None);
        let win95_item =
            CheckMenuItem::new("Windows 95", true, initial == ThemeKind::Win95, None);
        let dracula_item =
            CheckMenuItem::new("Dracula", true, initial == ThemeKind::Dracula, None);
        let theme_menu = Submenu::new("Theme", true);
        let _ = theme_menu.append_items(&[&candy_item, &win95_item, &dracula_item]);

        let _ = menu.append_items(&[&app_menu, &theme_menu]);

        #[cfg(target_os = "macos")]
        menu.init_for_nsapp();

        let candy_id = candy_item.id().clone();
        let win95_id = win95_item.id().clone();
        let dracula_id = dracula_item.id().clone();

        // Bridge muda's global menu events to our app + wake the egui loop.
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx2.request_repaint();
        }));

        Self {
            _menu: menu,
            candy_id,
            win95_id,
            dracula_id,
            candy_item,
            win95_item,
            dracula_item,
            events: rx,
        }
    }
}

fn font_static(bytes: &'static [u8]) -> std::sync::Arc<egui::FontData> {
    std::sync::Arc::new(egui::FontData::from_static(bytes))
}

fn apply_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "courier".to_owned(),
        font_static(include_bytes!("../assets/CourierPrime-Regular.ttf")),
    );
    fonts.font_data.insert(
        "serif".to_owned(),
        font_static(include_bytes!("../assets/SourceSerif4-Regular.ttf")),
    );
    fonts.font_data.insert(
        "serif_bold".to_owned(),
        font_static(include_bytes!("../assets/SourceSerif4-Semibold.ttf")),
    );
    fonts.font_data.insert(
        "serif_italic".to_owned(),
        font_static(include_bytes!("../assets/SourceSerif4-It.ttf")),
    );
    fonts.font_data.insert(
        "win95".to_owned(),
        font_static(include_bytes!("../assets/R95-sans.ttf")),
    );

    let (prop, bold, ital) = match theme_kind() {
        ThemeKind::CandyCane | ThemeKind::Dracula => {
            ("serif", "serif_bold", "serif_italic")
        }
        ThemeKind::Win95 => ("win95", "win95", "win95"),
    };

    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "courier".to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, prop.to_owned());
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), vec![bold.to_owned()]);
    fonts
        .families
        .insert(FontFamily::Name("italic".into()), vec![ital.to_owned()]);
    ctx.set_fonts(fonts);

    let p = pal();
    let mut style = (*ctx.style()).clone();
    style.visuals.override_text_color = Some(p.text);
    style.visuals.panel_fill = p.face;
    style.visuals.window_fill = p.face;
    style.visuals.extreme_bg_color = p.field;
    style.visuals.faint_bg_color = p.field;
    style.visuals.code_bg_color = p.code_bg;
    style.visuals.selection.bg_fill = p.selection;
    style.visuals.selection.stroke = Stroke::NONE;
    style.visuals.text_cursor.stroke = Stroke::new(1.6, p.cursor);
    style.visuals.window_stroke = Stroke::NONE;
    style.spacing.item_spacing = egui::vec2(0.0, 6.0);

    // Widgets (only used by the find bar) follow the palette instead of egui's
    // stock dark theme.
    let w = &mut style.visuals.widgets;
    for v in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        v.bg_fill = p.face;
        v.weak_bg_fill = p.face;
        v.bg_stroke = Stroke::new(1.0, p.syntax);
        v.fg_stroke = Stroke::new(1.0, p.text);
        v.corner_radius = egui::CornerRadius::ZERO;
        v.expansion = 0.0;
    }
    w.hovered.weak_bg_fill = p.code_bg;
    w.active.weak_bg_fill = p.selection;
    ctx.set_style(style);
}

struct NotedApp {
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
        let path = note_path();
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

    fn current_config(&self) -> Config {
        Config {
            theme: ACTIVE_THEME.load(Ordering::Relaxed),
            pos: self.last_pos,
        }
    }

    fn poll_menu(&mut self, ctx: &egui::Context) {
        let mut new_theme = None;
        while let Ok(ev) = self.menu.events.try_recv() {
            if ev.id == self.menu.candy_id {
                new_theme = Some(ThemeKind::CandyCane);
            } else if ev.id == self.menu.win95_id {
                new_theme = Some(ThemeKind::Win95);
            } else if ev.id == self.menu.dracula_id {
                new_theme = Some(ThemeKind::Dracula);
            }
        }
        if let Some(t) = new_theme {
            set_theme_kind(t);
            self.menu
                .candy_item
                .set_checked(t == ThemeKind::CandyCane);
            self.menu.win95_item.set_checked(t == ThemeKind::Win95);
            self.menu
                .dracula_item
                .set_checked(t == ThemeKind::Dracula);
            apply_style(ctx);
            self.current_config().save();
        }
    }

    fn maybe_save(&mut self) {
        if !self.dirty {
            return;
        }
        if self.last_change.elapsed() < Duration::from_millis(500) {
            return;
        }
        if self.text == self.last_saved {
            self.dirty = false;
            return;
        }
        if atomic_save(&self.path, &self.text).is_ok() {
            self.last_saved = self.text.clone();
            self.dirty = false;
        }
    }
}

// ---------- Find ----------

fn edit_id() -> egui::Id {
    egui::Id::new("note_editor")
}

fn find_id() -> egui::Id {
    egui::Id::new("find_field")
}

#[derive(Default)]
struct Find {
    open: bool,
    query: String,
    /// Byte ranges of every match, in document order.
    hits: Vec<(usize, usize)>,
    current: usize,
    /// The query/text the current `hits` were computed from.
    indexed: Option<(String, u64)>,
    focus_field: bool,
    scroll_to_hit: bool,
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

    fn hit(&self) -> Option<(usize, usize)> {
        if self.open {
            self.hits.get(self.current).copied()
        } else {
            None
        }
    }
}

fn text_hash(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// All non-overlapping occurrences of `needle`, as byte ranges.
///
/// Smart case: a query typed in all-lowercase matches case-insensitively.
/// Lowercasing is ASCII-only so byte offsets stay valid for the original text.
fn find_hits(hay: &str, needle: &str) -> Vec<(usize, usize)> {
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
    fn handle_find_keys(&mut self, ctx: &egui::Context) {
        let cmd = egui::Modifiers::COMMAND;
        let cmd_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);

        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::F)) {
            // Seed the query from the editor selection, the way every other
            // macOS app does.
            if let Some((a, b)) = load_selection(ctx, edit_id()) {
                if a != b {
                    let sel = &self.text[char_idx_to_byte(&self.text, a)
                        ..char_idx_to_byte(&self.text, b)];
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
                || (field_focused
                    && i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter))
        });
        let next = ctx.input_mut(|i| {
            i.consume_key(cmd, egui::Key::G)
                || (field_focused
                    && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
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
    fn refresh_hits(&mut self, ctx: &egui::Context) {
        if !self.find.open {
            return;
        }
        let stamp = (self.find.query.clone(), text_hash(&self.text));
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

    fn draw_find_bar(&mut self, ctx: &egui::Context) {
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
                                        egui::RichText::new("find")
                                            .color(p.syntax)
                                            .size(15.0),
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
                            ui.label(
                                egui::RichText::new(count).color(p.syntax).size(13.0),
                            );

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
            raised(
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

fn note_path() -> PathBuf {
    if let Some(dir) = dirs::data_dir() {
        dir.join("noted").join("notes.md")
    } else {
        PathBuf::from("notes.md")
    }
}

fn config_path() -> PathBuf {
    note_path().with_file_name("config")
}

/// Crash-safe write: roll a `.bak`, write to a temp file, fsync, then rename
/// over the target (rename is atomic on the same filesystem).
fn atomic_save(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let _ = std::fs::copy(path, path.with_extension("bak"));
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[derive(Default)]
struct Config {
    theme: u8,
    pos: Option<(f32, f32)>,
}

impl Config {
    fn load() -> Self {
        let mut c = Config::default();
        if let Ok(s) = std::fs::read_to_string(config_path()) {
            for line in s.lines() {
                if let Some(v) = line.strip_prefix("theme=") {
                    if let Ok(n) = v.trim().parse() {
                        c.theme = n;
                    }
                } else if let Some(v) = line.strip_prefix("pos=") {
                    let mut it = v.split(',');
                    if let (Some(a), Some(b)) = (it.next(), it.next()) {
                        if let (Ok(x), Ok(y)) =
                            (a.trim().parse::<f32>(), b.trim().parse::<f32>())
                        {
                            c.pos = Some((x, y));
                        }
                    }
                }
            }
        }
        c
    }

    fn save(&self) {
        let mut s = format!("theme={}\n", self.theme);
        if let Some((x, y)) = self.pos {
            s.push_str(&format!("pos={},{}\n", x, y));
        }
        let _ = atomic_save(&config_path(), &s);
    }
}

impl eframe::App for NotedApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b, _] = pal().face.to_array();
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_menu(ctx);

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

        let p = pal();
        let win95 = theme_kind() == ThemeKind::Win95;
        let frame = egui::Frame::NONE
            .fill(p.face)
            .inner_margin(egui::Margin::ZERO);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let full = ui.max_rect();
            let editor_rect = if win95 {
                draw_win95_chrome(ui, full)
            } else {
                full
            };

            ui.allocate_ui_at_rect(editor_rect, |ui| {
              let h_pad: f32 = if win95 { 0.0 } else { 28.0 };
              let avail = ui.available_width();
              let column = (avail - 2.0 * h_pad).max(120.0);

              egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.add_space(if win95 { 0.0 } else { 32.0 });
                    ui.horizontal(|ui| {
                        ui.add_space(h_pad);
                        ui.allocate_ui_with_layout(
                            egui::vec2(column, ui.available_height()),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                let edit_id = edit_id();

                                // Pre-intercept Tab / Shift+Tab so they don't move focus
                                // or get swallowed by the TextEdit.
                                let focused = ui.memory(|m| m.has_focus(edit_id));

                                // Cmd/Ctrl + B / I / U wrap (or unwrap) the selection.
                                if focused {
                                    let mut marker = None;
                                    ui.input_mut(|i| {
                                        for (key, m) in [
                                            (egui::Key::B, "**"),
                                            (egui::Key::I, "*"),
                                            (egui::Key::U, "__"),
                                        ] {
                                            if i.consume_key(egui::Modifiers::COMMAND, key)
                                                || i.consume_key(egui::Modifiers::CTRL, key)
                                            {
                                                marker = Some(m);
                                            }
                                        }
                                    });
                                    if let Some(m) = marker {
                                        if let Some(sel) = load_selection(ctx, edit_id) {
                                            let (a, b) =
                                                toggle_emphasis(&mut self.text, sel, m);
                                            set_selection(ctx, edit_id, a, b);
                                            self.dirty = true;
                                            self.last_change = Instant::now();
                                        }
                                    }
                                }

                                if focused {
                                    let (do_tab, do_shift_tab) = ui.input_mut(|i| {
                                        let mut tab = false;
                                        let mut shift_tab = false;
                                        i.events.retain(|e| match e {
                                            egui::Event::Key {
                                                key: egui::Key::Tab,
                                                pressed: true,
                                                modifiers,
                                                ..
                                            } => {
                                                if modifiers.command
                                                    || modifiers.ctrl
                                                    || modifiers.alt
                                                {
                                                    return true;
                                                }
                                                if modifiers.shift {
                                                    shift_tab = true;
                                                } else {
                                                    tab = true;
                                                }
                                                false
                                            }
                                            _ => true,
                                        });
                                        (tab, shift_tab)
                                    });

                                    if do_tab || do_shift_tab {
                                        if let Some(c_idx) = load_cursor(ctx, edit_id) {
                                            let byte_idx =
                                                char_idx_to_byte(&self.text, c_idx);
                                            let line_start = self.text[..byte_idx]
                                                .rfind('\n')
                                                .map(|i| i + 1)
                                                .unwrap_or(0);
                                            let line_end = self.text[byte_idx..]
                                                .find('\n')
                                                .map(|i| byte_idx + i)
                                                .unwrap_or(self.text.len());
                                            let line = self.text
                                                [line_start..line_end]
                                                .to_string();
                                            let on_list =
                                                parse_list_marker(&line).is_some();

                                            if do_tab && on_list {
                                                self.text.insert_str(line_start, "  ");
                                                set_cursor(ctx, edit_id, c_idx + 2);
                                                self.dirty = true;
                                                self.last_change = Instant::now();
                                            } else if do_shift_tab && on_list {
                                                let bytes = self.text.as_bytes();
                                                let mut rm = 0;
                                                while rm < 2
                                                    && line_start + rm < bytes.len()
                                                    && bytes[line_start + rm] == b' '
                                                {
                                                    rm += 1;
                                                }
                                                if rm > 0 {
                                                    self.text.replace_range(
                                                        line_start..line_start + rm,
                                                        "",
                                                    );
                                                    let line_start_char = self.text
                                                        [..line_start]
                                                        .chars()
                                                        .count();
                                                    let new_c = c_idx
                                                        .saturating_sub(rm)
                                                        .max(line_start_char);
                                                    set_cursor(ctx, edit_id, new_c);
                                                    self.dirty = true;
                                                    self.last_change = Instant::now();
                                                }
                                            } else if do_tab {
                                                // not on a list line — insert literal tab
                                                self.text.insert(byte_idx, '\t');
                                                set_cursor(ctx, edit_id, c_idx + 1);
                                                self.dirty = true;
                                                self.last_change = Instant::now();
                                            }
                                        }
                                    }
                                }

                                let prev_len = self.text.len();

                                let o = {
                                    let mut layouter =
                                        |ui: &egui::Ui, text: &str, wrap: f32| {
                                            cached_galley(ui, text, wrap, &hits, hit)
                                        };

                                    let edit = egui::TextEdit::multiline(&mut self.text)
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
                                        .layouter(&mut layouter);

                                    edit.show(ui)
                                };
                                let (changed, cursor_range) =
                                    (o.response.changed(), o.cursor_range);

                                // Bring the active search match into view. Done
                                // from the galley so it works while the find
                                // field, not the editor, holds focus.
                                if self.find.scroll_to_hit {
                                    self.find.scroll_to_hit = false;
                                    if let Some((s, e)) = hit {
                                        let a = o.galley.pos_from_ccursor(
                                            egui::text::CCursor::new(byte_to_char_idx(
                                                &self.text, s,
                                            )),
                                        );
                                        let b = o.galley.pos_from_ccursor(
                                            egui::text::CCursor::new(byte_to_char_idx(
                                                &self.text, e,
                                            )),
                                        );
                                        let r = a
                                            .union(b)
                                            .translate(o.galley_pos.to_vec2())
                                            .expand(24.0);
                                        ui.scroll_to_rect(r, None);
                                    }
                                }

                                if changed {
                                    self.dirty = true;
                                    self.last_change = Instant::now();

                                    let added = self.text.len() as i64 - prev_len as i64;
                                    if added == 1 {
                                        if let Some(cr) = cursor_range {
                                            let c_idx = cr.primary.ccursor.index;
                                            let byte_idx =
                                                char_idx_to_byte(&self.text, c_idx);
                                            if byte_idx > 0
                                                && self.text.as_bytes()[byte_idx - 1]
                                                    == b'\n'
                                            {
                                                apply_list_continuation(
                                                    ctx,
                                                    edit_id,
                                                    &mut self.text,
                                                    c_idx,
                                                    byte_idx,
                                                );
                                            }
                                        }
                                    }
                                }
                            },
                        );
                        ui.add_space(h_pad);
                    });
                    ui.add_space(64.0);
                });
            });
        });

        if self.dirty {
            ctx.request_repaint_after(Duration::from_millis(550));
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

// ---------- Win95 chrome ----------

const W95_FACE: Color32 = rgb(0xc0, 0xc0, 0xc0);
const W95_WHITE: Color32 = rgb(0xff, 0xff, 0xff);
const W95_LITE: Color32 = rgb(0xdf, 0xdf, 0xdf);
const W95_GRAY: Color32 = rgb(0x80, 0x80, 0x80);
const W95_BLACK: Color32 = rgb(0x00, 0x00, 0x00);
const W95_NAVY: Color32 = rgb(0x00, 0x00, 0x80);

/// One 1px bevel ring: `tl` on top+left, `br` on bottom+right.
fn bevel(p: &egui::Painter, r: egui::Rect, tl: Color32, br: Color32) {
    p.hline(r.left()..=r.right(), r.top(), Stroke::new(1.0, tl));
    p.vline(r.left(), r.top()..=r.bottom(), Stroke::new(1.0, tl));
    p.hline(r.left()..=r.right(), r.bottom() - 1.0, Stroke::new(1.0, br));
    p.vline(r.right() - 1.0, r.top()..=r.bottom(), Stroke::new(1.0, br));
}

/// Classic raised 3D control edge (buttons, window frame).
fn raised(p: &egui::Painter, r: egui::Rect) {
    bevel(p, r, W95_WHITE, W95_BLACK);
    bevel(p, r.shrink(1.0), W95_LITE, W95_GRAY);
}

/// Classic sunken client edge (text fields).
fn sunken(p: &egui::Painter, r: egui::Rect) {
    bevel(p, r, W95_GRAY, W95_WHITE);
    bevel(p, r.shrink(1.0), W95_BLACK, W95_LITE);
}

/// Win95 caption-button symbols, drawn as primitives (no font glyphs).
fn draw_w95_button_glyph(p: &egui::Painter, c: egui::Pos2, act: &str) {
    let ink = Stroke::new(1.0, W95_BLACK);
    match act {
        "min" => {
            // short bar resting near the bottom
            let bar = egui::Rect::from_min_max(
                egui::pos2(c.x - 3.5, c.y + 2.5),
                egui::pos2(c.x + 3.5, c.y + 4.5),
            );
            p.rect_filled(bar, 0.0, W95_BLACK);
        }
        "max" => {
            // window outline with a thick (2px) caption bar on top
            let win = egui::Rect::from_min_max(
                egui::pos2(c.x - 5.0, c.y - 4.5),
                egui::pos2(c.x + 5.0, c.y + 4.5),
            );
            p.rect_stroke(win, 0.0, ink, egui::StrokeKind::Inside);
            let cap = egui::Rect::from_min_max(
                egui::pos2(win.left(), win.top()),
                egui::pos2(win.right(), win.top() + 2.0),
            );
            p.rect_filled(cap, 0.0, W95_BLACK);
        }
        "close" => {
            // an X (drawn twice, 1px apart, for that chunky pixel weight)
            let (a, b) = (4.0_f32, 4.0_f32);
            for off in [0.0_f32, 1.0] {
                p.line_segment(
                    [
                        egui::pos2(c.x - a + off, c.y - b),
                        egui::pos2(c.x + a + off, c.y + b),
                    ],
                    ink,
                );
                p.line_segment(
                    [
                        egui::pos2(c.x + a + off, c.y - b),
                        egui::pos2(c.x - a + off, c.y + b),
                    ],
                    ink,
                );
            }
        }
        _ => {}
    }
}

/// Draws the full Win95/VB6 window: raised frame, navy caption bar with
/// 3D buttons, gray form, sunken white text box. Returns the interior rect
/// where the editor should be placed.
static W95_FULLSCREEN: AtomicBool = AtomicBool::new(false);

fn draw_win95_chrome(ui: &egui::Ui, full: egui::Rect) -> egui::Rect {
    let p = ui.painter();
    p.rect_filled(full, 0.0, W95_FACE);
    raised(p, full);

    let inner = full.shrink(3.0);

    // ---- title bar ----
    let tb_h = 20.0;
    let tb = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), tb_h));
    p.rect_filled(tb, 0.0, W95_NAVY);
    p.text(
        egui::pos2(tb.left() + 5.0, tb.center().y),
        egui::Align2::LEFT_CENTER,
        "noted",
        egui::FontId::new(13.0, FontFamily::Name("bold".into())),
        W95_WHITE,
    );

    // ---- window buttons: close, maximize, minimize (right to left) ----
    let bw = 18.0;
    let bh = 16.0;
    let by = tb.center().y - bh / 2.0;
    let mut bx = tb.right() - 4.0 - bw;
    let mut buttons_left = tb.right();
    for (i, act) in ["close", "max", "min"].iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(bx, by), egui::vec2(bw, bh));
        let resp = ui.interact(r, egui::Id::new(("w95btn", *act)), egui::Sense::click());
        let pressed = resp.is_pointer_button_down_on();

        p.rect_filled(r, 0.0, W95_FACE);
        if pressed {
            sunken(p, r);
        } else {
            raised(p, r);
        }
        let nudge = if pressed { egui::vec2(1.0, 1.0) } else { egui::Vec2::ZERO };
        draw_w95_button_glyph(p, r.center() + nudge, act);

        if resp.clicked() {
            match *act {
                "close" => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                "min" => ui
                    .ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                "max" => {
                    let next = !W95_FULLSCREEN.load(Ordering::Relaxed);
                    W95_FULLSCREEN.store(next, Ordering::Relaxed);
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Fullscreen(next));
                }
                _ => {}
            }
        }

        buttons_left = buttons_left.min(r.left());
        bx -= bw + if i == 0 { 2.0 } else { 1.0 };
    }

    // ---- draggable caption (left of the buttons) ----
    let drag_rect = egui::Rect::from_min_max(
        tb.min,
        egui::pos2(buttons_left - 4.0, tb.bottom()),
    );
    let drag = ui.interact(
        drag_rect,
        egui::Id::new("w95_caption_drag"),
        egui::Sense::click_and_drag(),
    );
    if drag.drag_started() || (drag.is_pointer_button_down_on() && drag.dragged()) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    // ---- form + sunken text box ----
    let form = egui::Rect::from_min_max(egui::pos2(inner.left(), tb.bottom()), inner.max);
    let box_rect = form.shrink(7.0);
    p.rect_filled(box_rect, 0.0, W95_WHITE);
    sunken(p, box_rect);

    // interior available to the editor (inside the 2px sunken edge + padding)
    box_rect.shrink(2.0).shrink2(egui::vec2(6.0, 5.0))
}

// ---------- Markdown layouting ----------

fn body_font() -> FontId {
    FontId::new(BODY_SIZE, FontFamily::Proportional)
}

fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

fn italic_family() -> FontFamily {
    FontFamily::Name("italic".into())
}

fn syntax_fmt(font: FontId) -> TextFormat {
    TextFormat {
        font_id: font,
        color: pal().syntax,
        ..Default::default()
    }
}

fn layout_markdown(text: &str, wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.wrap.break_anywhere = false;

    let mut highlighter: Option<HighlightLines<'static>> = None;
    let mut first = true;

    for line in text.split('\n') {
        if !first {
            job.append(
                "\n",
                0.0,
                TextFormat {
                    font_id: body_font(),
                    color: pal().text,
                    ..Default::default()
                },
            );
        }
        first = false;

        // Fenced code block delimiter
        if line.trim_start().starts_with("```") {
            let fence_fmt = TextFormat {
                font_id: code_font(),
                color: pal().syntax,
                ..Default::default()
            };
            job.append(line, 0.0, fence_fmt);

            if highlighter.is_some() {
                highlighter = None;
            } else {
                let lang = line
                    .trim_start()
                    .trim_start_matches('`')
                    .trim();
                let ss = syntax_set();
                let syntax = ss
                    .find_syntax_by_token(lang)
                    .or_else(|| ss.find_syntax_by_name(lang))
                    .or_else(|| ss.find_syntax_by_extension(lang))
                    .unwrap_or_else(|| ss.find_syntax_plain_text());
                highlighter = Some(HighlightLines::new(syntax, code_theme()));
            }
            continue;
        }

        // Inside a fenced code block
        if let Some(hl) = highlighter.as_mut() {
            let line_nl = format!("{}\n", line);
            match hl.highlight_line(&line_nl, syntax_set()) {
                Ok(ranges) => {
                    for (sty, mut s) in ranges {
                        if s.ends_with('\n') {
                            s = &s[..s.len() - 1];
                        }
                        if s.is_empty() {
                            continue;
                        }
                        job.append(s, 0.0, syntect_fmt(sty));
                    }
                }
                Err(_) => {
                    job.append(line, 0.0, plain_code_fmt());
                }
            }
            continue;
        }

        layout_line(&mut job, line);
    }
    job
}

/// Paints search-match backgrounds over a finished job.
///
/// `layout_markdown` appends every byte of the source exactly once and in
/// order, so a job section's `byte_range` indexes the original text and match
/// ranges can simply be sliced into the section list.
fn paint_hits(job: &mut LayoutJob, hits: &[(usize, usize)], current: Option<(usize, usize)>) {
    if hits.is_empty() {
        return;
    }
    let p = pal();
    let mut out: Vec<LayoutSection> = Vec::with_capacity(job.sections.len() + 2 * hits.len());

    for sec in job.sections.drain(..) {
        let (s, e) = (sec.byte_range.start, sec.byte_range.end);
        if s >= e {
            out.push(sec);
            continue;
        }
        // Cut the section at every match boundary falling inside it. Matches can
        // be one edit stale (the editor mutates the text before it lays it out),
        // so offsets that no longer land on a char boundary are dropped rather
        // than handed to the layouter.
        let mut cuts = vec![s];
        for &(hs, he) in hits {
            if he <= s {
                continue;
            }
            if hs >= e {
                break;
            }
            if hs > s && job.text.is_char_boundary(hs) {
                cuts.push(hs);
            }
            if he < e && job.text.is_char_boundary(he) {
                cuts.push(he);
            }
        }
        cuts.push(e);
        cuts.dedup();

        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let mut format = sec.format.clone();
            if let Some(&h) = hits.iter().find(|&&(hs, he)| hs <= a && b <= he) {
                format.background = if Some(h) == current {
                    p.find_cur
                } else {
                    p.find
                };
            }
            out.push(LayoutSection {
                leading_space: if a == s { sec.leading_space } else { 0.0 },
                byte_range: a..b,
                format,
            });
        }
    }
    job.sections = out;
}

fn code_font() -> FontId {
    FontId::new(BODY_SIZE * 0.95, FontFamily::Monospace)
}

fn plain_code_fmt() -> TextFormat {
    TextFormat {
        font_id: code_font(),
        color: pal().text,
        ..Default::default()
    }
}

fn syntect_fmt(sty: syntect::highlighting::Style) -> TextFormat {
    let mut fmt = TextFormat {
        font_id: code_font(),
        color: Color32::from_rgba_unmultiplied(
            sty.foreground.r,
            sty.foreground.g,
            sty.foreground.b,
            sty.foreground.a,
        ),
        ..Default::default()
    };
    if sty.font_style.contains(SynFontStyle::ITALIC) {
        fmt.italics = true;
    }
    fmt
}

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_LIGHT: OnceLock<Theme> = OnceLock::new();
static THEME_DARK: OnceLock<Theme> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn code_theme() -> &'static Theme {
    let dark = matches!(theme_kind(), ThemeKind::Dracula);
    let cell = if dark { &THEME_DARK } else { &THEME_LIGHT };
    cell.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        let name = if dark { "base16-mocha.dark" } else { "InspiredGitHub" };
        ts.themes
            .get(name)
            .cloned()
            .unwrap_or_else(|| ts.themes.values().next().unwrap().clone())
    })
}

fn layout_line(job: &mut LayoutJob, line: &str) {
    // Detect block-level prefix
    let trimmed_start = line.len() - line.trim_start_matches(' ').len();
    let indent = &line[..trimmed_start];
    let rest = &line[trimmed_start..];

    // headings
    for (n, size) in [(1, H1_SIZE), (2, H2_SIZE), (3, H3_SIZE), (4, H4_SIZE)] {
        let hashes = "#".repeat(n);
        let with_space = format!("{} ", hashes);
        if rest.starts_with(&with_space) {
            let base = TextFormat {
                font_id: FontId::new(size, bold_family()),
                color: pal().heading,
                ..Default::default()
            };
            if !indent.is_empty() {
                job.append(indent, 0.0, body_fmt());
            }
            // syntax: # or ## etc, plus the trailing space, in dim
            let prefix = &rest[..with_space.len()];
            job.append(
                prefix,
                0.0,
                TextFormat {
                    font_id: FontId::new(size, FontFamily::Proportional),
                    color: pal().syntax,
                    ..Default::default()
                },
            );
            layout_inline(job, &rest[with_space.len()..], &base);
            return;
        }
    }

    // blockquote
    if let Some(after) = rest.strip_prefix("> ") {
        if !indent.is_empty() {
            job.append(indent, 0.0, body_fmt());
        }
        job.append(
            "> ",
            0.0,
            TextFormat {
                font_id: body_font(),
                color: pal().syntax,
                ..Default::default()
            },
        );
        let mut base = body_fmt();
        base.color = pal().quote;
        base.italics = true;
        layout_inline(job, after, &base);
        return;
    }

    // unordered list
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        if !indent.is_empty() {
            job.append(indent, 0.0, indent_fmt());
        }
        // bullet glyph, just the marker char, prominent
        job.append(
            &rest[..1],
            0.0,
            TextFormat {
                font_id: body_font(),
                color: pal().heading,
                ..Default::default()
            },
        );
        // the trailing space gets extra room so content sits further right
        job.append(
            &rest[1..2],
            0.0,
            TextFormat {
                font_id: body_font(),
                color: pal().text,
                extra_letter_spacing: 6.0,
                ..Default::default()
            },
        );
        layout_inline(job, &rest[2..], &body_fmt());
        return;
    }

    // ordered list: "1. " / "12. " etc
    if let Some(dot_idx) = rest.find(". ") {
        let head = &rest[..dot_idx];
        if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) {
            if !indent.is_empty() {
                job.append(indent, 0.0, indent_fmt());
            }
            // digits + dot in red
            job.append(
                &rest[..dot_idx + 1],
                0.0,
                TextFormat {
                    font_id: body_font(),
                    color: pal().heading,
                    ..Default::default()
                },
            );
            // trailing space with extra spacing
            job.append(
                &rest[dot_idx + 1..dot_idx + 2],
                0.0,
                TextFormat {
                    font_id: body_font(),
                    color: pal().text,
                    extra_letter_spacing: 6.0,
                    ..Default::default()
                },
            );
            layout_inline(job, &rest[dot_idx + 2..], &body_fmt());
            return;
        }
    }

    // horizontal rule: --- or ***
    if rest == "---" || rest == "***" || rest == "___" {
        if !indent.is_empty() {
            job.append(indent, 0.0, body_fmt());
        }
        job.append(
            rest,
            0.0,
            TextFormat {
                font_id: body_font(),
                color: pal().syntax,
                ..Default::default()
            },
        );
        return;
    }

    // default paragraph
    if !indent.is_empty() {
        job.append(indent, 0.0, body_fmt());
    }
    layout_inline(job, rest, &body_fmt());
}

fn body_fmt() -> TextFormat {
    TextFormat {
        font_id: body_font(),
        color: pal().text,
        ..Default::default()
    }
}

// Renders leading whitespace with extra letter spacing so nested-list indents look
// like real indentation without changing the underlying character count.
fn indent_fmt() -> TextFormat {
    TextFormat {
        font_id: body_font(),
        color: pal().text,
        extra_letter_spacing: 8.0,
        ..Default::default()
    }
}

fn layout_inline(job: &mut LayoutJob, text: &str, base: &TextFormat) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut plain_start = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // bold **...**
        if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = find_close(text, i + 2, "**") {
                flush_plain(job, &text[plain_start..i], base);
                let syn = syntax_fmt(base.font_id.clone());
                job.append("**", 0.0, syn.clone());
                let mut bold = base.clone();
                bold.font_id = FontId::new(base.font_id.size, bold_family());
                bold.color = pal().text_bold;
                layout_inline_styled(job, &text[i + 2..end], &bold);
                job.append("**", 0.0, syn);
                i = end + 2;
                plain_start = i;
                continue;
            }
        }

        // italic *...* (not bold, not list marker)
        if b == b'*'
            && (i + 1 >= bytes.len() || bytes[i + 1] != b'*')
            && (i == 0 || bytes[i - 1] != b'*')
        {
            if let Some(end) = find_close_single(text, i + 1, b'*') {
                flush_plain(job, &text[plain_start..i], base);
                let syn = syntax_fmt(base.font_id.clone());
                job.append("*", 0.0, syn.clone());
                let mut it = base.clone();
                it.font_id = FontId::new(base.font_id.size, italic_family());
                layout_inline_styled(job, &text[i + 1..end], &it);
                job.append("*", 0.0, syn);
                i = end + 1;
                plain_start = i;
                continue;
            }
        }

        // underline __...__
        if b == b'_' && i + 1 < bytes.len() && bytes[i + 1] == b'_' {
            let prev_word = i > 0 && is_word(bytes[i - 1]);
            if !prev_word {
                if let Some(end) = find_close(text, i + 2, "__") {
                    flush_plain(job, &text[plain_start..i], base);
                    let stroke = Stroke::new(1.0, base.color);
                    // Hide the literal `_` glyphs but keep their layout width so the
                    // cursor stays aligned; the continuous underline stroke spans the
                    // whole region, so the markers read as the underline itself.
                    let mut marker = syntax_fmt(base.font_id.clone());
                    marker.color = Color32::TRANSPARENT;
                    marker.underline = stroke;
                    job.append("__", 0.0, marker.clone());
                    let mut und = base.clone();
                    und.underline = stroke;
                    layout_inline_styled(job, &text[i + 2..end], &und);
                    job.append("__", 0.0, marker);
                    i = end + 2;
                    plain_start = i;
                    continue;
                }
            }
        }

        // italic _..._ (only if surrounded by non-word chars, simple check)
        if b == b'_'
            && (i + 1 >= bytes.len() || bytes[i + 1] != b'_')
            && (i == 0 || bytes[i - 1] != b'_')
        {
            let prev_word = i > 0 && is_word(bytes[i - 1]);
            if !prev_word {
                if let Some(end) = find_close_single(text, i + 1, b'_') {
                    flush_plain(job, &text[plain_start..i], base);
                    let syn = syntax_fmt(base.font_id.clone());
                    job.append("_", 0.0, syn.clone());
                    let mut it = base.clone();
                    it.font_id = FontId::new(base.font_id.size, italic_family());
                    layout_inline_styled(job, &text[i + 1..end], &it);
                    job.append("_", 0.0, syn);
                    i = end + 1;
                    plain_start = i;
                    continue;
                }
            }
        }

        // strikethrough ~~...~~ or ~...~
        if b == b'~' {
            let n = if i + 1 < bytes.len() && bytes[i + 1] == b'~' { 2 } else { 1 };
            let marker = if n == 2 { "~~" } else { "~" };
            if let Some(end) = find_close(text, i + n, marker) {
                flush_plain(job, &text[plain_start..i], base);
                let syn = syntax_fmt(base.font_id.clone());
                job.append(marker, 0.0, syn.clone());
                let mut st = base.clone();
                st.strikethrough = Stroke::new(1.0, base.color);
                layout_inline_styled(job, &text[i + n..end], &st);
                job.append(marker, 0.0, syn);
                i = end + n;
                plain_start = i;
                continue;
            }
        }

        // inline code `...`
        if b == b'`' {
            if let Some(end) = find_close_single(text, i + 1, b'`') {
                flush_plain(job, &text[plain_start..i], base);
                let syn = syntax_fmt(base.font_id.clone());
                job.append("`", 0.0, syn.clone());
                let mut code = base.clone();
                code.font_id = FontId::new(base.font_id.size * 0.95, FontFamily::Monospace);
                code.color = pal().code;
                code.background = pal().code_bg;
                job.append(&text[i + 1..end], 0.0, code);
                job.append("`", 0.0, syn);
                i = end + 1;
                plain_start = i;
                continue;
            }
        }

        // link [text](url)
        if b == b'[' {
            if let Some(close_bracket) = find_close_single(text, i + 1, b']') {
                if close_bracket + 1 < bytes.len() && bytes[close_bracket + 1] == b'(' {
                    if let Some(close_paren) =
                        find_close_single(text, close_bracket + 2, b')')
                    {
                        flush_plain(job, &text[plain_start..i], base);
                        let syn = syntax_fmt(base.font_id.clone());
                        job.append("[", 0.0, syn.clone());
                        let mut link = base.clone();
                        link.color = pal().link;
                        link.underline = Stroke::new(1.0, pal().link);
                        job.append(&text[i + 1..close_bracket], 0.0, link);
                        job.append("](", 0.0, syn.clone());
                        let mut url = base.clone();
                        url.color = pal().syntax;
                        job.append(&text[close_bracket + 2..close_paren], 0.0, url);
                        job.append(")", 0.0, syn);
                        i = close_paren + 1;
                        plain_start = i;
                        continue;
                    }
                }
            }
        }

        i += 1;
    }
    flush_plain(job, &text[plain_start..], base);
}

// Reduced second pass for content inside already-styled regions (no further markdown nesting).
fn layout_inline_styled(job: &mut LayoutJob, text: &str, fmt: &TextFormat) {
    if !text.is_empty() {
        job.append(text, 0.0, fmt.clone());
    }
}

fn flush_plain(job: &mut LayoutJob, slice: &str, base: &TextFormat) {
    if !slice.is_empty() {
        job.append(slice, 0.0, base.clone());
    }
}

fn find_close(text: &str, from: usize, pat: &str) -> Option<usize> {
    text[from..].find(pat).map(|p| from + p)
}

fn find_close_single(text: &str, from: usize, ch: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == ch {
            return Some(i);
        }
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        i += 1;
    }
    None
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------- List continuation ----------

fn char_idx_to_byte(text: &str, char_idx: usize) -> usize {
    let mut chars = 0;
    for (b, _) in text.char_indices() {
        if chars == char_idx {
            return b;
        }
        chars += 1;
    }
    text.len()
}

/// Returns (marker_end_byte_index_in_line, indent_str, marker_str)
/// where marker_str includes the bullet and the trailing space, e.g. "* ", "- ", "1. ".
fn parse_list_marker(line: &str) -> Option<(usize, String, String)> {
    let trimmed = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
    let indent_len = line.len() - trimmed.len();
    let indent = line[..indent_len].to_string();

    for m in ["* ", "- ", "+ "] {
        if trimmed.starts_with(m) {
            return Some((indent_len + 2, indent, m.to_string()));
        }
    }
    let digits: usize = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        let marker = trimmed[..digits + 2].to_string();
        return Some((indent_len + digits + 2, indent, marker));
    }
    None
}

fn apply_list_continuation(
    ctx: &egui::Context,
    edit_id: egui::Id,
    text: &mut String,
    c_idx: usize,
    byte_idx: usize,
) {
    // byte_idx is right after the newline; cursor sits at start of new (empty) line.
    let prev_line_end = byte_idx - 1; // position of '\n'
    let prev_line_start = text[..prev_line_end]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prev_line = text[prev_line_start..prev_line_end].to_string();

    let Some((marker_end, indent, marker)) = parse_list_marker(&prev_line) else {
        return;
    };
    let content = &prev_line[marker_end..];

    if content.trim().is_empty() {
        // Empty list item — strip the marker and the just-inserted newline; exit list.
        let new_cursor_chars = text[..prev_line_start].chars().count();
        let mut new_text = String::with_capacity(text.len() - (byte_idx - prev_line_start));
        new_text.push_str(&text[..prev_line_start]);
        new_text.push_str(&text[byte_idx..]);
        *text = new_text;
        set_cursor(ctx, edit_id, new_cursor_chars);
    } else {
        // Insert "indent + marker" right after the newline, at byte_idx.
        let injection = format!("{}{}", indent, marker);
        text.insert_str(byte_idx, &injection);
        let new_cursor_chars = c_idx + injection.chars().count();
        set_cursor(ctx, edit_id, new_cursor_chars);
    }
}

fn set_cursor(ctx: &egui::Context, id: egui::Id, char_idx: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let cursor = egui::text::CCursor::new(char_idx);
        let range = egui::text::CCursorRange::one(cursor);
        state.cursor.set_char_range(Some(range));
        state.store(ctx, id);
    }
}

fn load_cursor(ctx: &egui::Context, id: egui::Id) -> Option<usize> {
    let state = egui::TextEdit::load_state(ctx, id)?;
    let range = state.cursor.char_range()?;
    Some(range.primary.index)
}

/// The selection as sorted char indices (equal if there is no selection).
fn load_selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
    let state = egui::TextEdit::load_state(ctx, id)?;
    let [a, b] = state.cursor.char_range()?.sorted();
    Some((a.index, b.index))
}

fn set_selection(ctx: &egui::Context, id: egui::Id, a: usize, b: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let range = egui::text::CCursorRange::two(
            egui::text::CCursor::new(a),
            egui::text::CCursor::new(b),
        );
        state.cursor.set_char_range(Some(range));
        state.store(ctx, id);
    }
}

/// Tolerates a stale or mid-character offset by snapping down to the nearest
/// char boundary.
fn byte_to_char_idx(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text[..byte].chars().count()
}

// ---------- Emphasis toggling (Cmd-B / Cmd-I / Cmd-U) ----------

fn has_marker_at(text: &str, at: usize, marker: &str) -> bool {
    text.as_bytes()
        .get(at..at + marker.len())
        .is_some_and(|s| s == marker.as_bytes())
}

/// Byte range of the alphanumeric word containing `byte`, empty if there is none.
fn word_at(text: &str, byte: usize) -> (usize, usize) {
    let mut start = byte;
    for (i, c) in text[..byte].char_indices().rev() {
        if !c.is_alphanumeric() {
            break;
        }
        start = i;
    }
    let mut end = byte;
    for (i, c) in text[byte..].char_indices() {
        if !c.is_alphanumeric() {
            break;
        }
        end = byte + i + c.len_utf8();
    }
    (start, end)
}

/// Wraps the selection in `marker`, or strips the markers if they are already
/// there. With no selection it acts on the word under the cursor, and failing
/// that inserts an empty pair with the cursor parked inside.
///
/// Takes and returns char indices (what egui's cursor speaks).
fn toggle_emphasis(text: &mut String, sel: (usize, usize), marker: &str) -> (usize, usize) {
    let m = marker.len(); // markers are ASCII, so bytes == chars
    let mut bs = char_idx_to_byte(text, sel.0);
    let mut be = char_idx_to_byte(text, sel.1);

    if bs == be {
        let (ws, we) = word_at(text, bs);
        bs = ws;
        be = we;
    }

    if bs == be {
        text.insert_str(bs, &marker.repeat(2));
        let c = byte_to_char_idx(text, bs + m);
        return (c, c);
    }

    // Markers sit inside the selection: **like this**
    if be - bs >= 2 * m && has_marker_at(text, bs, marker) && has_marker_at(text, be - m, marker)
    {
        text.replace_range(be - m..be, "");
        text.replace_range(bs..bs + m, "");
        return (byte_to_char_idx(text, bs), byte_to_char_idx(text, be - 2 * m));
    }

    // Markers sit just outside the selection: **like this**
    if bs >= m && has_marker_at(text, bs - m, marker) && has_marker_at(text, be, marker) {
        text.replace_range(be..be + m, "");
        text.replace_range(bs - m..bs, "");
        return (byte_to_char_idx(text, bs - m), byte_to_char_idx(text, be - m));
    }

    text.insert_str(be, marker);
    text.insert_str(bs, marker);
    (
        byte_to_char_idx(text, bs + m),
        byte_to_char_idx(text, be + m),
    )
}

// ---------- Layout cache ----------

struct CacheKey {
    hash: u64,
    theme: u8,
    wrap: u32,
    ppp: u32,
    find: u64,
}

thread_local! {
    static LAYOUT_CACHE: RefCell<Option<(CacheKey, std::sync::Arc<egui::Galley>)>> =
        const { RefCell::new(None) };
}

fn cached_galley(
    ui: &egui::Ui,
    text: &str,
    wrap: f32,
    hits: &[(usize, usize)],
    hit: Option<(usize, usize)>,
) -> std::sync::Arc<egui::Galley> {
    let theme = ACTIVE_THEME.load(Ordering::Relaxed);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    let mut find_hasher = std::collections::hash_map::DefaultHasher::new();
    hits.hash(&mut find_hasher);
    hit.hash(&mut find_hasher);
    let key = CacheKey {
        hash: hasher.finish(),
        find: find_hasher.finish(),
        theme,
        wrap: wrap.to_bits(),
        // pixels_per_point changes when the window moves to a monitor with a
        // different scale factor; a galley laid out at the old DPI references a
        // stale font atlas and renders as garbled overlapping glyphs.
        ppp: ui.ctx().pixels_per_point().to_bits(),
    };

    if let Some(g) = LAYOUT_CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(k, _)| {
                k.hash == key.hash
                    && k.theme == key.theme
                    && k.wrap == key.wrap
                    && k.ppp == key.ppp
                    && k.find == key.find
            })
            .map(|(_, g)| g.clone())
    }) {
        return g;
    }
    let mut job = layout_markdown(text, wrap);
    paint_hits(&mut job, hits, hit);
    let g = ui.fonts(|f| f.layout_job(job));
    LAYOUT_CACHE.with(|c| *c.borrow_mut() = Some((key, g.clone())));
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
# Heading **bold**
para with *italic*, __underline__, ~~struck~~, ~also~, `code` and [a link](http://x.y)

- list item
  - nested
1. ordered item

> quoted _text_

```rust
fn main() { println!(\"hi\"); }
```
trailing";

    /// `paint_hits` slices sections by byte offsets into the source, which is
    /// only valid because the layout emits every source byte exactly once.
    #[test]
    fn layout_reproduces_the_source_verbatim() {
        let job = layout_markdown(DOC, 400.0);
        assert_eq!(job.text, DOC);
        let mut next = 0;
        for s in &job.sections {
            assert_eq!(s.byte_range.start, next, "gap or overlap in sections");
            next = s.byte_range.end;
        }
        assert_eq!(next, DOC.len());
    }

    #[test]
    fn tilde_runs_strike_through_their_contents() {
        for (src, marker) in [("~~struck~~", "~~"), ("~also~", "~")] {
            let job = layout_markdown(src, 400.0);
            let inner = &src[marker.len()..src.len() - marker.len()];
            let s = job
                .sections
                .iter()
                .find(|s| &job.text[s.byte_range.clone()] == inner)
                .expect("styled run missing");
            assert!(s.format.strikethrough.width > 0.0, "no strike for {src}");
        }
        // Unmatched tilde stays literal.
        assert_eq!(layout_markdown("a ~ b", 400.0).text, "a ~ b");
    }

    #[test]
    fn hits_get_a_background_and_sections_stay_contiguous() {
        let hits = find_hits(DOC, "item");
        assert_eq!(hits.len(), 2);
        let mut job = layout_markdown(DOC, 400.0);
        paint_hits(&mut job, &hits, Some(hits[1]));

        let mut next = 0;
        for s in &job.sections {
            assert_eq!(s.byte_range.start, next);
            next = s.byte_range.end;
        }
        assert_eq!(next, DOC.len());

        let bg = |range: (usize, usize)| {
            job.sections
                .iter()
                .filter(|s| s.byte_range.start >= range.0 && s.byte_range.end <= range.1)
                .map(|s| s.format.background)
                .collect::<Vec<_>>()
        };
        assert_eq!(bg(hits[0]), vec![pal().find]);
        assert_eq!(bg(hits[1]), vec![pal().find_cur]);
    }

    /// Highlight ranges are computed from the text as it was *before* the
    /// editor applied this frame's keystroke, so they can point mid-character.
    /// The job must still be something epaint can lay out.
    #[test]
    fn stale_hits_survive_multi_byte_text() {
        let text = "héllo wörld, héllo";
        let stale = [(1, 3), (7, 9), (13, 18)]; // (1,3) and (7,9) start inside a char
        let mut job = layout_markdown(text, 400.0);
        paint_hits(&mut job, &stale, Some(stale[0]));
        for s in &job.sections {
            assert!(text.is_char_boundary(s.byte_range.start));
            assert!(text.is_char_boundary(s.byte_range.end));
        }

        let mut defs = egui::FontDefinitions::default();
        let fallback = defs.families[&FontFamily::Proportional].clone();
        defs.families.insert(bold_family(), fallback.clone());
        defs.families.insert(italic_family(), fallback);
        let fonts = egui::epaint::text::Fonts::new(2.0, 4096, defs);
        fonts.begin_pass(2.0, 4096);
        assert_eq!(fonts.layout_job(job).text(), text);
    }

    #[test]
    fn find_is_smart_case_and_non_overlapping() {
        assert_eq!(find_hits("aXa xax", "xa"), vec![(1, 3), (4, 6)]);
        assert_eq!(find_hits("aXa xax", "Xa"), vec![(1, 3)]);
        assert_eq!(find_hits("aaaa", "aa"), vec![(0, 2), (2, 4)]);
        assert!(find_hits("anything", "").is_empty());
        // Byte offsets stay valid around multi-byte characters.
        assert_eq!(find_hits("é far", "far"), vec![(3, 6)]);
    }

    fn toggled(text: &str, sel: (usize, usize), marker: &str) -> (String, (usize, usize)) {
        let mut s = text.to_owned();
        let range = toggle_emphasis(&mut s, sel, marker);
        (s, range)
    }

    #[test]
    fn emphasis_wraps_the_selection() {
        assert_eq!(
            toggled("one two three", (4, 7), "**"),
            ("one **two** three".to_owned(), (6, 9))
        );
    }

    #[test]
    fn emphasis_unwraps_markers_inside_or_outside_the_selection() {
        // selection covers the markers
        assert_eq!(
            toggled("one **two** three", (4, 11), "**"),
            ("one two three".to_owned(), (4, 7))
        );
        // selection covers only the content
        assert_eq!(
            toggled("one **two** three", (6, 9), "**"),
            ("one two three".to_owned(), (4, 7))
        );
        assert_eq!(
            toggled("a __b__ c", (4, 5), "__"),
            ("a b c".to_owned(), (2, 3))
        );
    }

    #[test]
    fn emphasis_falls_back_to_the_word_under_the_cursor() {
        assert_eq!(
            toggled("one two three", (5, 5), "*"),
            ("one *two* three".to_owned(), (5, 8))
        );
        // No word under the cursor: empty pair, cursor parked inside.
        assert_eq!(
            toggled("one  two", (4, 4), "**"),
            ("one **** two".to_owned(), (6, 6))
        );
    }

    #[test]
    fn emphasis_handles_multi_byte_text() {
        // "héllo wörld", selecting "wörld" (chars 6..11)
        let (text, sel) = toggled("héllo wörld", (6, 11), "**");
        assert_eq!(text, "héllo **wörld**");
        assert_eq!(sel, (8, 13));
        // and back
        assert_eq!(
            toggled(&text, sel, "**").0,
            "héllo wörld".to_owned()
        );
    }
}
