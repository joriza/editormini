# Especificación maestra de editormini

> **Propósito de este documento:** permitir construir una **aplicación hermana** de editormini en **cualquier lenguaje y para cualquier plataforma**, reproduciendo su comportamiento, su formato de datos y su experiencia de uso, sin leer el código Rust. Todo lo marcado **[H1]** está implementado en el hito 1 y es la referencia de comportamiento observado; lo marcado **[Hn]** está planificado para el hito n (ver [plan.md](plan.md)).

---

## 1. Identidad y alcance

- **Producto:** editor de texto para terminal (TUI) con la forma de uso de Visual Studio Code.
- **Nombre de binario:** `editormini`. Paquete/proyecto: `editormini`.
- **Plataforma objetivo inicial:** Windows (Windows Terminal / consolas VT modernas). El diseño es portable a cualquier plataforma con terminal VT100+ o equivalente.
- **Distribución:** un solo ejecutable, sin dependencias de runtime; configuración opcional en un archivo junto al ejecutable (portable-first).
- **Idiomas de interfaz:** inglés (por defecto) y español, conmutables.

## 2. Requisitos de plataforma

1. Terminal con soporte de secuencias VT (render por celdas, cursor posicionable, entrada de teclas con modificadores). Recomendado: Windows Terminal.
2. Modo crudo (raw) + pantalla alternativa al iniciar; restaurar terminal al salir (también en errores).
3. Si stdout no es un TTY: mensaje de error localizado y código de salida 2.
4. Un ejecutable por plataforma; sin instalación.

## 3. Modelo del documento **[H1]**

Estado por documento:

| Campo | Semántica |
|---|---|
| `lines` | Líneas de texto **sin terminadores**. Mínimo 1 línea (archivo vacío = 1 línea vacía). |
| `path` | Ruta absoluta o relativa; `None` = documento sin título. |
| `line_ending` | `CRLF` o `LF`. |
| `has_bom` | BOM UTF-8 detectado al cargar; se re-emite al guardar. |
| `ends_with_newline` | Si el archivo original terminaba en nueva línea; se respeta al guardar. Archivos nuevos: `true`. |
| `modified` | Bandera de cambios sin guardar; la limpia el guardado exitoso. |
| `read_only` | Bloquea EDICIONES (no el guardado). |

Reglas de carga (`from_path`):

1. Leer bytes completos. Si `bytes > 10 × 1024 × 1024` (10 MB, constante `READ_ONLY_THRESHOLD`) → cargar con `read_only = true`.
2. BOM: si los bytes empiezan con `EF BB BF`, marcar `has_bom` y descartarlo del texto.
3. Codificación: **solo UTF-8**; bytes inválidos → error `NotUtf8` con mensaje claro.
4. Fines de línea: contar `CRLF` vs `LF` solos; **domina el mayor; empate → CRLF** (Windows-first). Los archivos mixtos se normalizan al dominante al guardar.
5. `ends_with_newline` = el texto terminaba en `\n`.

Reglas de creación:

- Nuevo sin título: 1 línea vacía, `CRLF`, sin BOM, `ends_with_newline = true`, no modificado.
- Nuevo con ruta (el archivo no existe en disco): igual al sin título + `path`.

## 4. Operaciones de edición **[H1]**

Posiciones: `(línea, columna)` con columna en **caracteres** (no bytes). Toda operación valida límites → error `OutOfBounds{line, col}`.

- **`insert(line, col, text)`**: inserta `text`; los `\n` dividen líneas (los `\r\n` y `\r` se normalizan a `\n` en la operación; el fin de línea almacenado aplica solo al guardar). Devuelve registro de edición.
- **`delete_range(start, end)`**: borra el rango contiguo de `(start_line,start_col)` a `(end_line,end_col)` (extremo exclusivo); si cruza líneas, las une. Devuelve registro.
- **`apply(record)`**: aplica un registro (usado por undo/redo).
- Toda mutación: si `read_only` → error `ReadOnly`; si ok → `modified = true`.

### Registro de edición (`EditRecord`) — primitiva de undo

`Insert{line, col, text}` o `Delete{start_line, start_col, end_line, end_col, text}` con `inverse()`:

- inversa de `Insert{L,C,text}` → `Delete` desde `(L,C)` hasta:
  - **una sola línea**: `(L, C + caracteres(text))` — ⚠️ no `caracteres(text)` a secas (defecto histórico corregido);
  - **multi-línea**: `(L + n_segmentos − 1, caracteres(último_segmento))` — el último segmento inicia en columna 0 de su nueva línea.
- inversa de `Delete` → `Insert` en `(start_line, start_col)` con el texto eliminado.

Errores: `Io`, `NotUtf8`, `Untitled`, `ReadOnly`, `OutOfBounds{line, col}` — con mensajes en inglés, concisos.

## 5. Deshacer / rehacer **[H1]**

