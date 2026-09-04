//! macOS press-and-hold accent panel.
//!
//! AppKit only offers the accent panel to an `NSTextInputClient` that reports a
//! real caret location from `selectedRange` and honors the `replacementRange` it
//! hands back to `insertText:`. winit does neither -- both are upstream TODOs,
//! blocked on winit having no way to know the application's document -- so the
//! panel never appears in an egui window. We do know the document, so we patch
//! those two selectors on winit's view class and answer them from Noted's buffer.
//!
//! While the panel is open AppKit marks the base character, so that we replace it
//! rather than append to it. winit drops the replacement range there too, which
//! egui renders as a second copy of the character; [`panel_open`] reports that
//! window so the caller can drop those marks.
//!
//! Everything here is a no-op unless the accent panel is involved: a call with no
//! replacement range falls through to winit's own implementation, so ordinary
//! typing and IME input are untouched.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::runtime::{AnyClass, Imp, Sel};
use objc2::{msg_send, sel};
use objc2_foundation::{
    NSAttributedString, NSCopying, NSNotFound, NSObject, NSRange, NSString, NSUInteger,
};

struct State {
    /// `(location, length)` of the editor's selection in UTF-16 code units, which
    /// is how AppKit counts text. `None` whenever the editor is not the focused
    /// field, so the panel never offers to replace text we would not be editing.
    selection: Option<(usize, usize)>,
    /// `(location, length, replacement)` handed to us by the accent panel.
    pending: Option<(usize, usize, String)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    selection: None,
    pending: None,
});

/// Kept so an accent picked with the mouse -- which never wakes the event loop on
/// its own -- still lands in the note right away.
static REPAINT: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Whether the accent panel is currently marking the character it would replace.
static PANEL_OPEN: AtomicBool = AtomicBool::new(false);

type SelectedRangeFn = unsafe extern "C" fn(&NSObject, Sel) -> NSRange;
type InsertTextFn = unsafe extern "C" fn(&NSObject, Sel, &NSObject, NSRange);
type SetMarkedTextFn = unsafe extern "C" fn(&NSObject, Sel, &NSObject, NSRange, NSRange);
type UnmarkTextFn = unsafe extern "C" fn(&NSObject, Sel);

static ORIG_INSERT_TEXT: OnceLock<InsertTextFn> = OnceLock::new();
static ORIG_SET_MARKED_TEXT: OnceLock<SetMarkedTextFn> = OnceLock::new();
static ORIG_UNMARK_TEXT: OnceLock<UnmarkTextFn> = OnceLock::new();

unsafe extern "C" fn selected_range(_this: &NSObject, _sel: Sel) -> NSRange {
    match STATE.lock().unwrap().selection {
        Some((location, length)) => NSRange::new(location as NSUInteger, length as NSUInteger),
        None => NSRange::new(NSNotFound as NSUInteger, 0),
    }
}

unsafe extern "C" fn insert_text(
    this: &NSObject,
    sel: Sel,
    string: &NSObject,
    range: NSRange,
) {
    if range.location != NSNotFound as NSUInteger {
        let text = nsstring_of(string).to_string();
        STATE.lock().unwrap().pending =
            Some((range.location as usize, range.length as usize, text));
        if let Some(ctx) = REPAINT.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
        // Clears winit's marked-text state. Without this its IME state machine
        // stays in preedit and it stops forwarding keystrokes altogether.
        unsafe { msg_send![this, unmarkText] }
    }
    // Not the accent panel: let winit handle it as before.
    if let Some(orig) = ORIG_INSERT_TEXT.get() {
        unsafe { orig(this, sel, string, range) };
    }
}

/// The panel marks the character it is offering to replace. Everything else that
/// marks text is a real input method, and is passed through untouched.
unsafe extern "C" fn set_marked_text(
    this: &NSObject,
    sel: Sel,
    string: &NSObject,
    selected_range: NSRange,
    replacement_range: NSRange,
) {
    if replacement_range.location != NSNotFound as NSUInteger {
        PANEL_OPEN.store(!nsstring_of(string).is_empty(), Ordering::Relaxed);
    }
    if let Some(orig) = ORIG_SET_MARKED_TEXT.get() {
        unsafe { orig(this, sel, string, selected_range, replacement_range) };
    }
}

/// Dismissing the panel (escape, clicking away) unmarks rather than inserting.
unsafe extern "C" fn unmark_text(this: &NSObject, sel: Sel) {
    PANEL_OPEN.store(false, Ordering::Relaxed);
    if let Some(orig) = ORIG_UNMARK_TEXT.get() {
        unsafe { orig(this, sel) };
    }
}

/// `insertText:` is documented to receive either an `NSString` or an
/// `NSAttributedString`.
fn nsstring_of(string: &NSObject) -> Retained<NSString> {
    if string.is_kind_of::<NSAttributedString>() {
        let ptr: *const NSObject = string;
        unsafe { &*ptr.cast::<NSAttributedString>() }.string()
    } else {
        let ptr: *const NSObject = string;
        unsafe { &*ptr.cast::<NSString>() }.copy()
    }
}

