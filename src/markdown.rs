//! Markdown layouting: turns the note into a styled `LayoutJob` for egui.
//!
//! Invariant: the job's text is the source **verbatim**, every byte appended
//! exactly once and in order. Markers like `**` are dimmed, never hidden, so
//! caret positions in the galley line up with the source. [`paint_hits`]
//! relies on this to slice search matches into sections by byte offset.

use crate::theme::{
    body_fmt, body_font, bold_family, code_font, italic_family, pal, syntax_fmt, theme_kind,
    ThemeKind, H1_SIZE, H2_SIZE, H3_SIZE, H4_SIZE,
};
use egui::{
    text::{LayoutJob, LayoutSection},
    Color32, FontFamily, FontId, Stroke, TextFormat,
};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SynFontStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

pub fn hash_of(x: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    x.hash(&mut h);
    h.finish()
}

// ---------- Block level ----------

pub fn layout_markdown(text: &str, wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.wrap.break_anywhere = false;

    let mut highlighter: Option<HighlightLines<'static>> = None;

    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            job.append("\n", 0.0, body_fmt());
        }

        // Fenced code block delimiter
        if line.trim_start().starts_with("```") {
            job.append(line, 0.0, syntax_fmt(code_font()));
            highlighter = match highlighter {
                Some(_) => None,
                None => {
                    let lang = line.trim_start().trim_start_matches('`').trim();
                    let ss = syntax_set();
                    let syntax = ss
                        .find_syntax_by_token(lang)
                        .or_else(|| ss.find_syntax_by_name(lang))
                        .or_else(|| ss.find_syntax_by_extension(lang))
                        .unwrap_or_else(|| ss.find_syntax_plain_text());
                    Some(HighlightLines::new(syntax, code_theme()))
                }
            };
            continue;
        }

        // Inside a fenced code block
        if let Some(hl) = highlighter.as_mut() {
            let line_nl = format!("{line}\n");
            match hl.highlight_line(&line_nl, syntax_set()) {
                Ok(ranges) => {
                    for (sty, s) in ranges {
                        let s = s.strip_suffix('\n').unwrap_or(s);
                        if !s.is_empty() {
                            job.append(s, 0.0, syntect_fmt(sty));
                        }
                    }
                }
                Err(_) => job.append(line, 0.0, plain_code_fmt()),
            }
            continue;
        }

        layout_line(&mut job, line);
    }
    job
}

fn layout_line(job: &mut LayoutJob, line: &str) {
    let rest = line.trim_start_matches(' ');
    let indent = &line[..line.len() - rest.len()];

    // headings
    for (n, size) in [(1, H1_SIZE), (2, H2_SIZE), (3, H3_SIZE), (4, H4_SIZE)] {
        let prefix = format!("{} ", "#".repeat(n));
        if let Some(body) = rest.strip_prefix(&prefix) {
            let base = TextFormat {
                font_id: FontId::new(size, bold_family()),
                color: pal().heading,
                ..Default::default()
            };
            append_indent(job, indent, body_fmt());
            job.append(
                &prefix,
                0.0,
                syntax_fmt(FontId::new(size, FontFamily::Proportional)),
            );
            layout_inline(job, body, &base);
            return;
        }
    }

    // blockquote
    if let Some(after) = rest.strip_prefix("> ") {
        append_indent(job, indent, body_fmt());
        job.append("> ", 0.0, syntax_fmt(body_font()));
        let mut base = body_fmt();
        base.color = pal().quote;
        base.italics = true;
        layout_inline(job, after, &base);
        return;
    }

    // list item: "- ", "* ", "+ " or "12. "
    if let Some(marker_len) = list_marker_len(rest) {
        append_indent(job, indent, indent_fmt());
        // marker in the accent colour ...
        job.append(&rest[..marker_len], 0.0, marker_fmt());
        // ... and its trailing space widened so content sits further right
        job.append(
            &rest[marker_len..marker_len + 1],
            0.0,
            TextFormat {
                extra_letter_spacing: 6.0,
                ..body_fmt()
            },
        );
        layout_inline(job, &rest[marker_len + 1..], &body_fmt());
        return;
    }

    // horizontal rule
    if matches!(rest, "---" | "***" | "___") {
        append_indent(job, indent, body_fmt());
        job.append(rest, 0.0, syntax_fmt(body_font()));
        return;
    }

    // paragraph
    append_indent(job, indent, body_fmt());
    layout_inline(job, rest, &body_fmt());
}

/// Length of the list marker without its trailing space (`"-"` is 1, `"12."`
/// is 3), if `rest` starts with one.
fn list_marker_len(rest: &str) -> Option<usize> {
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        return Some(1);
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0 && rest[digits..].starts_with(". ")).then_some(digits + 1)
}

fn append_indent(job: &mut LayoutJob, indent: &str, fmt: TextFormat) {
    if !indent.is_empty() {
        job.append(indent, 0.0, fmt);
    }
}

fn marker_fmt() -> TextFormat {
    TextFormat {
        color: pal().heading,
        ..body_fmt()
    }
}

