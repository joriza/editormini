# Avances del proyecto

Registro cronológico de entregas con su verificación. El detalle técnico por tarea vive en `odd/tasks/milestone-1-editor-tabs.md`; este documento es la vista del dueño del proyecto.

## Estado actual

- **Hito 1**: 8 de 11 tareas completadas (1, 2, 4, 5, 7, 9, 10 y 11; la 3 —deshacer— se reordenó después de la 5 a pedido del dueño para tener algo probable antes).
- **Verificación**: 114/114 tests (106 originales + 8 nuevos de gutter), build con 0 warnings.
- **Ronda de hardening (task 9 mouse)**: refactor de `tab_layout` helper compartido entre render y hit-testing, test de `col_off` no nulo, y tests de rueda de scroll con distancia exacta.
- **Verificado por el dueño**: escritura correcta, guardado, prompt de salida, `●` de modificado en pestañas.

## Cronología

| Fecha | Commit | Entrega | Verificación |
|---|---|---|---|
| 2026-10-12 | `5cbfe8d` | Tarea 1 — Andamiaje: CLI (`--lang`, archivos), config portable `editormini.json`, catálogo i18n EN/ES | 5 tests, 0 warnings |
| 2026-10-12 | `df4a8d2` | Tareas 2+4 — Modelo de documento (BOM, fin de línea, solo-lectura >10 MB, `EditRecord`) + shell TUI: tab bar, viewport Dark+, Ctrl+S/Q, ciclo de pestañas | 25 tests + smoke ANSI; prueba del dueño |
| 2026-10-12 | `d2f16c9` | Documentación de evidencia y aclaración de config en desarrollo | — |
| 2026-10-12 | `19547c5` | **Ronda de bugs del dueño**: cursor que reescribía el texto (crítico), mensajes de estado efímeros, guardado sin destino visible, `F2` como guardado universal, modo sobrescribir con `Insert` | 37 tests; corregido y aprobado por el dueño |
| 2026-10-12 | `58c1fa6` | Tarea 5 — Cierre de pestaña con prompt (`Ctrl+W`), salto de panel (`Ctrl+1/2`), cursor por pestaña; detectado y corregido `Ctrl+Q → N` sin efecto | 44 tests; aprobado por el dueño |
| 2026-10-12 | — | Tarea 8 — Ciclo de archivos: PromptKind{OpenFile,SaveAs} prompt state machine, Ctrl+Shift+S/F3 save_all, Ctrl+O open prompt, Ctrl+S on modified untitled opens SaveAs, Document::save_as, i18n keys (OpenFile, SaveAs, SavedAll, NoModifiedFiles, FileOpened, SavedAs) | 78 tests; build 0 warnings |
| 2026-10-12 | — | Tarea 7 — i18n completo: auditoría de rutas de error (IoError/NotUtf8/OutOfBounds/ReadOnly), corrección de 3 `format!("{e}")` sin traducir en undo/redo/do_insert, corrección de `read-only` hardcodeado, traslado de `ALL_KEYS` al módulo de tests para eliminar warning, 3 tests con assertions rotas por migración i18n corregidas | 78 tests; build 0 warnings |
| 2026-10-12 | — | Tarea 9 — Mouse básico: clic para colocar cursor (con mapeo pantalla→buffer que respeta scroll_top y offset), clic en pestaña para cambiar documento, rueda de scroll (ScrollUp/ScrollDown con mismos límites que PgUp/PgDn), clic derecho ignorado; eventos de mouse enrutados por `App::on_mouse` para tests determinísticos | 91 tests; build 0 warnings |
| 2026-10-12 | — | Tarea 10 — Status bar: filename ●, Ln:col, modified ●, overwrite OVR, read-only [RO], language indicator (Rust/Markdown/Plain text), ellipsis truncation on narrow terminals, theme-styled; i18n key PlainText added; 13 new tests | 106 tests; build 0 warnings |
| 2026-10-12 | — | Gutter de números de línea: columna fija derecha-alineada (dígitos de la última línea visible + 1 espacio), estilo dim para números y énfasis en la línea del cursor, fila en blanco para líneas pasadas de EOF, helper `gutter_width` compartido entre render y mouse-mapping, mapeo de clic corregido restando gutter_width, cursor screen col = gutter_w + buf_col - col_off; 8 tests nuevos | 114 tests; build 0 warnings |
| 2026-10-12 | — | Tarea 11 — Release portable: crt-static via `.cargo/config.toml`. VCRUNTIME140.dll eliminada. Antes: 3 353 088 bytes + 1 dep VC redist. Después: 3 455 488 bytes (~+102 KB), 0 deps runtime (solo DLLs de sistema Windows). Build release + test release 0 warnings; 106/106 tests verdes. | — |
| 2026-10-12 | — | Búsqueda/reemplazo tarea 1/4 — Ctrl+F diálogo de búsqueda en documento activo: contador "actual/total", Enter/Shift+Enter navega con wrap moviendo cursor y scroll, Esc restaura posición pre-búsqueda si no se navegó, resaltado de coincidencias (actual vs resto) con estilos de tema nuevos, coincidencia simple de línea (multi-línea a futuro), `SearchOptions{case_sensitive, is_regex}` listo para toggles futuros, insensitive por defecto; 11 tests nuevos | 125 tests; build 0 warnings |

## Reordenamientos decididos

- **Tarea 3 (deshacer acotado) pasó después de la tarea 5**: el dueño pidió parar en cada entregable probable; tabs completas daban más valor visible que undo.
- **README en español y `docs/`** se adelantaron a la tarea 11 a pedido del dueño.

## Próximo

1. Tarea 3 — deshacer/rehacer acotado (Ctrl+Z/Y, tandas de tipeo, historia por pestaña).
2. Tarea 6 — resaltado por extensión con tema Dark+ (el más visible).

