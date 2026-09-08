//! Pure text edits behind the editor's shortcuts. No egui in here, so every
//! behaviour is unit-testable.
//!
//! Positions crossing the boundary with egui are **char** indices; slicing
//! happens on **byte** offsets. Convert at the edge, never mix them.

// ---------- Index conversion ----------

pub fn char_idx_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(b, _)| b)
}

/// Tolerates a stale or mid-character offset by snapping down to the nearest
/// char boundary.
pub fn byte_to_char_idx(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text[..byte].chars().count()
}

/// Byte offset of the start of the line containing `byte`.
fn line_start(text: &str, byte: usize) -> usize {
    text[..byte].rfind('\n').map_or(0, |i| i + 1)
}

/// The line beginning at byte `start`, without its newline.
fn line_from(text: &str, start: usize) -> &str {
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    &text[start..end]
}

// ---------- Lists ----------

/// Splits a list line into `(indent, marker)` where the marker includes its
/// trailing space: `"  * foo"` gives `("  ", "* ")`, `"1. x"` gives `("", "1. ")`.
pub fn parse_list_marker(line: &str) -> Option<(&str, &str)> {
    let body = line.trim_start_matches([' ', '\t']);
    let indent = &line[..line.len() - body.len()];
    if body.starts_with("* ") || body.starts_with("- ") || body.starts_with("+ ") {
        return Some((indent, &body[..2]));
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && body[digits..].starts_with(". ") {
        return Some((indent, &body[..digits + 2]));
    }
    None
}

/// Runs after the user typed a newline; the caret is at char `caret`, the
/// start of the new line. Carries the previous line's list marker over, or
/// ends the list when Enter is hit on an empty item.
/// Returns the new caret if the text changed.
pub fn continue_list(text: &mut String, caret: usize) -> Option<usize> {
    let at = char_idx_to_byte(text, caret);
    if at == 0 || text.as_bytes()[at - 1] != b'\n' {
        return None;
    }
    let prev_start = line_start(text, at - 1);
    let prev = &text[prev_start..at - 1];
    let (indent, marker) = parse_list_marker(prev)?;

    if prev[indent.len() + marker.len()..].trim().is_empty() {
        // Empty item: drop it and the newline just typed, leaving the list.
        text.replace_range(prev_start..at, "");
        return Some(byte_to_char_idx(text, prev_start));
    }
    let inject = format!("{indent}{marker}");
    text.insert_str(at, &inject);
    Some(caret + inject.chars().count())
}

// ---------- Indentation ----------

const INDENT: &str = "  ";

/// Tab / Shift+Tab at the caret (char index `sel.0`).
///
/// List lines are indented (or outdented) by two spaces; a caret on a non-list
/// line inserts a literal tab. Returns the new caret, or `None` if nothing
/// changed.
pub fn shift_indent(
    text: &mut String,
    sel: (usize, usize),
    outdent: bool,
) -> Option<(usize, usize)> {
    let bs = char_idx_to_byte(text, sel.0);
    let start = line_start(text, bs);
    let on_list = parse_list_marker(line_from(text, start)).is_some();

    if outdent {
        if !on_list {
            return None;
        }
        let rm = text[start..]
            .bytes()
            .take(INDENT.len())
            .take_while(|&b| b == b' ')
            .count();
        if rm == 0 {
            return None;
        }
        text.replace_range(start..start + rm, "");
        let c = byte_to_char_idx(text, bs.saturating_sub(rm).max(start));
        Some((c, c))
    } else if on_list {
        text.insert_str(start, INDENT);
        Some((sel.0 + 2, sel.0 + 2))
    } else {
        text.insert(bs, '\t');
        Some((sel.0 + 1, sel.0 + 1))
    }
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
pub fn toggle_emphasis(text: &mut String, sel: (usize, usize), marker: &str) -> (usize, usize) {
    let m = marker.len(); // markers are ASCII, so bytes == chars
    let mut bs = char_idx_to_byte(text, sel.0);
    let mut be = char_idx_to_byte(text, sel.1);

    if bs == be {
        (bs, be) = word_at(text, bs);
    }

    if bs == be {
        text.insert_str(bs, &marker.repeat(2));
        let c = byte_to_char_idx(text, bs + m);
        return (c, c);
    }

    // Markers sit inside the selection: **like this**
    if be - bs >= 2 * m && has_marker_at(text, bs, marker) && has_marker_at(text, be - m, marker) {
        text.replace_range(be - m..be, "");
        text.replace_range(bs..bs + m, "");
        return (
            byte_to_char_idx(text, bs),
            byte_to_char_idx(text, be - 2 * m),
        );
    }

    // Markers sit just outside the selection: **like this**
    if bs >= m && has_marker_at(text, bs - m, marker) && has_marker_at(text, be, marker) {
        text.replace_range(be..be + m, "");
        text.replace_range(bs - m..bs, "");
        return (
            byte_to_char_idx(text, bs - m),
            byte_to_char_idx(text, be - m),
        );
    }

    text.insert_str(be, marker);
    text.insert_str(bs, marker);
    (
        byte_to_char_idx(text, bs + m),
        byte_to_char_idx(text, be + m),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Applies `f` to `text` and returns the new text with the result.
    fn apply<R>(text: &str, f: impl FnOnce(&mut String) -> R) -> (String, R) {
        let mut s = text.to_owned();
        let r = f(&mut s);
        (s, r)
    }

    // ---- emphasis ----

    fn toggled(text: &str, sel: (usize, usize), marker: &str) -> (String, (usize, usize)) {
        apply(text, |t| toggle_emphasis(t, sel, marker))
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
        assert_eq!(toggled(&text, sel, "**").0, "héllo wörld".to_owned());
    }
}