/// Leading whitespace with extra letter spacing so nested-list indents look
/// like real indentation without changing the underlying character count.
fn indent_fmt() -> TextFormat {
    TextFormat {
        extra_letter_spacing: 8.0,
        ..body_fmt()
    }
}

// ---------- Inline level ----------

/// Scans `text` for inline markup and appends it to the job in `base` style.
/// Styled spans do not nest: `**a *b* c**` is bold with literal asterisks.
fn layout_inline(job: &mut LayoutJob, text: &str, base: &TextFormat) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut plain_start = 0;

    // Emits `open`, the styled inner text, then `close`, and advances past it.
    macro_rules! span {
        ($open:expr, $inner:expr, $close:expr, $fmt:expr, $next:expr) => {{
            flush_plain(job, &text[plain_start..i], base);
            let syn = syntax_fmt(base.font_id.clone());
            job.append($open, 0.0, syn.clone());
            job.append($inner, 0.0, $fmt);
            job.append($close, 0.0, syn);
            i = $next;
            plain_start = i;
            continue;
        }};
    }

    while i < bytes.len() {
        let b = bytes[i];
        let at = |k: usize| bytes.get(k).copied();
        let prev_word = i > 0 && is_word(bytes[i - 1]);

        // bold **...**
        if b == b'*' && at(i + 1) == Some(b'*') {
            if let Some(end) = find_close(text, i + 2, "**") {
                let mut bold = base.clone();
                bold.font_id = FontId::new(base.font_id.size, bold_family());
                bold.color = pal().text_bold;
                span!("**", &text[i + 2..end], "**", bold, end + 2);
            }
        }

        // italic *...* (not bold, not list marker)
        if b == b'*' && at(i + 1) != Some(b'*') && (i == 0 || bytes[i - 1] != b'*') {
            if let Some(end) = find_close_single(text, i + 1, b'*') {
                let mut it = base.clone();
                it.font_id = FontId::new(base.font_id.size, italic_family());
                span!("*", &text[i + 1..end], "*", it, end + 1);
            }
        }

        // underline __...__
        if b == b'_' && at(i + 1) == Some(b'_') && !prev_word {
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
                job.append(&text[i + 2..end], 0.0, und);
                job.append("__", 0.0, marker);
                i = end + 2;
                plain_start = i;
                continue;
            }
        }

        // italic _..._ (only when not glued to a word)
        if b == b'_' && at(i + 1) != Some(b'_') && (i == 0 || bytes[i - 1] != b'_') && !prev_word {
            if let Some(end) = find_close_single(text, i + 1, b'_') {
                let mut it = base.clone();
                it.font_id = FontId::new(base.font_id.size, italic_family());
                span!("_", &text[i + 1..end], "_", it, end + 1);
            }
        }

        // strikethrough ~~...~~ or ~...~
        if b == b'~' {
            let marker = if at(i + 1) == Some(b'~') { "~~" } else { "~" };
            let n = marker.len();
            if let Some(end) = find_close(text, i + n, marker) {
                let mut st = base.clone();
                st.strikethrough = Stroke::new(1.0, base.color);
                span!(marker, &text[i + n..end], marker, st, end + n);
            }
        }

        // highlight ==...== (opener must hug its text so `a == b` stays literal)
        if b == b'=' && at(i + 1) == Some(b'=') && !matches!(at(i + 2), Some(b' ' | b'=') | None) {
            if let Some(end) = find_close(text, i + 2, "==") {
                let mut hl = base.clone();
                hl.background = pal().highlight;
                span!("==", &text[i + 2..end], "==", hl, end + 2);
            }
        }

        // inline code `...`
        if b == b'`' {
            if let Some(end) = find_close_single(text, i + 1, b'`') {
                let mut code = base.clone();
                code.font_id = FontId::new(base.font_id.size * 0.95, FontFamily::Monospace);
                code.color = pal().code;
                code.background = pal().code_bg;
                span!("`", &text[i + 1..end], "`", code, end + 1);
            }
        }

        // link [text](url)
        if b == b'[' {
            if let Some(close_bracket) = find_close_single(text, i + 1, b']') {
                if at(close_bracket + 1) == Some(b'(') {
                    if let Some(close_paren) = find_close_single(text, close_bracket + 2, b')') {
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

fn flush_plain(job: &mut LayoutJob, slice: &str, base: &TextFormat) {
    if !slice.is_empty() {
        job.append(slice, 0.0, base.clone());
    }
}

fn find_close(text: &str, from: usize, pat: &str) -> Option<usize> {
    text[from..].find(pat).map(|p| from + p)
}

/// Like `find_close` for a single byte, but skips backslash-escaped bytes.
fn find_close_single(text: &str, from: usize, ch: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == ch {
            return Some(i);
        }
        i += if bytes[i] == b'\\' { 2 } else { 1 };
    }
    None
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------- Fenced code (syntect) ----------

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
        let name = if dark {
            "base16-mocha.dark"
        } else {
            "InspiredGitHub"
        };
        ts.themes
            .get(name)
            .cloned()
            .unwrap_or_else(|| ts.themes.values().next().unwrap().clone())
    })
}

fn plain_code_fmt() -> TextFormat {
    TextFormat {
        font_id: code_font(),
        color: pal().text,
        ..Default::default()
    }
}

fn syntect_fmt(sty: syntect::highlighting::Style) -> TextFormat {
    let c = sty.foreground;
    TextFormat {
        font_id: code_font(),
        color: Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a),
        italics: sty.font_style.contains(SynFontStyle::ITALIC),
        ..Default::default()
    }
}

