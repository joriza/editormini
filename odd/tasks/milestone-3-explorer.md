# Feature: Milestone 3 — Side file explorer (editormini)

**Goal:** A keyboard-navigable file tree panel on the left: browse the active document's directory, open files with Enter, expand/collapse directories with depth limit 3 (expandable on demand). Toggle `Ctrl+B` (fallback `F9`). Panel opens by default only when no files are loaded.

**Decisions (owner, 2026-10-16):** (a) panel starts OPEN when no files are loaded, CLOSED when at least one file is loaded; (b) 3 levels of recursion, expandable on demand; (c) tree root = the active document's directory, updatable (re-rooted when switching tabs to a pathed doc).

**Verification policy:** every task: `cargo build` + `cargo test`, 0 warnings; UI behavior driven through `App::on_key`/`TestBackend` render tests (existing precedent). Owner runs the explorer at the end of the milestone.

## Specs

- S1: "El panel inicia abierto si no hay archivos cargados, cerrado si hay al menos un archivo cargado" (owner, 2026-10-16).
- S2: "3 niveles de recursion. expandible bajo demanda" (owner, 2026-10-16).
- S3: "¿Mostrar solo el árbol del directorio del archivo activo, o del cwd? (Propuesta: del directorio del archivo activo, actualizable)" — owner accepted the proposal: tree root is the active document's directory, re-rooted when the active document changes to a pathed file.
- S4: "Hito 3 — Explorador lateral: árbol de archivos del proyecto navegable con teclado (y mouse), apertura por Enter." (docs/plan.md).

## Tasks

- [x] **T1 (S2): Tree model** — `src/explorer.rs`: `ExplorerState` (root, flat visible rows, selected), initial load of 3 levels (`INITIAL_DEPTH`), expand/collapse on demand, selection clamping. Pure model, no UI. Route: inline. Commit: `2758e54`.
- [x] **T2 (S1): Panel state + toggle (state + re-root)** — `App.explorer: Option<ExplorerState>`; initial open state = open iff no document has a path (S1); re-root on tab switch to a pathed doc wired in `switch_to` (S3). Route: inline. Commits: `2758e54`, `53e52f9`.
- [x] **T3 (S4): Keyboard navigation** — Ctrl+B/F9 toggle with VS Code focus semantics: closed → open+focused, focused → close, visible-unfocused → focus. While focused: plain arrows select, Right toggle-expand, Left collapse-or-up, Enter opens file (closes panel) or toggles dir, Esc closes. All other keys (typing, Ctrl+O, F10) fall through to the editor; unfocused panel never steals keys. Route: inline. Commit: `fc39b45`.
- [x] **T4: Panel render** — `draw()` splits the main area: bordered left panel (min 24 cols, max ~40%, full width on narrow terminals) + shrinking editor; viewport_h/w track the reduced editor area. Depth indent, `▼`/`▶` markers, `/` on dirs, prompt-color selection while focused (dim fg unfocused); panel scroll keeps the selection visible. Route: inline. Commit: `9505de6`.
- [x] **T5: Status bar + hints + docs** — título localizado del panel (Explorer/Explorador) con la raíz del árbol (refinamiento sobre el "status bar root" original: sin conflicto con mensajes de estado); StartupHints agrega Ctrl+B; README (estado, filas de keymap), specifications.md y avances.md actualizados. Route: inline. Commit: `9c38a3e`.
- [ ] **T6: Mouse (opcional, S4)** — click en el panel selecciona fila, doble click/Enter abre. Pendiente de decisión del dueño (solo teclado esta ronda vs. mouse ahora).
- [x] **T7: Navegación de carpetas + dot-dirs + título legible** — sentinel `..` como primera fila (Enter = ir al padre); `walk` filtra directorios que empiezan con `.` (`.git`, `.github`, etc.); título del panel usa `theme.fg` + BOLD en lugar de `line_number_fg` DIM para legibilidad en modo oscuro; rebuild pre-populates expanded set también en `set_root` (bug de re-root). Tests: 277 passing. Route: inline.

## Out of scope (this milestone)

File operations (delete/rename), git status decoration, search box in the panel, file type filtering, panel width resize (milestone 4 split-screen handles resizing), multiple roots/sidebars.

## Log

- L1: Owner decisions verbatim — "El panel inicia abierto si no hay archivos cargados, cerrado si hay al menos un archivo cargado"; "3 niveles de recursion. expandible bajo demanda"; accepted "del directorio del archivo activo, actualizable".
- L2: Branch: work continues on `feature/hito-2-command-palette` (the feature branch of the current delivery cycle); merge/push remains the owner's call.
- L3: T3 design decision: plain "explorer consumes arrows when open" broke 20 editor tests (the panel opens by default on untitled docs, so Down/Esc/Enter never reached the editor). Resolved with a focus flag: `App.explorer_focused` (default false). Focus semantics mirror VS Code sidebar. Evidence: full suite 258 → 268 tests, 0 failures.
- L4: Slice boundary recorded per delivery strategy: 2,750 authored changed lines in 20 commits since main exceed the ~400-line review slice. Chained delivery (split PRs) applies before the next commit; the owner decides merge/push.
- L5: T2 completed: `switch_to` re-roots the open explorer to the new doc's dir (test `explorer_reroots_when_switching_to_pathed_doc`, commit `53e52f9`). 269 tests, 0 failures, 0 build warnings.
- L6: T4 completed (commit `9505de6`): render + shrink + scroll; 4 TestBackend tests. Two pre-existing selection-bg tests now close the default-open panel first (they pin selection colors, not the panel). 273 tests, 0 failures. Narrow-terminal clamp fixed (max before min) — caught by `palette_popup_fits_a_narrow_terminal_without_panicking`.
- L7: T5 completed (commit `9c38a3e`): título localizado + root en el header del panel (desviación registrada del texto original de la tarea, que decía status bar: el título es visible siempre y no compite con mensajes de estado). 274 tests, 0 fallos, 0 warnings.
- L8: Estado del hito: T1–T5 completados; T6 (mouse) opcional — requiere decisión del dueño. Merge/push siguen siendo decisión del dueño (slice de 3.046 líneas acumuladas → entrega encadenada sugerida).
- L9: Owner feedback round 4 (2026-10-17). (1) Panel needs folder navigation: added `..` sentinel as first row; Enter on it calls `go_up()` which changes the tree root to the parent directory (no-op at filesystem root). (2) Title contrast fixed: switched from `theme.line_number_fg`+DIM to `theme.fg`+BOLD — readable on dark theme. (3) Hidden dot-directories (`mkdir .git/.github`) filtered out of the walk so the tree doesn't drown in noise; dot-files still show. (4) Fixed `set_root` bug: `rebuild(initial=true)` now pre-populates the expanded set, so tabs switches preserve auto-expanded dirs. 277 tests passing, 0 failures. T6 mouse deferred per owner.
