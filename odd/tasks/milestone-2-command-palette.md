# Feature: Milestone 2 — Command palette + fuzzy quick open (editormini)

**Goal:** VS Code-style `Ctrl+Shift+P` command palette (fuzzy search over a command registry) and `Ctrl+P` quick-open fuzzy file picker. Heart of "usarse como VS Code". Registry-first architecture so the keymap can become remappable in milestone 4.

**Decisions:** F4 is the palette F-key fallback (terminal-proof precedent F2/F3/F5/F6/F8; F4 unused). `Ctrl+P` needs no F-fallback: opening is already covered by `Ctrl+O` (not a critical path). Palette/quick-open are App-level modals: they open only when no other modal (confirm/search/prompt) is active, and while open they consume all keys (plain chars filter, Esc closes, Enter confirms, ↑/↓ navigate). Fuzzy match is a dependency-free case-insensitive subsequence with scoring (contiguity, word-start bonus, gap penalty). Quick-open candidates = open tabs first, then files of the active document's directory (reuse `compute_open_candidates`). Keymap remapping stays OUT (milestone 4).

**Verification policy:** every task: `cargo build` + `cargo test`, 0 warnings; UI behavior driven through `App::on_key` tests (search-dialog precedent); owner runs the palette at the end of the milestone.

## Specs

- S1: "necesito realizar un editor que de a poco se parezca a visual studio en la forma de uso. Pero tiene que ser para terminal de texto (TUI)" (pdt-editormini.md).
- S2: "Menu default en ingles estilo VSCode, pero que se pueda cambiar a español, tambien como vscode." (pdt-editormini.md).
- S3: "Ctrl+Shift+P con búsqueda difusa sobre un registro de comandos; keymap remapeable; Ctrl+P para abrir archivos (fuzzy picker). Es el corazón de 'usarse como VS Code'." (docs/plan.md, Hito 2).
- S4: "toda tecla crítica debe tener alternativa que ningún terminal pueda absorber" (docs/specifications.md §7 fallback rule).

## Tasks

- [ ] **T1 (S3): Fuzzy matcher** — `src/fuzzy.rs`: `fuzzy_score(query, text) -> Option<i32>` (case-insensitive subsequence; +2/matched char, +5 contiguous with previous match, +6 word-start, −1/skipped gap char; empty query = Some(0)) and `rank` helper ordering indices by score desc (stable). Test-first. **Commit:** —
- [ ] **T2 (S3): Command registry** — `src/command.rs`: `CommandKind` (existing actions only: save, save-all, close tab, quit, undo, redo, find, replace, find-all-docs, replace-all-docs, goto-line, open, delete-line, move-line-up/down, next/prev tab, last-doc-panel-1, toggle-overwrite) + `Command { kind, label_key }` + `all_commands()` stable order; every label resolves non-empty in EN and ES (new i18n keys, compiler-exhaustive). Test-first. **Commit:** —
- [ ] **T3 (S1, S2, S3, S4): Palette UI** — `palette: Option<PaletteState>` modal; `Ctrl+Shift+P` opens (F4 fallback), only when no other modal; floating bordered popup (draw_open_list pattern: Clear, accent border, hint title, `>` marker highlight); type-to-filter fuzzy over localized labels, ↑/↓, Enter executes via `App::execute_command`, Esc closes, `No matches` when empty. Tests via `App::on_key`. **Commit:** —
- [ ] **T4 (S3): Quick open** — `Ctrl+P` same popup, kind=QuickOpen: candidates = open tabs (doc titles, `>` switch) then active-dir files not already open; fuzzy filter; Enter: tab → `switch_to`, file → existing open logic (focus/open as tab); typed free path still allowed (Ctrl+O semantics). Tests. **Commit:** —
- [ ] **T5: Docs round** — README keymap rows (palette/quick-open/F4), specifications §7 keymap + §12 palette [H2]→done, avances row with evidence, hallazgos closure note for the abandoned review lineages. **Commit:** —

## Out of scope (this milestone)

Keymap remapping via config (hito 4), MRU/recency ranking beyond score ties, command arguments/parameters, project-wide disk search (hito 3 explorer), light-theme syntax palette.

## Log

- L1: "ok. continuemos con el proyecto. que tengo pendiente de revision" — owner asked for the review queue; after the list, owner chose: "Arrancar Hito 2" and "Abandonarlos" (the 6 stuck review lineages). Native `inspect` showed the authority inventory EMPTY (0 lineages) — the pre-restart store is gone, so there is nothing left to dispose; decision recorded here and closed in hallazgos at T5.
- L2: Branch `feature/hito-2-command-palette` created before first write (work-unit commits land here; merge/push remains the owner's call).