// ---------- Search highlighting ----------

/// Paints search-match backgrounds over a finished job.
///
/// `layout_markdown` appends every byte of the source exactly once and in
/// order, so a job section's `byte_range` indexes the original text and match
/// ranges can simply be sliced into the section list.
pub fn paint_hits(job: &mut LayoutJob, hits: &[(usize, usize)], current: Option<(usize, usize)>) {
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

// ---------- Galley cache ----------

/// The last laid-out galley, keyed on everything that changes its shape. One
/// entry is enough: the app has one document and lays it out once per frame.
struct CacheKey {
    text: u64,
    find: u64,
    theme: ThemeKind,
    wrap: u32,
    ppp: u32,
}

thread_local! {
    static LAYOUT_CACHE: RefCell<Option<(CacheKey, std::sync::Arc<egui::Galley>)>> =
        const { RefCell::new(None) };
}

pub fn cached_galley(
    ui: &egui::Ui,
    text: &str,
    wrap: f32,
    hits: &[(usize, usize)],
    hit: Option<(usize, usize)>,
) -> std::sync::Arc<egui::Galley> {
    let key = CacheKey {
        text: hash_of(text),
        find: hash_of((hits, hit)),
        theme: theme_kind(),
        wrap: wrap.to_bits(),
        // pixels_per_point changes when the window moves to a monitor with a
        // different scale factor; a galley laid out at the old DPI references a
        // stale font atlas and renders as garbled overlapping glyphs.
        ppp: ui.ctx().pixels_per_point().to_bits(),
    };

    let cached = LAYOUT_CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(k, _)| {
                k.text == key.text
                    && k.find == key.find
                    && k.theme == key.theme
                    && k.wrap == key.wrap
                    && k.ppp == key.ppp
            })
            .map(|(_, g)| g.clone())
    });
    if let Some(g) = cached {
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
    use crate::find::find_hits;

    const DOC: &str = "\
# Heading **bold**
para with *italic*, __underline__, ~~struck~~, ~also~, ==marked==, `code` and [a link](http://x.y)

- list item
  - nested
1. ordered item

> quoted _text_

```rust
fn main() { println!(\"hi\"); }
```
trailing";

    fn assert_contiguous(job: &LayoutJob, src: &str) {
        let mut next = 0;
        for s in &job.sections {
            assert_eq!(s.byte_range.start, next, "gap or overlap in sections");
            next = s.byte_range.end;
        }
        assert_eq!(next, src.len());
    }

    /// `paint_hits` slices sections by byte offsets into the source, which is
    /// only valid because the layout emits every source byte exactly once.
    #[test]
    fn layout_reproduces_the_source_verbatim() {
        let job = layout_markdown(DOC, 400.0);
        assert_eq!(job.text, DOC);
        assert_contiguous(&job, DOC);
    }

    #[test]
    fn list_markers_get_the_accent_colour() {
        for src in ["- a", "* a", "+ a", "12. a", "  3. a"] {
            let job = layout_markdown(src, 400.0);
            let marker = src.trim_start().split(' ').next().unwrap();
            let s = job
                .sections
                .iter()
                .find(|s| &job.text[s.byte_range.clone()] == marker)
                .unwrap_or_else(|| panic!("marker section missing in {src:?}"));
            assert_eq!(s.format.color, pal().heading);
        }
        // Not a list: "1.5 things", "-not"
        for src in ["1.5 things", "-not"] {
            let job = layout_markdown(src, 400.0);
            assert!(job.sections.iter().all(|s| s.format.color != pal().heading));
        }
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
    fn equals_runs_highlight_their_contents() {
        let job = layout_markdown("a ==marked== b", 400.0);
        let s = job
            .sections
            .iter()
            .find(|s| &job.text[s.byte_range.clone()] == "marked")
            .expect("highlighted run missing");
        assert_eq!(s.format.background, pal().highlight);
        // Comparisons and rules stay literal.
        for src in ["a == b and c == d", "===", "x =="] {
            let job = layout_markdown(src, 400.0);
            assert!(
                job.sections
                    .iter()
                    .all(|s| s.format.background != pal().highlight),
                "{src}"
            );
        }
    }

    #[test]
    fn hits_get_a_background_and_sections_stay_contiguous() {
        let hits = find_hits(DOC, "item");
        assert_eq!(hits.len(), 2);
        let mut job = layout_markdown(DOC, 400.0);
        paint_hits(&mut job, &hits, Some(hits[1]));
        assert_contiguous(&job, DOC);

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
}
