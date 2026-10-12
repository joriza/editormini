# Feature: Milestone 1 — Editor base con tabs (editormini)

**Goal:** Open, edit, save multiple files in tabs with VS Code Dark+ style syntax highlighting by extension. Single portable Windows executable. Architecture prepped for: i18n (EN default/ES), light theme, pane splits with mouse-resizable divider + scroll-sync compare, external PDF export pipeline, Typst via external grammars.

**Decisions:** See Engram `editormini/milestone-1-scope`, `editormini/keymap-navigation`, `editormini/markdown-export-typst`, `editormini/stack-decisions`.
**Stack:** Rust + crossterm + ratatui + syntect (default-fancy) + serde + clap. Toolchain: rustc 1.99.0 MSVC.
**Verification policy:** Every task: automated checks (cargo build/test). User-verify at user-visible milestones (after task 4, 5, 11 minimum).

## Tasks

- [ ] **1. Scaffold** — git init; cargo project `editormini`; deps (ratatui, crossterm, syntect, serde, serde_json, clap); `src/config.rs` (editormini.json: language, theme, inert `[export]` section; look beside exe, else defaults); `src/i18n.rs` catalog skeleton EN/ES (all UI strings route through it from day 1); CLI `editormini [FILE]... --lang <en|es>`; .gitignore; build passes. **Verify:** `cargo build` + config round-trip test. **Commit:** (pending)
- [ ] **2. Document model** — `src/document/`: line-based buffer, UTF-8, preserve BOM, preserve line endings (normalize to dominant on mixed), new files CRLF, read-only guard >10 MB, cursor pos; test-first. **Verify:** `cargo test`. **Commit:** (pending)
- [ ] **3. Bounded undo** — per-buffer undo/redo stack, typing-burst grouping, insert/delete/replace ops; test-first. **Verify:** `cargo test`. **Commit:** (pending)
- [ ] **4. Pane tree + editor viewport** — `src/pane.rs` PaneId/tree (single pane now; split/divider/scroll-sync fields stubbed); viewport render, cursor, scroll, movement keys (arrows/Home/End/PgUp/PgDn), editing keys; keymap table + command registry base. **Verify:** `cargo test` + **USER RUNS IT** (`cargo run -- file.txt`). **Commit:** (pending)
- [ ] **5. Tabs** — tab bar UI, Ctrl+PgDn/PgUp wrap (cross-panel-ready), Ctrl+1/2 panel jump, modified dot, Ctrl+W close with unsaved prompt (i18n). **Verify:** tests + **USER RUNS IT**. **Commit:** (pending)
- [ ] **6. Language registry + theme** — syntect curated set (Rust, Go, JS/TS, Python, C/C++, JSON, TOML, YAML, MD, HTML/CSS, XML, Shell), extension match; external grammar loading path (`languages/`); Theme struct: syntax theme (Dark+ calco) + chrome theme (ratatui Styles), Light+ stub. Visible-lines-only highlighting. **Verify:** tests + **USER RUNS IT** on .rs/.md/.json. **Commit:** (pending)
- [ ] **7. i18n complete** — full EN/ES catalog coverage audit (every user-visible string); `--lang` + config both switch; missing-key fails visible in dev. **Verify:** run in both languages. **Commit:** (pending)
- [ ] **8. File cycle** — Ctrl+S save (line endings/BOM respected), Ctrl+O open, Ctrl+Q quit with unsaved prompt, multi-file CLI args open as tabs. **Verify:** **USER RUNS IT**. **Commit:** (pending)
- [ ] **9. Basic mouse** — click to place cursor, click tab to switch, wheel scroll. **Verify:** **USER RUNS IT**. **Commit:** (pending)
- [ ] **10. Status bar** — file name, line:col, modified flag, language indicator; theme-styled. **Verify:** run. **Commit:** (pending)
- [ ] **11. Portable release** — `cargo build --release`; single-exe sanity (no runtime deps beyond Windows); README (usage, config, keymap); final user acceptance run. **Verify:** exe runs standalone. **Commit:** (pending)

## Keymap (hito 1)
Ctrl+PgDn/PgUp next/prev doc (wrap, cross-panel-ready) · Ctrl+1/2 last doc of panel 1/2 · Ctrl+W close tab · Ctrl+S/O/Q save/open/quit · Ctrl+Z/Y undo/redo · F-key fallbacks if terminal swallows chords.

## Out of scope (hito 1)
Command palette, sidebar, split UI, LSP/linter, MD preview, PDF export, Typst, settings UI. Config file: `editormini.json` only.

## Evidence log
- 2026-10-12 Recon: micro=Go1.23/tcell/lua, edit=Rust own-TUI/gap-buffer/lsh (Engram 645).
- 2026-10-12 Scope locked (Engram 647, 648, 649).
