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
- [ ] T4 (S1): Clipboard via `arboard`: Ctrl+C copy (selection; with empty selection copies the current line — VS Code standard), Ctrl+X cut (selection else line), Ctrl+V paste (replaces active selection if any; inserts as ONE undo step; multi-line supported). Windows-classic fallbacks: Shift+Insert paste, Shift+Delete cut, Ctrl+Insert copy. **Verify:** tests + USER. **Commit:** —
- [ ] T5 (S4): Ctrl+O interactive list of the active document's directory (cwd when untitled): type-to-filter on file name, Up/Down to move, Enter opens, Esc cancels; list renders above the status bar; directories shown with trailing `/` (not openable). **Verify:** tests + USER. **Commit:** —
- [ ] T6 (S3,S5): Docs: README keymap additions (selection/clipboard/word movement/F3+Shift+F3/open list), avances row with evidence. **Commit:** —

## Log

- L1: "La barra de busqueda ahora es legible, la proxima busqueda se confirma con enter, pero debería aceptar ademas F3, Ademas la barra deberia ser mas pequeña como en vscode. Anotar para luego. Reemplazar, no entiendo como hacer para colocar el texto por el que se debe reemplazar. En la barra inferior no queda nada, solo debia quitar deshacer y Busqueda Global." (prev round)
- L2: "Al elegir abrir un archivo desde dentro del editor que sea mas amigable en interactivo que solo preguntar el archivo. Asi como Shif+Enter retrocede lo debe hacer Shift+F3. Ahora se puede reemplazar, anotar para hacerlo mas similar a vscode. La barra inferior ahora no se borra. Dependiendo del nombre del archivo, el final del nombre queda pegado a las coordenadas del cursor. Como en VScode el tipo de archivo reconocido debe estar mas a la derecha. Agregar poder seleccionar texto para luego copiar con ctrl+C y pegar con ctrl+v. mantener compatibilidad con Shift+Insert y Shift+Supr como era antes. Poder moverse por el texto con Ctrl+Flecha derecha e izquierda que avance de a palabra y si se agrega Shit que lo haga seleccionando texto. Presente los desplazamientos standard de VScode, eso está documentado."
- L3: Owner chose "Lista del directorio actual" for S4 scope.
- L4: S5 delivered (Shift+F3) + status spacing/language position/left-segment priority in b9a004d; S6 backlog note 128b3a8; this doc 64fdf7d.
- L6: T1-T3 GREEN in ea3ae99 (193 tests). Gotchas discovered: ratatui TestBackend render tests must prime the viewport with a first draw (else stale viewport_h=1 pins scroll_top to the cursor line); the search dialog overlays line 0 without pushing lines down; typed() leaves the cursor at doc end.
- L5: T1-T3 first worker pass left 7 tests red (render spans discarded; word-right consumed next line's word; Shift+Home head not updated). Continuation launched with root-cause analysis.
