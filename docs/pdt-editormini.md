editormini


Diferencias entre las bases de datos orientadas a columnas y las relacionales orientadas a filas.
Ventajas y beneficios de cada una.

por que las bases de datos orientadas a columnas son mas eficientes para la analítica.

Que son los formatos Parquet, CSV y JSON

duckdb puede leer un fichero que corresponde a una base sqlite3






La barra completa de color verde está bien, prefiero que sea un verde mas apagado, pero visible.
Cuando la seleccion era en video inverso, funcionaba correcta, ahora inicia como si todo estuviera seleccoinado. Es un poco raro de explicar con palabras.

Ctrl+PgUp desafectado, pero deberia funcionar para desplazamiento en la misma carpeta de forma mas rapida que con las teclas flecha del cursor.

BackSpace al directorio funciona bien.

----

el color verde de la barra de fila activa está bien.

La seleccion sigue siendo extraña, inicia todo verde, y a medida que bajo se vá deseleccionando y la parte superior de esa selecion ahora parcial es la fila activa.

Ctrl+PgUp / Ctrl+PgDn de a 10 pasos está bien, pero anote para realizar luego, que sea la cantidad de filas de la pantalla, como hacer total commander.

-----

no sale con Esc aunque no tenga dialogo activo, está bien, mejor que solo salga con F10

Al confirmar el borrado con F8 se queda colgado en el mensaje Processing...
aunque si elimina el directorio

El cuadro que muestra al presionar F7 u F8 debe tener un marco para que sea visible sus limites.

Vaya documentando todos los cambios y correcciones de errores para tener en cuenta para cuando se desarrolle la aplicacion hermana y no caer en los mismo errores o cambios

----

Ahora los menu contextuales tienen borde con titulo centrado.
Deberían tener menos altura, le sobran varios renglones.

F8 Funciona correctamente, ya no se cuelga y realiza el borrado.

Ademas detecté que presionando Home / End la linea activa se posiciona en la primera y la ultima respectivamente. Eso está bien.

-----

Copia bien.
Solo detalles.
Al presionar F8 el cuadro de confirmacion le falta altura, agregue 2 renglones de alto.
Mientras está copiando.
Podría agregar 2 barras de estado.
Una para el % del archivo actual.
Y la segunda para el % del total?
Y luego del 2% un estimado del tiempo restante y actualizarlo la estimación periódicamente, como hace el total commander.
Consulta, estas situaciones de funcionamiento. Puedes verificarlas en el codigo del total commander que está instalado en el sistema? Es que hay varias opciones de funcionamiento del total commander que son un standard desde hace décadas que no estás contemplando.

------

necesito realizar un editor que de a poco se parezca a visual studio en la forma de uso. Pero tiene que ser para terminal de texto (TUI)
Aqui tiene un par de ejemplos c:\Users\USER\Downloads\micro-2.0.15\micro.exe y c:\Users\USER\Downloads\edit-2.0.0-x86_64-windows\edit.exe 
para analizarlos tiene el proyecto que sirve para ingenieria inversa en d:\Desarrollo\rea\

-----

Lo que busco es un editor para reemplazar visual studio, que sea agil, consuma pocos recursos, que tenga su estetica y branding. y que funciona en terminal (TUI)

-----

Menu default en ingles estilo VSCode, pero que se pueda cambiar a español, tambien como vscode.
Colores y temas, vscode standard oscuro.
Preparado para modo claro / oscuro
Dejar preparado para luego: Dividir pantalla, modificar division con el mouse, sincronizar las 2 divisiones, para cortejar 2 archivos.

-----

Cuando tenga mas de 1 archivo abierto, con Ctrl+PGDown y Ctrl+PgUp se pasa a los archivos siguientes. de cada panel, cuando se llega a un borde se pasa al otro panel.
Ctrl+1 va al ultimo documento del panel 1 y análogo con Ctrl+2

-----

Soportar archivos markdown. 
Contemplar a futuro exportar a pdf con plantilla css que le dé formato (estos pueden ser archivos externos, tanto el generador de pdf y el .css)
contemplar a futuro soporte de formato Typst y exportacion a pdf.

-----

Si no es muy caro, contemplar la posibilidad de que tenga modo de uso modal, aunque sea minimo y opcional activable.

-----

Al escribir siempre como si estuviera modo insertar activado.
Al presionar Crtl+S no parecia grabar. Preguntó al salir, pero parece que tampoco grabó nada.

----

Cree documentacion y guardelo en una carpeta. Plan, avances, hallazgo. Ruta de vida del proyecto.
Agrege readme en idioma español.

-----

Agregar a las lista por hacer.
Ctrl+Shift+S Graba todos los documentos abiertos que necesiten grabarse.

-----

Guarde todas las especificaciones en un documento para una posible aplicacion hermana en otro lenguaje para otra plataforma.
Realice todas las tareas que sean posibles en sub agentes. Para minimizar tiempos y preservar el contexto principal.

----

