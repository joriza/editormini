# Métricas y estimación de esfuerzo — editormini

*Referencia generada el 2026-10-12, tras cerrar la ronda de selección/portapapeles/line-ops (HEAD `ab6b8af`, 211 tests).*

## 1. Líneas de código actuales

**7.748 líneas** en total (src + build.rs):

| Archivo | Líneas |
|---|---:|
| `src/app.rs` | 5.989 (≈2.830 de implementación + ≈3.160 de tests) |
| `src/document/mod.rs` | 673 |
| `src/i18n.rs` | 382 |
| `src/highlight.rs` | 263 |
| `src/main.rs` | 115 |
| `src/config.rs` | 114 |
| `src/theme.rs` | 100 |
| `src/pane.rs` | 79 |
| `build.rs` | 33 |
| **Total** | **7.748** |

En grueso: **~4.600 líneas de implementación + ~3.150 de tests** (211 tests, todos verdes).

## 2. Lenguaje: por qué Rust (aclaración de premisa)

Este proyecto **siempre fue Rust** — la pila se fijó antes de escribir la primera línea
(Rust + crossterm + ratatui + syntect, decisión del owner registrada en las rondas de
aclaración). Lo que está escrito en Go es **otra cosa**: la herramienta de revisión que
asiste al agente (Gentle AI, el binario `gentle-ai`), que es andamiaje, no producto.

¿Se sostiene la elección? Sí:

- **Un solo exe portable** (CRT estático, sin dependencias) — Rust lo resuelve de fábrica.
- **Latencia de UI sin pausas de GC** — para un editor que repinta constantemente, el
  modelo de ownership evita los micro-stutters que un GC introduciría.
- **Ecosistema exacto**: syntect (resaltado), arboard (portapapeles), crossterm/ratatui.
- Donde Go gana (compilación ultrarrápida, concurrencia simple) no pesa en un editor
  monousuario de un solo proceso.

## 3. Estimación para un desarrollador semi-senior sin experiencia en Rust

Dominante: la curva de Rust (borrow checker, lifetimes, el modelo de ratatui) — los
primeros 30-40 h son mayormente pelea con el compilador.

| Bloque | Horas aprox. |
|---|---:|
| Evaluación de stack + setup | 4–8 |
| Event loop, raw mode, esqueleto del editor | 8–16 |
| Modelo de documento (buffer por líneas, insert/delete/inverso) | 12–20 |
| Pestañas + estado por pestaña | 8–12 |
| Undo/redo acotado con tandas | 8–12 |
| Guardar/abrir/codificación/readonly | 6–10 |
| Resaltado (syntect) + temas + i18n + config | 10–16 |
| Barra de estado, gutér de números, mouse | 8–12 |
| Búsqueda/reemplazo (documento + cruzado, diálogos) | 16–24 |
| Selección + portapapeles + palabras + ir-a-línea + line-ops | 16–24 |
| Lista interactiva de apertura | 4–8 |
| Suite de 211 tests con disciplina test-first | 30–50 |
| Release portable (CRT estático) + docs | 4–8 |
| Pulido por rondas de pruebas manuales | 10–20 |
| **Total** | **≈ 145–240 h** |

Lectura rápida:

- Punto medio: **~180-200 h** (≈5 semanas a tiempo completo).
- Sin la disciplina de tests: ~130-170 h, con riesgo de regresiones alto.
- Si ya dominara Rust: ~80-120 h.
- Con asistencia de IA (como se construyó este proyecto): el muro de tiempo colapsa a días.
