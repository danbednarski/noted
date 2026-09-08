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

/// `"3. "` becomes `"4. "`; bullets are returned unchanged.
fn next_marker(marker: &str) -> String {
    marker
        .strip_suffix(". ")
        .and_then(|n| n.parse::<u64>().ok())
        .map_or_else(|| marker.to_owned(), |n| format!("{}. ", n + 1))
}

/// Runs after the user typed a newline; the caret is at char `caret`, the
/// start of the new line. Carries the previous line's list marker over
/// (numbering counts up), or ends the list when Enter is hit on an empty item.
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
    let inject = format!("{indent}{}", next_marker(marker));
    text.insert_str(at, &inject);
    Some(caret + inject.chars().count())
}

// ---------- Indentation ----------

const INDENT: &str = "  ";

/// Tab / Shift+Tab over the selection `(a, b)`, given as char indices.
///
/// Every line the selection touches is indented (or outdented) by two spaces.
/// A bare caret on a non-list line inserts a literal tab instead, so Tab still
/// types a tab in prose. Returns the new selection, or `None` if nothing
/// changed.
pub fn shift_indent(
    text: &mut String,
    sel: (usize, usize),
    outdent: bool,
) -> Option<(usize, usize)> {
    let (mut bs, mut be) = (char_idx_to_byte(text, sel.0), char_idx_to_byte(text, sel.1));
    let first = line_start(text, bs);
    // A selection ending right after a newline does not include the next line.
    let last_end = if be > bs && text.as_bytes()[be - 1] == b'\n' {
        be - 1
    } else {
        be
    };
    let last = line_start(text, last_end);

    if bs == be && !outdent && parse_list_marker(line_from(text, first)).is_none() {
        text.insert(bs, '\t');
        return Some((sel.0 + 1, sel.0 + 1));
    }

    let starts: Vec<usize> = std::iter::once(first)
        .chain(
            text[first..last]
                .match_indices('\n')
                .map(|(i, _)| first + i + 1),
        )
        .collect();

    let mut changed = false;
    // Walk backwards so earlier offsets stay valid while we splice.
    for &start in starts.iter().rev() {
        if outdent {
            let rm = text[start..]
                .bytes()
                .take(INDENT.len())
                .take_while(|&b| b == b' ')
                .count();
            if rm == 0 {
                continue;
            }
            text.replace_range(start..start + rm, "");
            // A caret inside the removed run lands on the line start.
            let shrink = |p: usize| {
                if p >= start + rm {
                    p - rm
                } else {
                    p.min(start)
                }
            };
            bs = shrink(bs);
            be = shrink(be);
        } else {
            text.insert_str(start, INDENT);
            if start <= bs {
                bs += INDENT.len();
            }
            if start <= be {
                be += INDENT.len();
            }
        }
        changed = true;
    }
    changed.then(|| (byte_to_char_idx(text, bs), byte_to_char_idx(text, be)))
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

    // ---- lists ----

    #[test]
    fn enter_continues_bullets_and_counts_ordered_lists_up() {
        assert_eq!(
            apply("* a\n", |t| continue_list(t, 4)),
            ("* a\n* ".to_owned(), Some(6))
        );
        assert_eq!(
            apply("Three things:\n1. Fitness\n", |t| continue_list(t, 25)),
            ("Three things:\n1. Fitness\n2. ".to_owned(), Some(28))
        );
        assert_eq!(
            apply("  9. x\n", |t| continue_list(t, 7)),
            ("  9. x\n  10. ".to_owned(), Some(13))
        );
    }

    #[test]
    fn enter_on_an_empty_item_leaves_the_list() {
        assert_eq!(
            apply("1. a\n2. \n", |t| continue_list(t, 9)),
            ("1. a\n".to_owned(), Some(5))
        );
    }

    #[test]
    fn enter_elsewhere_is_left_alone() {
        assert_eq!(apply("plain\n", |t| continue_list(t, 6)).1, None);
        assert_eq!(apply("* a", |t| continue_list(t, 3)).1, None); // no newline typed
        assert_eq!(apply("héllo\n", |t| continue_list(t, 6)).1, None); // multi-byte
    }

    // ---- indentation ----

    #[test]
    fn tab_indents_every_selected_line() {
        let src = "* a\n* b\n* c\n* d";
        // Select from inside "b" to inside "c".
        assert_eq!(
            apply(src, |t| shift_indent(t, (6, 10), false)),
            ("* a\n  * b\n  * c\n* d".to_owned(), Some((8, 14)))
        );
        // Selection ending right after a newline leaves the next line alone.
        assert_eq!(
            apply(src, |t| shift_indent(t, (4, 8), false)).0,
            "* a\n  * b\n* c\n* d"
        );
        // Works on prose too when there is a selection.
        assert_eq!(
            apply("x\ny", |t| shift_indent(t, (0, 3), false)).0,
            "  x\n  y"
        );
    }

    #[test]
    fn shift_tab_outdents_every_selected_line_and_clamps_the_caret() {
        assert_eq!(
            apply("  * a\n * b\n* c", |t| shift_indent(t, (1, 13), true)),
            ("* a\n* b\n* c".to_owned(), Some((0, 10)))
        );
        // Nothing to remove: no change reported.
        assert_eq!(apply("* a", |t| shift_indent(t, (2, 2), true)).1, None);
    }

    #[test]
    fn bare_caret_indents_list_lines_but_types_a_tab_in_prose() {
        assert_eq!(
            apply("* a", |t| shift_indent(t, (3, 3), false)),
            ("  * a".to_owned(), Some((5, 5)))
        );
        assert_eq!(
            apply("ab", |t| shift_indent(t, (1, 1), false)),
            ("a\tb".to_owned(), Some((2, 2)))
        );
    }

    #[test]
    fn indent_keeps_char_indices_honest_around_multi_byte_text() {
        assert_eq!(
            apply("* é\n* ö", |t| shift_indent(t, (2, 7), false)),
            ("  * é\n  * ö".to_owned(), Some((4, 11)))
        );
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
