editormini


## Estado actual del proyecto

- **Versión completada — v0.1.0 (Hito 1 + extensiones)**: editor base con pestañas, resaltado por extensión, i18n EN/ES, búsqueda/reemplazo (documento activo y cruzado), selección con portapapeles del sistema y movimientos VS Code, operaciones de línea, paleta de comandos y quick open, mouse, gutter de números de línea y exe portable. **248 tests verdes, build con 0 warnings.** Detalle: [avances.md](avances.md).
- **Versión en progreso — Hito 2 (completado)**: paleta de comandos (`Ctrl+Shift+P`) y apertura difusa de archivos (`Ctrl+P`). Ruta completa: [plan.md](plan.md).
- **Hito 3 (planificado, aún sin código)**: explorador lateral con árbol de archivos del proyecto.

## Especificación: [specifications.md](specifications.md)

Comportamiento completo, modelo de datos e i18n, agnóstico del lenguaje (referencia para una futura app hermana). Complementos: [hallazgos.md](hallazgos.md) (decisiones con dato y defectos) y [métricas y estimación](metricas-y-estimacion.md).

## Compilar

Requiere Rust estable (probado con 1.99, toolchain MSVC).

```bash
cargo build --release
# El ejecutable queda en target/release/editormini.exe
```

> El ejecutable de release es **totalmente estático** (CRT enlazado estático): no requiere la redistribución de Visual C++ (VCRUNTIME140.dll) ni ninguna dependencia runtime adicional más allá de Windows mismo.

## Usar

```bash
editormini                          # documento nuevo sin título
editormini nota.md main.rs          # varios archivos = varias pestañas
editormini --lang es codigo.rs      # fuerza la interfaz al español
editormini examples/                # abre todos los samples de sintaxis
```

> Consejo: ejecutarlo dentro de **Windows Terminal** o el terminal de VS Code.
> En Git Bash/mintty, el control de flujo del terminal se come `Ctrl+S` (por eso también existe `F2`).

## Teclas actuales

| Tecla | Acción |
|---|---|
| Flechas, Home, End, PgUp, PgDn | mover el cursor |
| Escribir, Enter, Backspace, Delete | editar texto (con selección activa, Backspace/Delete borran lo seleccionado) |
| `Insert` | alternar modo sobrescribir (indicador `OVR`) |
| `Ctrl+S` o `F2` | guardar (en pestaña sin título abre Guardar como) |
| `Ctrl+Q` o `F10` | salir (pregunta si hay cambios sin guardar: Y/N/C — acepta S en español) |
| `Ctrl+W` | cerrar pestaña (pregunta si hay cambios) |
| `Ctrl+PgDn` / `Ctrl+PgUp` | pestaña siguiente / anterior (cicla) |
| `Ctrl+1` | ir al último documento del panel 1 (`Ctrl+2` reservada para el panel 2) |
| `Ctrl+Z` / `Ctrl+Y` (o `Ctrl+Shift+Z`) | deshacer / rehacer (historial acotado por pestaña; cada tanda de tipeo es un paso) |
| `F3` / `Ctrl+Shift+S` | guardar todo (pestañas con ruta; omite sin título) — fuera del diálogo de búsqueda |
| `Alt+↑` / `Alt+↓` | mover la línea del cursor una posición arriba/abajo (con selección activa mueve todo el bloque seleccionado; la selección viaja con él) |
| `Ctrl+K` (o `Ctrl+Shift+K`) | eliminar la línea completa del cursor (con selección activa borra todas las líneas seleccionadas; la columna se conserva) |
| `F8` | eliminar la línea del cursor — respaldo de `Ctrl+Shift+K` cuando el terminal no entrega los modificadores (si al pulsarlo aparece una `K` en el texto, tu terminal/PTY está comiendo el Shift) |
| `Ctrl+G` | ir a línea: número de línea destino (`Enter` salta, `Esc` cancela; fuera de rango se ajusta a la última línea) |
| `Ctrl+O` | abrir archivo: navegador del directorio del documento activo — carpetas con `/` (Enter entra), `..` sube, tipeá para filtrar, `↑`/`↓`/`Home`/`End`/`PgUp`/`PgDn` te movés, `Enter` abre, `Esc` cancela (también acepta escribir una ruta directamente) |
| `Ctrl+Shift+P` o `F4` | paleta de comandos: búsqueda difusa sobre todos los comandos con sus atajos (`↑`/`↓` elige, `Enter` ejecuta, `Esc` cancela) |
| `Ctrl+P` | apertura difusa: pestañas abiertas primero, después los archivos del directorio del documento activo; con la lista vacía, `Enter` abre la ruta tipeada |
| `Ctrl+F` | buscar en el documento activo (diálogo con contador; `Enter`/`F3` siguiente, `Shift+Enter`/`Shift+F3` anterior, `Esc` cierra; opciones case/regex a futuro) |
| `Ctrl+H` | reemplazar en el documento activo (diálogo con pestaña entre Query/Replace —`Tab` cambia de campo—, `Ctrl+Enter` o `F5` reemplaza la coincidencia actual, `F6` reemplaza todo con confirmación Y/N) |
| `Ctrl+Shift+F` | buscar en todos los documentos abiertos (itera coincidencias en orden de pestañas, cambia de pestaña al cruzar límites, muestra título del doc actual; `Esc` restaura la pestaña de inicio si no se navegó) |
| `Ctrl+Shift+H` | reemplazar en todos los documentos abiertos (reemplaza actual y avanza cambiando pestañas; reemplazar todo con confirmación única muestra total de ocurrencias y número de archivos; docs solo-lectura se omiten; deshacer por pestaña) |
| `Esc` | cierra el diálogo activo (en búsqueda, restaura cursor/pestaña si no se navegó); en el editor, colapsa la selección |
| `Ctrl+C` / `Ctrl+Insert` | copiar (selección activa; sin selección copia la línea actual) |
| `Ctrl+X` / `Shift+Supr` | cortar (selección activa; sin selección corta la línea actual) |
| `Ctrl+V` / `Shift+Insert` | pegar (si hay selección activa la reemplaza; un `Ctrl+Z` deshace todo el reemplazo) |
| `Shift+Flechas` / `Shift+Home/End` / `Shift+PgUp/PgDn` | seleccionar texto (ancla fija, como VS Code) |
| `Ctrl+←` / `Ctrl+→` | mover el cursor de a palabras |
| `Ctrl+Shift+←` / `Ctrl+Shift+→` | seleccionar de a palabras |
| `Ctrl+Home` / `Ctrl+End` | inicio / fin del documento (con `Shift` selecciona) |
| Clic izquierdo en el editor | colocar el cursor (se adapta al desplazamiento) |
| Números de línea | margen de números de línea siempre visible, alineados a la derecha, con énfasis en la línea del cursor |
| Clic izquierdo en una pestaña | cambiar al documento de esa pestaña |
| Rueda del ratón (arriba/abajo) | desplazar la vista (mismo rango que PgUp/PgDn) |

