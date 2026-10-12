# Avances del proyecto

Registro cronológico de entregas con su verificación. El detalle técnico por tarea vive en `odd/tasks/milestone-1-editor-tabs.md`; este documento es la vista del dueño del proyecto.

## Estado actual

- **Hito 1**: 4 de 11 tareas completadas (1, 2, 4 y 5; la 3 —deshacer— se reordenó después de la 5 a pedido del dueño para tener algo probable antes).
- **Verificación**: 78/78 tests, build con 0 warnings.
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

## Reordenamientos decididos

- **Tarea 3 (deshacer acotado) pasó después de la tarea 5**: el dueño pidió parar en cada entregable probable; tabs completas daban más valor visible que undo.
- **README en español y `docs/`** se adelantaron a la tarea 11 a pedido del dueño.

## Próximo

1. Tarea 3 — deshacer/rehacer acotado (Ctrl+Z/Y, tandas de tipeo, historia por pestaña).
2. Tarea 6 — resaltado por extensión con tema Dark+ (el más visible).
3. Tareas 7-11: auditoría i18n, ciclo de archivos, mouse, status bar, release portable.
