//! Hand-drawn Windows 95 window chrome for the Win95 theme: raised frame, navy
//! caption bar with working buttons, and a sunken text box.

use crate::theme::rgb;
use egui::{Color32, FontFamily, Stroke};
use std::sync::atomic::{AtomicBool, Ordering};

const FACE: Color32 = rgb(0xc0, 0xc0, 0xc0);
const WHITE: Color32 = rgb(0xff, 0xff, 0xff);
const LITE: Color32 = rgb(0xdf, 0xdf, 0xdf);
const GRAY: Color32 = rgb(0x80, 0x80, 0x80);
const BLACK: Color32 = rgb(0x00, 0x00, 0x00);
const NAVY: Color32 = rgb(0x00, 0x00, 0x80);

static FULLSCREEN: AtomicBool = AtomicBool::new(false);

/// One 1px bevel ring: `tl` on top+left, `br` on bottom+right.
fn bevel(p: &egui::Painter, r: egui::Rect, tl: Color32, br: Color32) {
    p.hline(r.left()..=r.right(), r.top(), Stroke::new(1.0, tl));
    p.vline(r.left(), r.top()..=r.bottom(), Stroke::new(1.0, tl));
    p.hline(r.left()..=r.right(), r.bottom() - 1.0, Stroke::new(1.0, br));
    p.vline(r.right() - 1.0, r.top()..=r.bottom(), Stroke::new(1.0, br));
}

/// Classic raised 3D control edge (buttons, window frame).
pub fn raised(p: &egui::Painter, r: egui::Rect) {
    bevel(p, r, WHITE, BLACK);
    bevel(p, r.shrink(1.0), LITE, GRAY);
}

/// Classic sunken client edge (text fields).
fn sunken(p: &egui::Painter, r: egui::Rect) {
    bevel(p, r, GRAY, WHITE);
    bevel(p, r.shrink(1.0), BLACK, LITE);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Button {
    Close,
    Max,
    Min,
}

/// Caption-button symbols, drawn as primitives (no font glyphs).
fn draw_button_glyph(p: &egui::Painter, c: egui::Pos2, b: Button) {
    let ink = Stroke::new(1.0, BLACK);
    match b {
        Button::Min => {
            // short bar resting near the bottom
            let bar = egui::Rect::from_min_max(
                egui::pos2(c.x - 3.5, c.y + 2.5),
                egui::pos2(c.x + 3.5, c.y + 4.5),
            );
            p.rect_filled(bar, 0.0, BLACK);
        }
        Button::Max => {
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
            p.rect_filled(cap, 0.0, BLACK);
        }
        Button::Close => {
            // an X (drawn twice, 1px apart, for that chunky pixel weight)
            for off in [0.0_f32, 1.0] {
                let (l, r) = (c.x - 4.0 + off, c.x + 4.0 + off);
                let (t, b) = (c.y - 4.0, c.y + 4.0);
                p.line_segment([egui::pos2(l, t), egui::pos2(r, b)], ink);
                p.line_segment([egui::pos2(r, t), egui::pos2(l, b)], ink);
            }
        }
    }
}

/// Draws the full Win95/VB6 window: raised frame, navy caption bar with
/// 3D buttons, gray form, sunken white text box. Returns the interior rect
/// where the editor should be placed.
pub fn draw_chrome(ui: &egui::Ui, full: egui::Rect) -> egui::Rect {
    let p = ui.painter();
    p.rect_filled(full, 0.0, FACE);
    raised(p, full);

    let inner = full.shrink(3.0);

    // ---- title bar ----
    let tb = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), 20.0));
    p.rect_filled(tb, 0.0, NAVY);
    p.text(
        egui::pos2(tb.left() + 5.0, tb.center().y),
        egui::Align2::LEFT_CENTER,
        "noted",
        egui::FontId::new(13.0, FontFamily::Name("bold".into())),
        WHITE,
    );

    // ---- window buttons: close, maximize, minimize (right to left) ----
    let (bw, bh) = (18.0, 16.0);
    let by = tb.center().y - bh / 2.0;
    let mut bx = tb.right() - 4.0 - bw;
    let mut buttons_left = tb.right();
    for (i, act) in [Button::Close, Button::Max, Button::Min]
        .into_iter()
        .enumerate()
    {
        let r = egui::Rect::from_min_size(egui::pos2(bx, by), egui::vec2(bw, bh));
        let resp = ui.interact(r, egui::Id::new(("w95btn", act)), egui::Sense::click());
        let pressed = resp.is_pointer_button_down_on();

        p.rect_filled(r, 0.0, FACE);
        if pressed {
            sunken(p, r);
        } else {
            raised(p, r);
        }
        let nudge = if pressed {
            egui::vec2(1.0, 1.0)
        } else {
            egui::Vec2::ZERO
        };
        draw_button_glyph(p, r.center() + nudge, act);

        if resp.clicked() {
            let cmd = match act {
                Button::Close => egui::ViewportCommand::Close,
                Button::Min => egui::ViewportCommand::Minimized(true),
                Button::Max => {
                    let next = !FULLSCREEN.load(Ordering::Relaxed);
                    FULLSCREEN.store(next, Ordering::Relaxed);
                    egui::ViewportCommand::Fullscreen(next)
                }
            };
            ui.ctx().send_viewport_cmd(cmd);
        }

        buttons_left = buttons_left.min(r.left());
        bx -= bw + if i == 0 { 2.0 } else { 1.0 };
    }

    // ---- draggable caption (left of the buttons) ----
    let drag_rect = egui::Rect::from_min_max(tb.min, egui::pos2(buttons_left - 4.0, tb.bottom()));
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
    p.rect_filled(box_rect, 0.0, WHITE);
    sunken(p, box_rect);

    // interior available to the editor (inside the 2px sunken edge + padding)
    box_rect.shrink(2.0).shrink2(egui::vec2(6.0, 5.0))
}
