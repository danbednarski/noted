//! Themes, palette and fonts.
//!
//! The active theme is a process-wide atomic so the layouter (which has no
//! access to app state) can read the palette. Call [`apply_style`] after
//! changing it; nothing else re-reads fonts.

use egui::{Color32, FontFamily, FontId, Stroke, TextFormat};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThemeKind {
    CandyCane = 0,
    Win95 = 1,
    Dracula = 2,
}

impl ThemeKind {
    pub fn from_u8(n: u8) -> Self {
        match n {
            1 => ThemeKind::Win95,
            2 => ThemeKind::Dracula,
            _ => ThemeKind::CandyCane,
        }
    }
}

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);

pub fn theme_kind() -> ThemeKind {
    ThemeKind::from_u8(ACTIVE_THEME.load(Ordering::Relaxed))
}

pub fn set_theme_kind(t: ThemeKind) {
    ACTIVE_THEME.store(t as u8, Ordering::Relaxed);
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub struct Pal {
    pub face: Color32,  // window / form background
    pub field: Color32, // text editing surface
    pub text: Color32,
    pub text_bold: Color32,
    pub heading: Color32,
    pub syntax: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub quote: Color32,
    pub link: Color32,
    pub cursor: Color32,
    pub selection: Color32,
    pub find: Color32,     // background of every search match
    pub find_cur: Color32, // background of the active search match
}

pub fn pal() -> Pal {
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
        // Dracula: https://draculatheme.com/contribute (official spec)
        ThemeKind::Dracula => Pal {
            face: rgb(0x28, 0x2a, 0x36),
            field: rgb(0x28, 0x2a, 0x36),
            text: rgb(0xf8, 0xf8, 0xf2),
            text_bold: rgb(0xff, 0xff, 0xff),
            heading: rgb(0xbd, 0x93, 0xf9), // purple
            syntax: rgb(0x62, 0x72, 0xa4),  // comment
            code: rgb(0xf1, 0xfa, 0x8c),    // yellow
            code_bg: rgb(0x44, 0x47, 0x5a), // current line
            quote: rgb(0x62, 0x72, 0xa4),
            link: rgb(0x8b, 0xe9, 0xfd),   // cyan
            cursor: rgb(0xff, 0x79, 0xc6), // pink
            selection: rgb(0x44, 0x47, 0x5a),
            find: rgb(0x55, 0x5c, 0x42),     // dim yellow wash
            find_cur: rgb(0x8a, 0x92, 0x5e), // brighter yellow wash
        },
    }
}

pub const BODY_SIZE: f32 = 18.5;
pub const H1_SIZE: f32 = 33.0;
pub const H2_SIZE: f32 = 27.0;
pub const H3_SIZE: f32 = 23.0;
pub const H4_SIZE: f32 = 20.0;

pub fn body_font() -> FontId {
    FontId::new(BODY_SIZE, FontFamily::Proportional)
}

pub fn code_font() -> FontId {
    FontId::new(BODY_SIZE * 0.95, FontFamily::Monospace)
}

pub fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn italic_family() -> FontFamily {
    FontFamily::Name("italic".into())
}

pub fn body_fmt() -> TextFormat {
    TextFormat {
        font_id: body_font(),
        color: pal().text,
        ..Default::default()
    }
}

/// Dim markdown punctuation (`**`, `#`, `` ` ``) in the given font.
pub fn syntax_fmt(font: FontId) -> TextFormat {
    TextFormat {
        font_id: font,
        color: pal().syntax,
        ..Default::default()
    }
}

fn font_static(bytes: &'static [u8]) -> std::sync::Arc<egui::FontData> {
    std::sync::Arc::new(egui::FontData::from_static(bytes))
}

/// Installs fonts and visuals for the active theme.
pub fn apply_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "courier",
            &include_bytes!("../assets/CourierPrime-Regular.ttf")[..],
        ),
        (
            "serif",
            &include_bytes!("../assets/SourceSerif4-Regular.ttf")[..],
        ),
        (
            "serif_bold",
            &include_bytes!("../assets/SourceSerif4-Semibold.ttf")[..],
        ),
        (
            "serif_italic",
            &include_bytes!("../assets/SourceSerif4-It.ttf")[..],
        ),
        ("win95", &include_bytes!("../assets/R95-sans.ttf")[..]),
    ] {
        fonts.font_data.insert(name.to_owned(), font_static(bytes));
    }

    let (prop, bold, ital) = match theme_kind() {
        ThemeKind::CandyCane | ThemeKind::Dracula => ("serif", "serif_bold", "serif_italic"),
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
    fonts.families.insert(bold_family(), vec![bold.to_owned()]);
    fonts
        .families
        .insert(italic_family(), vec![ital.to_owned()]);
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