> La barra de estado inferior muestra, a la derecha, el lenguaje del documento y la **versión de compilación** (p. ej. `v0.1.0·1012-1830`, sello UTC `MMDD-HHMM` que cambia en cada build): referencia rápida para saber qué ejecutable estás probando.

## Configuración

`editormini.json` junto al ejecutable (portable-first; en desarrollo queda en `target/debug/`). Si no existe, se usan los valores por defecto; si está mal formado, el editor avisa y corta:

```json
{
  "language": "en",
  "theme": "dark-plus",
  "export": {},
  "open_files": ["/path/to/last.txt", "/path/to/other.md"]
}
```

### Sesiones

Al cerrar el editor, las rutas de las pestañas abiertas (solo las que tienen archivo; las pestañas sin título se omiten) se guardan en `open_files`. Al siguiente arranque, **sin pasos archivos por línea de comandos**, el editor las reabre tal cual. Archivos que hayan sido eliminados entre tanto se saltean sin bloquear el arranque; si no queda ninguna pestaña válida, abre una sin título como siempre.

Los paths de la CLI siguen ganando: `editormini a.txt b.txt` abre esos archivos y ignora la sessión guardada.

## Lenguajes soportados

El resaltado por extensión está curado para (hito 1):

- **Rust** (`.rs`)
- **Go** (`.go`)
- **JavaScript/TypeScript** (`.js`, `.ts`, `.tsx`, `.jsx`)
- **Python** (`.py`)
- **C/C++** (`.c`, `.cpp`, `.h`, `.hpp`)
- **C#** (`.cs`)
- **JSON** (`.json`)
- **TOML** (`.toml`)
- **YAML** (`.yaml`, `.yml`)
- **Markdown** (`.md`)
- **HTML/CSS** (`.html`, `.css`)
- **XML** (`.xml`)
- **Shell** (`.sh`, `.bat`)

Fallbacks: `toml → yml`, `ts/tsx/jsx → js`. `.ps1` queda como texto plano hasta cargador de gramáticas externas.

## Estructura del proyecto

```
src/
  main.rs      CLI, carga de archivos, ciclo de vida del terminal
  app.rs       bucle de eventos, teclas, dibujo, pestañas, modales
  document/    modelo de documento (líneas, BOM, fin de línea, solo-lectura)
  highlight.rs resaltado de sintaxis por extensión (syntect → spans de ratatui)
  theme.rs     paleta de la UI (Dark+ de VS Code; Light+ preparado)
  pane.rs      árbol de paneles (preparado para pantalla dividida)
  i18n.rs      catálogo de mensajes EN/ES (exhaustivo por compilación)
  config.rs    configuración JSON junto al ejecutable
  build.rs     sello de compilación UTC (MMDD-HHMM) que muestra la barra de estado
docs/          plan del proyecto, especificaciones, avances, hallazgos y métricas
examples/      samples de código por lenguaje para probar resaltado de sintaxis
odd/           tareas del flujo de trabajo (ODD)
```

## Rumbo (resumen)

Pantalla dividida con sincronización para comparar archivos, paleta de comandos, explorador lateral, deshacer completo, LSP/linter, y exportación a PDF para Markdown y Typst. Detalle completo en [docs/plan.md](docs/plan.md).

## Licencia

MIT