- Historia **por pestaña**, **acotada a 100 pasos** (constante `UNDO_LIMIT`; se descarta el más antiguo).
- Un **paso** = grupo de registros. Reglas de agrupación:
  - Tecla de carácter normal → **continúa la tandas (burst)** en curso: se agrega al último grupo.
  - Enter, Backspace, Delete, sobrescribir → **cierran la tanda**: abren grupo nuevo. (Sobrescribir registra borrado+inserción como **un solo paso atómico**.)
- `Ctrl+Z`: desapila el último grupo, aplica inversas en **orden inverso** (más nueva primero), apila el grupo en redo, cursor al inicio del primer registro del grupo.
- `Ctrl+Y` (y `Ctrl+Shift+Z`): desapila de redo, aplica en orden original, apila en undo, cursor al final del último registro reaplicado.
- Cualquier edición nueva **limpia el redo**.
- Si no hay nada que deshacer/rehacer: la tecla se ignora sin mensaje.

## 6. Interfaz **[H1]**

Layout de 3 bandas (alto completo del terminal):

1. **Barra de pestañas** (1 fila): una pestaña por documento, `nombre ● ` si tiene cambios sin guardar; activa resaltada; separador `│` entre inactivas. `●` también en la status bar.
2. **Viewport de edición** (resto): solo las líneas visibles se renderizan (y en el hito 1+, solo las visibles se resaltan). Cursor del terminal posicionado sobre la celda lógica.
3. **Barra de estado** (1 fila): `nombre ● Ln l, Col c` + modo `OVR ` si sobrescribe + mensaje de estado a la derecha. Fondo azul `#007ACC`, texto blanco. Cuando hay confirmación pendiente, la banda muestra el prompt con fondo ámbar.

Scroll: el viewport sigue al cursor (vertical y horizontal); `scroll_top`/`col_off` se ajustan al perder visibilidad.

### Modales de confirmación

Prompt (misma banda de estado, invertida): EN `Save changes before closing? [Y]es / [N]o / [C]ancel` — ES `¿Guardar los cambios antes de cerrar? [S]í / [N]o / [C]ancelar`. Se aceptan `Y` y `S` para sí en ambos idiomas; `Esc` = cancelar.

- **Salir** (`Ctrl+Q` con cambios sin guardar en CUALQUIER pestaña): `Y` guarda todas las pestañas con ruta y sale; si un guardado falla, **no sale** y muestra el error. `N` sale sin guardar. `C` cancela.
- **Cerrar pestaña** (`Ctrl+W` con esa pestaña modificada): `Y` guarda esa pestaña y la cierra; `N` la descarta; `C` cancela. Al cerrar la última pestaña queda un documento nuevo vacío.

## 7. Keymap

### Implementado **[H1]**

| Tecla | Acción |
|---|---|
| Caracteres imprimibles | insertar en cursor (tanda de undo) |
| `Enter` | nueva línea (cierra tanda) |
| `Backspace` / `Delete` | borrar atrás / adelante (cierran tanda) |
| `Insert` | alternar sobrescribir (indicador `OVR`) |
| Flechas, `Home`, `End`, `PgUp`, `PgDn` | movimiento |
| `Ctrl+S` / `F2` | guardar pestaña activa |
| `Ctrl+Q` | salir (prompt si hay cambios) |
| `Ctrl+W` | cerrar pestaña (prompt si hay cambios) |
| `Ctrl+PgDn` / `Ctrl+PgUp` | pestaña siguiente / anterior (cicla; preparado para saltar entre paneles) |
| `Ctrl+1` | ir al último documento del panel 1 |
| `Ctrl+2` | reservada: último documento del panel 2 (no-op hasta splits) |
| `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | deshacer / rehacer / rehacer |

### Planificado

| Tecla | Hito | Acción |
|---|---|---|
| `Ctrl+Shift+S` | 2 | **guardar todas las pestañas modificadas** (pedido del dueño; pendiente) |
| `Ctrl+Shift+P` | 2 | paleta de comandos |
| `Ctrl+P` | 2 | abrir archivo por búsqueda difusa |
| `Ctrl+O` | 2 | abrir archivo (diálogo/navegador) |
| `Ctrl+N` | 2 | nuevo documento |
| Click / rueda | 2 | mouse: cursor, pestañas, scroll |

Regla de fallback: toda tecla crítica debe tener alternativa que ningún terminal pueda absorber (ej. `F2` por `Ctrl+S`, que mintty consume como XOFF).

## 8. Internacionalización **[H1]**

- **Catálogo de mensajes** con claves tipadas; toda cadena visible pasa por él. Regla de oro: **si una clave existe, existe en todos los idiomas o no compila** (exhaustividad garantizada por el compilador, no por revisión manual).
- Idiomas: `en` (por defecto), `es`.
- Selección: campo `language` de config + flag `--lang` (la flag gana). Errores previos a la config usan la flag o el default.
- Plantillas con placeholders `{nombre}` sustituibles (ej. `Ln {ln}, Col {col}`).
- Nuevos idiomas = nueva columna en el catálogo + valor de config; sin cambios en la lógica.

## 9. Configuración **[H1]**

Archivo `editormini.json` **junto al ejecutable** (portable-first; en desarrollo, `target/debug/`).

Política: archivo ausente → valores por defecto; archivo malformado → error visible y salida (código 1); campos desconocidos → ignorados (compatibilidad futura).

```json
{
  "language": "en",              // "en" | "es"
  "theme": "dark-plus",          // "dark-plus" | "light-plus"
  "export": {                    // reservado (inerte hoy)
    "markdown_pdf_command": null, // comando EXTERNO que renderiza Markdown a PDF
    "markdown_pdf_css": null,     // archivo CSS externo de plantilla
    "typst_command": null         // comando externo de Typst
  }
}
```

## 10. Resaltado de sintaxis y temas **[H1·parcial]**

- **Por extensión de archivo**, con motor de gramáticas TextMate/Sublime (syntect en Rust; en una app hermana, cualquier motor equivalente: TextMate, tree-sitter).
- Lenguajes curados (hito 1): Rust, Go, JavaScript/TypeScript (+JSX/TSX), Python, C/C++, JSON, TOML, YAML, Markdown, HTML, CSS, XML, Shell (.sh/.bat/.ps1).
- **Solo se resaltan las líneas visibles** por frame.
- Temas: **Dark+ de VS Code** por defecto. Paleta chrome (hex): fondo editor `#1E1E1E`, texto `#D4D4D4`, pestaña activa `#09477B`, pestaña inactiva `#2D2D2D`, acento/status `#007ACC`, modificado `#E2C089`. `Light+` existe como estructura con paleta placeholder (hito 4 lo completa).
- Gramáticas **externas**: el motor debe poder cargar gramáticas de archivos (TextMate/`.sublime-syntax`) desde un directorio de datos (p. ej. `languages/`) sin recompilar — camino de soporte para Typst y otros.