/// Patches winit's view class. Must run after the window exists, since that is
/// when the class is registered; safe to call every frame.
pub fn install(ctx: &egui::Context) {
    static DONE: OnceLock<()> = OnceLock::new();
    if DONE.get().is_some() {
        return;
    }
    let Some(class) = AnyClass::get("WinitView") else {
        return; // Window not built yet.
    };
    let (Some(range_method), Some(insert_method), Some(mark_method), Some(unmark_method)) = (
        class.instance_method(sel!(selectedRange)),
        class.instance_method(sel!(insertText:replacementRange:)),
        class.instance_method(sel!(setMarkedText:selectedRange:replacementRange:)),
        class.instance_method(sel!(unmarkText)),
    ) else {
        return;
    };

    let range_imp: SelectedRangeFn = selected_range;
    let insert_imp: InsertTextFn = insert_text;
    let mark_imp: SetMarkedTextFn = set_marked_text;
    let unmark_imp: UnmarkTextFn = unmark_text;
    unsafe {
        range_method.set_implementation(std::mem::transmute::<SelectedRangeFn, Imp>(range_imp));

        let prev = insert_method
            .set_implementation(std::mem::transmute::<InsertTextFn, Imp>(insert_imp));
        let _ = ORIG_INSERT_TEXT.set(std::mem::transmute::<Imp, InsertTextFn>(prev));

        let prev = mark_method
            .set_implementation(std::mem::transmute::<SetMarkedTextFn, Imp>(mark_imp));
        let _ = ORIG_SET_MARKED_TEXT.set(std::mem::transmute::<Imp, SetMarkedTextFn>(prev));

        let prev = unmark_method
            .set_implementation(std::mem::transmute::<UnmarkTextFn, Imp>(unmark_imp));
        let _ = ORIG_UNMARK_TEXT.set(std::mem::transmute::<Imp, UnmarkTextFn>(prev));
    }
    *REPAINT.lock().unwrap() = Some(ctx.clone());
    let _ = DONE.set(());
}

/// Tell AppKit where the caret is; `None` while the editor is unfocused. Without
/// a real location AppKit sees `NSNotFound` and skips the panel entirely.
pub fn publish_selection(selection: Option<(&str, usize, usize)>) {
    STATE.lock().unwrap().selection = selection.map(|(text, start_char, end_char)| {
        let start = utf16_offset(text, start_char);
        let end = utf16_offset(text, end_char);
        (start, end.saturating_sub(start))
    });
}

/// Applies an accent chosen from the panel. Returns the new caret position, in
/// chars, when the document changed.
pub fn take_replacement(text: &mut String) -> Option<usize> {
    let (loc, len, replacement) = STATE.lock().unwrap().pending.take()?;
    let start = char_offset(text, loc);
    let end = char_offset(text, loc + len);
    let start_byte = byte_offset(text, start);
    let end_byte = byte_offset(text, end);
    text.replace_range(start_byte..end_byte, &replacement);
    Some(start + replacement.chars().count())
}

/// True while the panel is marking the character it would replace.
pub fn panel_open() -> bool {
    PANEL_OPEN.load(Ordering::Relaxed)
}

fn utf16_offset(text: &str, char_idx: usize) -> usize {
    text.chars().take(char_idx).map(char::len_utf16).sum()
}

/// Inverse of [`utf16_offset`], snapping down if `units` lands mid-surrogate.
fn char_offset(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (idx, ch) in text.chars().enumerate() {
        // Covers the exact boundary and, for a surrogate pair, any offset inside it.
        if seen + ch.len_utf16() > units {
            return idx;
        }
        seen += ch.len_utf16();
    }
    text.chars().count()
}

fn byte_offset(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(byte, _)| byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_round_trip_across_surrogate_pairs() {
        // "a😀b": the emoji is one char, two UTF-16 units, four UTF-8 bytes.
        let s = "a\u{1F600}b";
        assert_eq!(utf16_offset(s, 0), 0);
        assert_eq!(utf16_offset(s, 1), 1);
        assert_eq!(utf16_offset(s, 2), 3);
        assert_eq!(utf16_offset(s, 3), 4);
        for chars in 0..=3 {
            assert_eq!(char_offset(s, utf16_offset(s, chars)), chars);
        }
        // Mid-surrogate offsets snap down to the emoji rather than past it.
        assert_eq!(char_offset(s, 2), 1);
        assert_eq!(byte_offset(s, 2), 5);
    }

    #[test]
    fn replacement_swaps_the_base_character() {
        // What the panel sends after typing "i" in "hi" and picking "í":
        // replace one UTF-16 unit at offset 1.
        STATE.lock().unwrap().pending = Some((1, 1, "í".to_owned()));
        let mut text = "hi".to_owned();
        assert_eq!(take_replacement(&mut text), Some(2));
        assert_eq!(text, "hí");

        // Past a surrogate pair the byte and UTF-16 offsets disagree.
        STATE.lock().unwrap().pending = Some((3, 1, "é".to_owned()));
        let mut text = "a\u{1F600}e".to_owned();
        assert_eq!(take_replacement(&mut text), Some(3));
        assert_eq!(text, "a\u{1F600}é");
    }
}
