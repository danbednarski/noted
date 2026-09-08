# Noted

Single-document Markdown notes app for macOS. Rust + egui/eframe. One note,
autosaved to `~/Library/Application Support/noted/notes.md`. No network, no
database, no plugins.

## Commands

```bash
cargo test                      # unit tests, ~0.2s (no GUI needed)
cargo clippy --all-targets      # must be warning-free
cargo fmt                       # rustfmt defaults
cargo run                       # launches the app
cargo bundle --release          # Noted.app (needs cargo-bundle); DMG steps in README
```

Definition of done for any change: `cargo fmt && cargo clippy --all-targets && cargo test` clean.

## Map

| File | Owns |
|---|---|
| `src/main.rs` | `NotedApp` state, the per-frame `update` loop, editor shortcuts, autosave |
| `src/edit.rs` | **Pure** text edits: list continuation, Tab indent, Cmd-B/I/U. No egui. Tests live here. |
| `src/markdown.rs` | Source text -> styled `LayoutJob`, search highlight painting, galley cache |
| `src/find.rs` | Cmd-F: match index, find bar UI, its shortcuts |
| `src/cursor.rs` | Read/write the caret via egui `TextEdit` state |
| `src/theme.rs` | `ThemeKind`, palette, fonts, `apply_style` |
| `src/menu.rs` | Native menu bar (muda), theme picker |
| `src/win95.rs` | Hand-drawn Win95 window chrome |
| `src/storage.rs` | Note/config paths, atomic writes, `Config` |
| `src/mac_ime.rs` | macOS press-and-hold accent panel (Objective-C runtime patching). Rarely needs touching. |

Frame order in `update`: menu -> accent panel -> find keys/bar -> editor shortcuts -> `TextEdit` -> list continuation -> autosave.

## Invariants (break these and things get subtly wrong)

1. **Char vs byte indices.** egui's caret speaks *char* indices. String slicing needs *byte* offsets. Every function in `edit.rs` takes/returns chars and converts internally with `char_idx_to_byte` / `byte_to_char_idx`. Never pass a byte offset to `set_cursor`.
2. **Layout is verbatim.** `layout_markdown` must append every source byte exactly once, in order (markers are dimmed, never dropped). `paint_hits` and caret placement depend on it. `layout_reproduces_the_source_verbatim` enforces this; add new syntax to `DOC` in that test.
3. **Cursor writes land next frame.** `set_cursor`/`set_selection` store into egui memory; the `TextEdit` picks them up on its next `show`. Reading back in the same frame before that returns the old value.
4. **Shortcuts run before the widget.** Anything the `TextEdit` would otherwise consume (Tab, Cmd-B) is handled in `handle_editor_keys` *before* the panel is drawn, and the event is removed from `ctx.input_mut().events`.
5. **Theme is global.** `theme::pal()` reads a process-wide atomic because the layouter has no app state. After changing it, call `apply_style`.
6. **One galley cache entry.** `cached_galley` keys on text, hits, theme, wrap width and DPI. If you add something that changes layout, add it to `CacheKey`.

## How to

- **Add inline markdown syntax:** new branch in `markdown::layout_inline` (use the `span!` macro), then a section-level test like `tilde_runs_strike_through_their_contents`.
- **Add block syntax:** new branch in `markdown::layout_line` before the paragraph fallback.
- **Add an editor shortcut:** pure function in `edit.rs` with tests, then wire it in `NotedApp::handle_editor_keys`. Call `self.touch()` after mutating text.
- **Add a theme:** variant in `ThemeKind` (+ `from_u8`), a `Pal` in `pal()`, an entry in `AppMenu::install`.
- **Add a persisted setting:** field on `storage::Config`, a line in `load`/`save`.

## Conventions

- Conventional Commits (`fix:`, `feat:`, `chore:`), imperative, lowercase, no trailing period.
- Plain dashes in prose and comments; no em-dashes.
- Doc comments explain *why*; the code already says what.
- Prefer a pure function + unit test over reaching into egui state. If it needs `ctx`, keep it in `main.rs` or `cursor.rs`.
- No new dependencies without a reason that a few lines of code can't cover.
