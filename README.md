# editormini

Editor de texto para terminal (TUI) con la **forma de uso de Visual Studio Code**: pestañas, resaltado de sintaxis por extensión con la paleta clásica **Dark+**, interfaz en inglés o español, y un solo ejecutable portable.

Estado: **Hito 1 en curso** — editor base con pestañas, ya usable (ver [docs/avances.md](docs/avances.md)).

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
```

> Consejo: ejecutarlo dentro de **Windows Terminal** o el terminal de VS Code.
> En Git Bash/mintty, el control de flujo del terminal se come `Ctrl+S` (por eso también existe `F2`).

## Teclas actuales

| Tecla | Acción |
|---|---|
| Flechas, Home, End, PgUp, PgDn | mover el cursor |
| Escribir, Enter, Backspace, Delete | editar texto |
| `Insert` | alternar modo sobrescribir (indicador `OVR`) |
| `Ctrl+S` o `F2` | guardar (en pestaña sin título abre Guardar como) |
| `Ctrl+Q` | salir (pregunta si hay cambios sin guardar: Y/N/C — acepta S en español) |
| `Ctrl+W` | cerrar pestaña (pregunta si hay cambios) |
| `Ctrl+PgDn` / `Ctrl+PgUp` | pestaña siguiente / anterior (cicla) |
| `Ctrl+1` | ir al último documento del panel 1 (`Ctrl+2` reservada para el panel 2) |
| `Ctrl+Z` / `Ctrl+Y` | deshacer / rehacer (historial acotado por pestaña; cada tanda de tipeo es un paso) |
| `F3` / `Ctrl+Shift+S` | guardar todo (pestañas con ruta; omite sin título) |
| `Ctrl+O` | abrir archivo (prompt de ruta) |
| Clic izquierdo en el editor | colocar el cursor (se adapta al desplazamiento) |
| Números de línea | margen de números de línea siempre visible, alineados a la derecha, con énfasis en la línea del cursor |
| Clic izquierdo en una pestaña | cambiar al documento de esa pestaña |
| Rueda del ratón (arriba/abajo) | desplazar la vista (mismo rango que PgUp/PgDn) |

## Configuración

`editormini.json` junto al ejecutable (portable-first; en desarrollo queda en `target/debug/`). Si no existe, se usan los valores por defecto; si está mal formado, el editor avisa y corta:

```json
{
  "language": "en",
  "theme": "dark-plus",
  "export": {
    "markdown_pdf_command": null,
    "markdown_pdf_css": null,
    "typst_command": null
  }
}
```

- `language`: `"en"` (por defecto) o `"es"`.
- `theme`: `"dark-plus"` (por defecto) o `"light-plus"` (preparado, aún sin paleta completa).
- `export`: sección **reservada** para la futura exportación a PDF (generador externo + plantilla CSS externa) y Typst.

## Estructura del proyecto

```
src/
  main.rs      CLI, carga de archivos, ciclo de vida del terminal
  app.rs       bucle de eventos, teclas, dibujo, pestañas, modales
  document/    modelo de documento (líneas, BOM, fin de línea, solo-lectura)
  pane.rs      árbol de paneles (preparado para pantalla dividida)
  i18n.rs      catálogo de mensajes EN/ES (exhaustivo por compilación)
  config.rs    configuración JSON junto al ejecutable
docs/          plan del proyecto, avances y hallazgos
odd/           tareas del flujo de trabajo (ODD)
```

## Rumbo (resumen)

Pantalla dividida con sincronización para comparar archivos, paleta de comandos, explorador lateral, deshacer completo, LSP/linter, y exportación a PDF para Markdown y Typst. Detalle completo en [docs/plan.md](docs/plan.md).

## Licencia

MIT
