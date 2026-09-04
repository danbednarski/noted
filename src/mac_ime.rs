//! macOS press-and-hold accent panel.
//!
//! AppKit only offers the panel to an `NSTextInputClient` that reports a real
//! caret location from `selectedRange` and honors the `replacementRange` it hands
//! back to `insertText:`. winit does neither -- both are upstream TODOs, blocked
//! on winit having no way to know the application's document -- so the panel
//! never appears in an egui window. We do know the document, so we patch those
//! selectors on winit's view class and answer them from Noted's buffer.
//!
//! There is a third problem once the panel does open. AppKit stops inserting the
//! character as soon as the panel takes the key over, the way a held key does not
//! repeat in a native text view. winit queues a key event for every repeat
//! regardless, so the character piles up underneath the open panel. `keyDown:` is
//! patched to notice a repeat that AppKit declined to insert and mark its key
//! event to be dropped.
//!
//! Everything here is a no-op unless the panel is involved: an `insertText:` with
//! no replacement range falls through to winit, and a repeat AppKit does insert
//! (a key with no accents, or press-and-hold turned off) is left alone, so
//! ordinary typing, key repeat and IME input are untouched.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::runtime::{AnyClass, Imp, Sel};
use objc2::sel;
use objc2_app_kit::NSEvent;
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

/// Characters winit queued that belong to the panel rather than to the document.
static SUPPRESS_TEXT: AtomicUsize = AtomicUsize::new(0);

/// Scratch flags covering the `keyDown:` currently being dispatched.
static APPKIT_INSERTED: AtomicBool = AtomicBool::new(false);
static PANEL_REPLACED: AtomicBool = AtomicBool::new(false);

type SelectedRangeFn = unsafe extern "C" fn(&NSObject, Sel) -> NSRange;
type InsertTextFn = unsafe extern "C" fn(&NSObject, Sel, &NSObject, NSRange);
type KeyDownFn = unsafe extern "C" fn(&NSObject, Sel, &NSEvent);

static ORIG_INSERT_TEXT: OnceLock<InsertTextFn> = OnceLock::new();
static ORIG_KEY_DOWN: OnceLock<KeyDownFn> = OnceLock::new();

unsafe extern "C" fn selected_range(_this: &NSObject, _sel: Sel) -> NSRange {
    let range = match STATE.lock().unwrap().selection {
        Some((location, length)) => NSRange::new(location as NSUInteger, length as NSUInteger),
        None => NSRange::new(NSNotFound as NSUInteger, 0),
    };
    trace(|| format!("selectedRange -> {}", show(range)));
    range
}

unsafe extern "C" fn insert_text(this: &NSObject, sel: Sel, string: &NSObject, range: NSRange) {
    let text = nsstring_of(string).to_string();
    trace(|| format!("insertText {text:?} range={}", show(range)));

    if range.location != NSNotFound as NSUInteger {
        // An accent picked from the panel, replacing the character it opened on.
        STATE.lock().unwrap().pending =
            Some((range.location as usize, range.length as usize, text));
        PANEL_REPLACED.store(true, Ordering::Relaxed);
        if let Some(ctx) = REPAINT.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
        return;
    }

    APPKIT_INSERTED.store(true, Ordering::Relaxed);
    if let Some(orig) = ORIG_INSERT_TEXT.get() {
        unsafe { orig(this, sel, string, range) };
    }
}

unsafe extern "C" fn key_down(this: &NSObject, sel: Sel, event: &NSEvent) {
    let repeat = unsafe { event.isARepeat() };
    let types_text = unsafe { event.characters() }.is_some_and(|characters| {
        characters
            .to_string()
            .chars()
            .next()
            .is_some_and(|c| !c.is_control())
    });
    let editing = STATE.lock().unwrap().selection.is_some();

    APPKIT_INSERTED.store(false, Ordering::Relaxed);
    PANEL_REPLACED.store(false, Ordering::Relaxed);

    // winit hands the key to AppKit and then queues its own event from in here,
    // so both answers are in by the time this returns.
    if let Some(orig) = ORIG_KEY_DOWN.get() {
        unsafe { orig(this, sel, event) };
    }

    // A repeat AppKit declined to insert is one the panel took over, and a key
    // that picked an accent has already been applied as a replacement. Either
    // way the character winit queued alongside it is not meant for the document.
    let swallowed_repeat = repeat && types_text && !APPKIT_INSERTED.load(Ordering::Relaxed);
    if editing && (swallowed_repeat || PANEL_REPLACED.load(Ordering::Relaxed)) {
        SUPPRESS_TEXT.fetch_add(1, Ordering::Relaxed);
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
    let (Some(range_method), Some(insert_method), Some(key_method)) = (
        class.instance_method(sel!(selectedRange)),
        class.instance_method(sel!(insertText:replacementRange:)),
        class.instance_method(sel!(keyDown:)),
    ) else {
        return;
    };

    let range_imp: SelectedRangeFn = selected_range;
    let insert_imp: InsertTextFn = insert_text;
    let key_imp: KeyDownFn = key_down;
    unsafe {
        range_method.set_implementation(std::mem::transmute::<SelectedRangeFn, Imp>(range_imp));

        let prev =
            insert_method.set_implementation(std::mem::transmute::<InsertTextFn, Imp>(insert_imp));
        let _ = ORIG_INSERT_TEXT.set(std::mem::transmute::<Imp, InsertTextFn>(prev));

        let prev = key_method.set_implementation(std::mem::transmute::<KeyDownFn, Imp>(key_imp));
        let _ = ORIG_KEY_DOWN.set(std::mem::transmute::<Imp, KeyDownFn>(prev));
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

/// How many queued characters belong to the panel rather than to the document.
pub fn text_events_to_drop() -> usize {
    SUPPRESS_TEXT.swap(0, Ordering::Relaxed)
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

/// `NOTED_IME_TRACE=1` logs every text-input call AppKit makes, which is the only
/// way to watch the panel from outside a debugger.
fn trace(message: impl FnOnce() -> String) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if !ENABLED.get_or_init(|| std::env::var_os("NOTED_IME_TRACE").is_some()) {
        return;
    }
    eprintln!("noted-ime: {}", message());
}

fn show(range: NSRange) -> String {
    if range.location == NSNotFound as NSUInteger {
        "{NOTFOUND}".to_owned()
    } else {
        format!("{{{},{}}}", range.location, range.length)
    }
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

    #[test]
    fn suppression_count_is_taken_once() {
        SUPPRESS_TEXT.store(2, Ordering::Relaxed);
        assert_eq!(text_events_to_drop(), 2);
        assert_eq!(text_events_to_drop(), 0);
    }
}