Consulta.
Que es mas efectivo.
Ir probando por etapas o realizar todas las etapas de una y revisar al final?
Otra Consulta
Que es mas efectivo cuando los cambios son costosos.
Anotar el error, salvarlo para que no pinche, tratar de evitarlo y corregir todo al final aunque involucre comenzar de cero.



Continua.
Recuerda Realizar todas las tareas que sean posibles en sub agentes. Para minimizar tiempos y preservar el contexto principal.
Si ejecuto por segunda vez el comando cargo run -- src/main.rs Cargo.toml docs/hallazgos.md porque hubo cambios, importan los archivos anteriores o los sobreescribe sin problemas.

-----

Guarda esto en tu memoria persistente para todos los proyectos. Esto tiene prioridad para funturos proyectos.
Si pasa como en este proyecto, que hay cosas que traen problemas por no tener el repositorio git al iniciar sesion, frena y avísame, y si te pido continuar, no dejes de avisar recurrentemente, es un incordio esto que pasa, que se solucion con solo salir y volver a ingresar.


Como editor de codigo debería mostrar los numero de linea.
Ctrl+F para busqueda de texto en el documento activo con cuadro de dialogo. A futuro que permita seleccionar algunas opciones como case sensitive busqueda regex.
Ctrl+Shift+F Para busqueda en todos los archivos abiertos.
Similar para las busquedas con Ctrl+H y su variante.


1. Tu verificación visual de la tarea 8: probá Ctrl+Shift+S (guardar todo), Ctrl+O (abrir), y Ctrl+S sobre un tab sin título (Guardar-como).
2. Tarea 7 (auditoría i18n) → 9 (mouse) → 10 (status bar) → 11 (release portable).


Colocar un help en la parte inferios con los comandos mas elementales al estilo nano.
Al elegir abrir un archivo desde dentro del editor que sea mas amigable en interactivo que solo preguntar el archivo.


De la ayuda inferior quite alguna opciones, está muy cargado y se pierde legibilidad, quite deshacer, es obvios igual que el buscar.
La barra de busqueda en color amarillo no me convence mucho la combinacion, pero zafa, oscurezca el texto allí, se pierde legibilidad con color gris y fondo amarillo.
Contempe a futuro la posibilidad de multicursor si no es muy caro. Luego veremos variantes de ella, como tiene vscode.


La barra de busqueda ahora es legible, la proxima busqueda se confirma con enter, pero debería aceptar ademas F3, Ademas la barra deberia ser mas pequeña como en vscode. Anotar para luego. 
Reemplazar, no entiendo como hacer para colocar el texto por el que se debe reemplazar.
En la barra inferior no queda nada, solo debia quitar deshacer y Busqueda Global.


Al elegir abrir un archivo desde dentro del editor que sea mas amigable en interactivo que solo preguntar el archivo.
Asi como Shif+Enter retrocede lo debe hacer Shift+F3.
Ahora se puede reemplazar, anotar para hacerlo mas similar a vscode.
La barra inferior ahora no se borra.
Dependiendo del nombre del archivo, el final del nombre queda pegado a las coordenadas del cursor.
Como en VScode el tipo de archivo reconocido debe estar mas a la derecha.
Agregar poder seleccionar texto para luego copiar con ctrl+C y pegar con ctrl+v. mantener compatibilidad con Shift+Insert y Shift+Supr como era antes.
Poder moverse por el texto con Ctrl+Flecha derecha e izquierda que avance de a palabra y si se agrega Shit que lo haga seleccionando texto.
Presente los desplazamientos standard de VScode, eso está documentado.


Shift+fecha Selecciona texto, pero si hay texto seleccionado, presionar suprimir o backspace borra solo el caracter, cuando debe borrar lo seleccionado.
Ctrl+flechas se mueve por palabras, es correcto.
Ctrl+Shift+flechas Selecciona por palabras, es correcto
Shift+PgUp/PgDn selecciona con ancla fija, es correcto.
Ctrl+O abre un pequeño menu con una lista de los archivos de la carpeta, bien. No devuelve el texto de ayuda de la barra inferior. Debe ser mas ancha la ventanida que muestra los nombres de los archivos, no completa nombre de archivos de mas de unos pocos caracteres en el nombre. Agregar un pequeño borde para que se noten los limites del la ventanita.
Ctrl+W cierra pestaña, pregunta si necesita ser guardado, es correcto


Borrar texto seleccionado, ok
Ventana de archivos a abrir, ok. Ademan no elimina el texto ayuda de la barra inferior.
Ya que el clipboard queda para luego.
Pensar bien que se va a colocar, para que sin gastar demasiado tenga cosas utiles, como un historial de las ultimas n copiar al porta papeles o funcionalidades similares.
Que tenga una secuencia de teclas para eliminar una fila completa Ctrl+Shit+k.
Si se presiona Shift+Flecha arriba/abajo desplace una linea completa en esa direccion.
Ctrl+G para ir a una linea en particular.
El texto seleccionado que se elimine al presionar Backspace o Suprimir
Tiene algo de seleccion multicursor?
Ctrl+Shift con flechas para seleccion rectangular.

----
