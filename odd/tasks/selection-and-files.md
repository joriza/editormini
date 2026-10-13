# Feature: Selection, clipboard, VS Code movements, interactive open (editormini)

**Goal:** Keyboard text selection with system-clipboard copy/paste/cut, VS Code
standard cursor movements (word-wise, document edges), and a friendlier
interactive Ctrl+O file list. All with tests, i18n EN/ES where user-visible.

## Specs

- S1: "Agregar poder seleccionar texto para luego copiar con ctrl+C y pegar con ctrl+v. mantener compatibilidad con Shift+Insert y Shift+Supr como era antes."
- S2: "Poder moverse por el texto con Ctrl+Flecha derecha e izquierda que avance de a palabra y si se agrega Shit que lo haga seleccionando texto."
- S3: "Presente los desplazamientos standard de VScode, eso está documentado." (VS Code key bindings doc is the reference for movements)
- S4: "Al elegir abrir un archivo desde dentro del editor que sea mas amigable en interactivo que solo preguntar el archivo." — Owner chose: interactive list of the current directory (type-to-filter, arrows + Enter).
- S5: "Así como Shif+Enter retrocede lo debe hacer Shift+F3." — DONE in b9a004d.
- S6: "Ahora se puede reemplazar, anotar para hacerlo mas similar a vscode." — Backlog note only (replace dialog VS Code-like evolution), no work this feature.

## Tasks

- [x] T1 (S1,S3): Selection state per tab (anchor + head). Shift+Left/Right/Up/Down/Home/End/PageUp/PageDown extend the selection; plain movement, typing, Esc collapse it. Selection is (start,end) normalized on demand. **Verify:** tests. **Commit:** ea3ae99
- [x] T2 (S1): Render selection with a distinct background (new theme fields `selection_bg` Dark+ 0x264F78 / Light+ 0 ADD8E6-ish stub), composable with syntax spans; search match bg wins over selection bg. **Verify:** tests + USER. **Commit:** ea3ae99
- [x] T3 (S2,S3): Ctrl+Left/Right move by word (VS Code boundaries: whitespace + punctuation split); Ctrl+Shift+Left/Right move-by-word selecting; Ctrl+Home/Ctrl+End jump to document start/end (with Shift they select). **Verify:** tests. **Commit:** ea3ae99
- [x] T4 (S1): Clipboard via `arboard`: Ctrl+C copy (selection; with empty selection copies the current line — VS Code standard), Ctrl+X cut (selection else line), Ctrl+V paste (replaces active selection if any; inserts as ONE undo step; multi-line supported). Windows-classic fallbacks: Shift+Insert paste, Shift+Delete cut, Ctrl+Insert copy. **Verify:** tests + USER. **Commit:** —
- [x] T5 (S4): Ctrl+O interactive list of the active document's directory (cwd when untitled): type-to-filter on file name, Up/Down to move, Enter opens, Esc cancels; list renders above the status bar; directories shown with trailing `/` (not openable). **Verify:** tests + USER. **Commit:** 3fc0ef7 (open list), T6 docs
- [x] T6 (S3,S5): Docs: README keymap additions (selection/clipboard/word movement/F3+Shift+F3/open list), avances row with evidence. **Commit:** —

## Log

- L1: "La barra de busqueda ahora es legible, la proxima busqueda se confirma con enter, pero debería aceptar ademas F3, Ademas la barra deberia ser mas pequeña como en vscode. Anotar para luego. Reemplazar, no entiendo como hacer para colocar el texto por el que se debe reemplazar. En la barra inferior no queda nada, solo debia quitar deshacer y Busqueda Global." (prev round)
- L2: "Al elegir abrir un archivo desde dentro del editor que sea mas amigable en interactivo que solo preguntar el archivo. Asi como Shif+Enter retrocede lo debe hacer Shift+F3. Ahora se puede reemplazar, anotar para hacerlo mas similar a vscode. La barra inferior ahora no se borra. Dependiendo del nombre del archivo, el final del nombre queda pegado a las coordenadas del cursor. Como en VScode el tipo de archivo reconocido debe estar mas a la derecha. Agregar poder seleccionar texto para luego copiar con ctrl+C y pegar con ctrl+v. mantener compatibilidad con Shift+Insert y Shift+Supr como era antes. Poder moverse por el texto con Ctrl+Flecha derecha e izquierda que avance de a palabra y si se agrega Shit que lo haga seleccionando texto. Presente los desplazamientos standard de VScode, eso está documentado."
- L3: Owner chose "Lista del directorio actual" for S4 scope.
- L4: S5 delivered (Shift+F3) + status spacing/language position/left-segment priority in b9a004d; S6 backlog note 128b3a8; this doc 64fdf7d.
- L6: T1-T3 GREEN in ea3ae99 (193 tests). Gotchas discovered: ratatui TestBackend render tests must prime the viewport with a first draw (else stale viewport_h=1 pins scroll_top to the cursor line); the search dialog overlays line 0 without pushing lines down; typed() leaves the cursor at doc end.
- L5: T1-T3 first worker pass left 7 tests red (render spans discarded; word-right consumed next line's word; Shift+Home head not updated). Continuation launched with root-cause analysis.
- L7: T4 landed 021505c (arboard 3.6.1, i18n ClipboardError, one consolidated clipboard test per the parallel-races rule); orchestrator fixed perform_cut branching on copied-text emptiness instead of real selection and wrote the missing tests after the worker stalled. T5 landed 3fc0ef7 (interactive open list + opened files now FOCUS their tab — pre-existing gap found by the new tests). T6 docs in this round's docs commit.
- L8: Owner manual-test round 2: Backspace/Delete now delete the active selection (c3f6fec); Ctrl+O popup redrawn with border, content-fit width (min 24) and i18n hint title via Clear+Block (this round's feat commit); confirmed-good items (Ctrl+arrows, Shift+PgUp/PgDn, Ctrl+W) untouched.
- L9: Owner round 3 requests landed: Ctrl+Shift+K delete-line + Ctrl+G go-to-line (PromptKind::GotoLine, invalid input keeps the prompt, range clamps to last line, column preserved). Pending owner decisions: Shift+Up/Down line-move vs selection conflict, rectangular selection, clipboard history design, bottom-bar help during Ctrl+O.
- L10: Owner round 3 decisions: Alt+Arrow moves lines (with selection moves the whole block) — landed this round; rectangular selection DEFERRED to the multi-cursor design (bundle them); clipboard history DESIGN ONLY (kill-ring: internal buffer of last n copies/cuts, Ctrl+Shift+V pastes previous and repeats backward; alternative popup reusing the open-list pattern; do NOT mirror history into the system clipboard).
- L11: Alt+Up/Down landed (move_lines/move_line_block): block delete+insert as one undo group; selection travels; boundaries no-op. Gotcha: after deleting lines s..e the following line sits at index s (insert position), and the up-branch must delete the trailing break (or absorb the leftover empty line when the block touches doc end).
- L12: Owner attributed the strange Ctrl+Shift+K redraw to Herdr (their terminal manager), not the app. Hardened anyway: Ctrl+Shift+K now deletes ALL lines spanned by an active selection (VS Code) with one undo step, and a delivery test pins uppercase Char('K') (Windows terminal delivery).
- L13: Owner retested in another terminal (still the same symptom) and Ctrl+G works while Ctrl+Shift+K doesn't → diagnosis: the SHIFT modifier is being stripped between the terminal/PTY and the app (the K arrives as a typed character). F8 added as the always-works fallback (precedent F2/F3/F5/F6); README documents the K-appears symptom.