## 11. Flujo de archivos **[H1]**

- CLI: `editormini [ARCHIVO]... [--lang en|es]`. Cada archivo = una pestaña; los inexistentes se crean al primer guardado (documento con ruta, vacío).
- Guardar: escribe BOM (si `has_bom`), líneas unidas con el fin de línea almacenado, nueva línea final según `ends_with_newline`. Éxito → mensaje `Saved <ruta> (<n> B)`; documento sin ruta → error visible (Guardar Como llega en hito 2).
- Salir con cambios: modal (ver §6).

## 12. Extensiones especificadas (futuro)

- **Paleta de comandos [H2]**: registro de comandos con nombre localizado, búsqueda difusa, ejecución; el keymap se vuelve una tabla remapeable hacia ese registro.
- **Explorador lateral [H3]**: árbol de archivos; ancho fijo inicial; navegable con teclado.
- **Pantalla dividida [H5]**: el layout raíz es un **árbol de paneles** (`Single(doc) | Split{primero, segundo, ratio‰, vertical}`) ya definido; split = extender el árbol. Divisor arrastrable con mouse; **scroll sincronizado** entre dos paneles (bandera de enlace) para comparar archivos.
- **LSP / linter [H6]**: cliente LSP estándar; los diagnósticos entran por la status bar y el gutter. El bucle de eventos debe ser compatible con I/O asíncrona.
- **Exportación [H7]**: Markdown→PDF orquestando un **comando externo** con **plantilla CSS externa** (ambos configurados en `export`); Typst como gramática externa + `typst compile`. El editor nunca embebe el generador.
- **Mouse [H2/H5]**: click = cursor, click en pestañas, rueda = scroll; luego arrastre de selección y divisor.

## 13. Criterios de calidad

1. Suite de tests automatizados verde (hito 1: 53 tests — modelo, keymap, i18n, config, temas, resaltado).
2. Compilación con **cero warnings**.
3. i18n completo garantizado por compilación.
4. Un commit de trabajo por tarea, mensajes convencionales, evidencia registrada.
5. Verificación humana de cada entregable probable antes de continuar.

## 14. Referencias

- **micro 2.0.15** (Go 1.23 + tcell + plugins Lua): keymap y sistema de comandos.
- **Microsoft Edit 2.0.0** (Rust, TUI propia, gap buffer, resaltado `lsh`, fuzzy finder, localización): arquitectura interna obtenida por análisis del binario + PDB.
- Detalles y hallazgos del proceso: [hallazgos.md](hallazgos.md).

## 15. Glosario

| Término | Definición |
|---|---|
| **Tanda (burst)** | Secuencia de caracteres tipeados consecutivos que deshacer trata como un solo paso. |
| **Registro de edición** | Descripción de una operación aplicada + su inversa (unidad de undo). |
| **Panel** | Región rectangular del layout que muestra un documento. |
| **Sin título** | Documento sin ruta asignada; no puede guardarse hasta tenerla. |
| **Portable-first** | El ejecutable y su configuración viven juntos; copiar = instalar. |
