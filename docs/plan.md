# Plan del proyecto — ruta de vida de editormini

## Visión

Un editor de texto para terminal que se use como Visual Studio Code: pestañas, paleta de comandos, resaltado por extensión con la paleta estándar de VS Code, rápido, liviano, y distribuible como **un solo ejecutable portable** para Windows primero.

Requisitos del dueño del proyecto (registrados en las rondas de decisión):

1. Forma de uso lo más parecida posible a VS Code.
2. Resaltado de sintaxis por extensión de archivo, colores estilo Dark+ estándar de VS Code; preparado para modo claro/oscuro.
3. Menú en inglés por defecto, cambiable a español, como VS Code.
4. Linter no necesario por ahora, pero la arquitectura debe contemplarlo (LSP).
5. Ágil, rápido, pocos recursos, un solo ejecutable portable.
6. Soporte de Markdown; a futuro exportar a PDF con plantilla CSS externa y soporte de Typst (gramática y compilador externos).
7. A futuro: pantalla dividida, divisor ajustable con mouse y scroll sincronizado para comparar dos archivos.

## Decisiones de stack (y por qué)

| Decisión | Alternativas evaluadas | Razón |
|---|---|---|
| **Rust** | Go+tcell (stack de micro), Node (motor TextMate literal), C# | Mejor equilibrio rendimiento/exe chico/portabilidad; syntect y tree-sitter nativos |
| **crossterm + ratatui** | TUI propia como MS Edit | Llegar al resultado sin construir un framebuffer; crossterm da Windows Terminal + consolas modernas |
| **syntect** (hitos tempranos) | tree-sitter ahora | Carga gramáticas TextMate por extensión y temas tipo VS Code; tree-sitter queda como evolución (Helix/Zed) |
| **Catálogo i18n propio en código** | crates de i18n | Exhaustivo por compilación: si falta una traducción, no compila; cero dependencias |
| **Análisis de referencias: binario + fuentes públicas** | Ghidra profundo | Ambos editores de referencia son open source; correlacionar cuesta 1% y rinde 10x. Ghidra queda para preguntas específicas |

## Arquitectura

Capas, de adentro hacia afuera (el reemplazo de estructuras internas no debe tocar la UI):

```
terminal (crossterm/ratatui) → app (eventos, teclas, dibujo) → documento (texto) → disco
                                   ↓
                     pane tree · i18n · config · theme (hitos)
```

Preparaciones ya plantadas para el futuro:

- **Árbol de paneles** (`pane.rs`): la raíz de la UI es un árbol `Single | Split`, hoy con un solo panel. La pantalla dividida será extender el árbol, no refactorizar.
- **`EditRecord` con inversa**: cada edición devuelve qué se aplicó y cómo deshacerla; el undo (hito 1) solo apila esos registros.
- **Registro de lenguajes extensible**: los 12 lenguajes curados compilados + carga de gramáticas externas (TextMate/`.sublime-syntax`) en runtime → Typst después es soltar un archivo en `languages/`.
- **Sección `export` inerte en config**: generador de PDF y plantilla CSS serán **archivos externos** referenciados; el editor solo orquesta.
- **Catálogo i18n**: todo texto de UI pasa por `i18n::t()`; EN por defecto, ES completo.

## Ruta de vida (hitos)

### Hito 1 — Editor base con pestañas *(completado)*
Abrir, editar, guardar varios archivos en pestañas con resaltado por extensión y exe único. Detalle de tareas y estado: `odd/tasks/milestone-1-editor-tabs.md` y [avances.md](avances.md).

Entregado además sobre esa base: búsqueda/reemplazo en documento activo y cruzado (`Ctrl+F/H`, `Ctrl+Shift+F/H`), selección con portapapeles del sistema y movimientos estándar de VS Code, operaciones de línea (`Alt+↑/↓`, `Ctrl+K`, `F8`, `Ctrl+G`), lista interactiva de apertura (`Ctrl+O`), gutter de números de línea y mouse básico. Siguiente parada: **Hito 2 — paleta de comandos**.

### Hito 2 — Paleta de comandos *(completado)*
`Ctrl+Shift+P` con búsqueda difusa sobre un registro de comandos; `Ctrl+P` para abrir archivos (fuzzy picker). Es el corazón de "usarse como VS Code". Entregado: matcher difuso propio (puntaje por contigüidad, inicio de palabra y huecos), registro de 19 comandos localizados EN/ES, paleta con `F4` de respaldo y quick open sobre pestañas + directorio activo. El keymap remapeable queda para el hito 4.

### Hito 3 — Explorador lateral
Árbol de archivos del proyecto navegable con teclado (y mouse), apertura por enter.

### Hito 4 — Configuración completa
`editormini.json` con keymap, tema e idioma recargables; modo claro completo (la estructura ya existe); deshacer con historial completo estilo VS Code.

### Hito 5 — Pantalla dividida
Split horizontal/vertical, divisor arrastrable con mouse, scroll sincronizado entre paneles para **comparar dos archivos** lado a lado.

### Hito 6 — LSP / linter
Cliente LSP (arranque de servidores, diagnóstico en la status bar/gutter). La arquitectura de comandos y el bucle async ya la contemplan desde el hito 1.

### Hito 7 — Markdown, PDF y Typst
Exportación Markdown→PDF orquestando un **generador externo** con **plantilla CSS externa** (ambos referenciados en config); resaltado y exportación Typst vía gramática externa + `typst compile`.

## Criterios de calidad (por tarea)

- Tests automatizados verdes (lógica pura en test-first; UI verificada interactivamente por el dueño).
- Build con **0 warnings**.
- Todo texto de usuario presente en los dos idiomas (garantizado por compilación).
- Un commit de trabajo por tarea con mensaje convencional; evidencia registrada en el documento de tareas y en [avances.md](avances.md).
- El dueño prueba cada aspecto probable antes de continuar ("pare cuando haya algo que probar").
