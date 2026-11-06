<p><img src="../../readme-icon.png" alt="Icono de la app Splat" width="168" height="168">&nbsp;&nbsp;&nbsp;<img src="screenshot.png" alt="Splat con Demo.md: el código fuente Markdown a la izquierda, la previsualización editable a la derecha" width="274" height="168"></p>

# Splat

**El editor de Markdown para macOS que te deja escribir en el código fuente o directamente en la previsualización.**

Splat muestra a la izquierda tu código fuente Markdown con colores de sintaxis y a la derecha una previsualización en directo, totalmente editable. Escribe en cualquiera de las dos: lo que escribas en la previsualización se convierte en un cambio mínimo en el código fuente, así que las líneas que no has tocado siguen idénticas byte a byte y tus diffs de Git se mantienen limpios. Splat entiende el Markdown que ya usas: CommonMark y GitHub Flavored Markdown, alertas de GitHub, enlaces wiki, incrustaciones y recuadros de Obsidian, extensiones de Pandoc y Typora, HTML incrustado, fórmulas LaTeX y diagramas de Mermaid, Graphviz y PlantUML, todo renderizado sin conexión. Explora y busca en toda una carpeta de notas, mira qué ha cambiado desde el último commit de Git, pega desde Word o una página web como Markdown y exporta a PDF, HTML o Word con un clic. Está escrito en Rust sobre controles genuinos de AppKit, habla 35 idiomas y no envía nada por la red a menos que se lo pidas.
 


[![Descargar Splat para macOS](https://img.shields.io/badge/Download-Splat_for_macOS-0A84FF?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)

**[Descargar Splat.dmg](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)**: firmado y notarizado por Apple, para macOS 14 o posterior en Apple Silicon e Intel.

**¿Has encontrado un error?** Splat mantiene idénticas byte a byte las líneas que no has tocado. Si aun así cambia alguna, o un archivo se muestra mal, [abre una incidencia](https://github.com/sventmueller/splat/issues/new/choose) y adjunta el archivo. Si Splat te resulta útil, una [estrella en GitHub](https://github.com/sventmueller/splat) ayuda a que otros lo encuentren.
 
 

[![Rust, edición 2024](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![macOS 14 o posterior](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)
![Universal: Apple Silicon e Intel](https://img.shields.io/badge/Universal-Apple_Silicon_%7C_Intel-333333?logo=apple&logoColor=white)
[![AppKit mediante objc2](https://img.shields.io/badge/AppKit-objc2_0.6-147EFB?logo=apple&logoColor=white)](https://github.com/madsmtm/objc2)
[![WebKit](https://img.shields.io/badge/WebKit-offline-0A84FF?logo=safari&logoColor=white)](https://webkit.org)
[![Comrak 0.59](https://img.shields.io/badge/comrak-0.59-DEA584?logo=markdown&logoColor=white)](https://github.com/kivikakk/comrak)
[![CommonMark y GFM](https://img.shields.io/badge/CommonMark-GFM-24292F?logo=github&logoColor=white)](https://github.github.com/gfm/)
[![MathJax 4.1.3](https://img.shields.io/badge/MathJax-4.1.3-1E6E9E?logo=latex&logoColor=white)](https://www.mathjax.org)
[![MathLive 0.111.4](https://img.shields.io/badge/MathLive-0.111.4-5A3FC0)](https://cortexjs.io/mathlive/)
[![Mermaid 12.3.0](https://img.shields.io/badge/Mermaid-12.3.0-FF3670?logo=mermaid&logoColor=white)](https://mermaid.js.org)
[![Graphviz mediante Viz.js 3.24](https://img.shields.io/badge/Graphviz-Viz.js_3.24-2596BE)](https://graphviz.org)
[![PlantUML 1.2026.9](https://img.shields.io/badge/PlantUML-1.2026.9-A80036)](https://plantuml.com)
![35 idiomas de interfaz](https://img.shields.io/badge/languages-35-4C9A2A)
![Sin conexión por omisión](https://img.shields.io/badge/network-offline_by_default-4C9A2A)
![Firmado con Hardened Runtime](https://img.shields.io/badge/Hardened_Runtime-signed-4C9A2A)
[![Licencia: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-4C9A2A)](#licencia)

[Bahasa Indonesia](../id/README.md) ·
[Bahasa Melayu](../ms/README.md) ·
[Català](../ca/README.md) ·
[Čeština](../cs/README.md) ·
[Dansk](../da/README.md) ·
[Deutsch](../de/README.md) ·
[English](../../../README.md) ·
**Español** ·
[Français](../fr/README.md) ·
[Hrvatski](../hr/README.md) ·
[Italiano](../it/README.md) ·
[Magyar](../hu/README.md) ·
[Nederlands](../nl/README.md) ·
[Norsk](../no/README.md) ·
[Polski](../pl/README.md) ·
[Português (Brasil)](../pt_BR/README.md) ·
[Português (Portugal)](../pt_PT/README.md) ·
[Română](../ro/README.md) ·
[Slovenčina](../sk/README.md) ·
[Slovenščina](../sl/README.md) ·
[Suomi](../fi/README.md) ·
[Svenska](../sv/README.md) ·
[Tiếng Việt](../vi/README.md) ·
[Türkçe](../tr/README.md) ·
[Ελληνικά](../el/README.md) ·
[Русский](../ru/README.md) ·
[Українська](../uk/README.md) ·
[עברית](../he/README.md)‎ ·
[العربية](../ar/README.md)‎ ·
[हिन्दी](../hi/README.md) ·
[ไทย](../th/README.md) ·
[한국어](../ko/README.md) ·
[日本語](../ja/README.md) ·
[简体中文](../zh-Hans/README.md) ·
[繁體中文](../zh-Hant/README.md)

## Contenido

- [Resumen de funciones](#resumen-de-funciones)
- [Requisitos](#requisitos)
- [Instalación](#instalación)
- [Primeros pasos](#primeros-pasos)
- [Escritura](#escritura)
  - [En el código fuente](#en-el-código-fuente)
  - [En la previsualización](#en-la-previsualización)
  - [Comandos de formato](#comandos-de-formato)
  - [Listas y tareas](#listas-y-tareas)
  - [Tablas](#tablas)
  - [Pegar texto con formato](#pegar-texto-con-formato)
  - [Metadatos](#metadatos)
- [Markdown compatible](#markdown-compatible)
  - [CommonMark y GitHub Flavored Markdown](#commonmark-y-github-flavored-markdown)
  - [Recuadros](#recuadros)
  - [Sintaxis ampliada](#sintaxis-ampliada)
  - [Obsidian](#obsidian)
  - [Citas y referencias](#citas-y-referencias)
  - [HTML incrustado](#html-incrustado)
  - [Puntuación tipográfica](#puntuación-tipográfica)
  - [Resumen de sintaxis](#resumen-de-sintaxis)
- [Fórmulas](#fórmulas)
- [Diagramas](#diagramas)
- [Imágenes](#imágenes)
- [Enlaces y navegación](#enlaces-y-navegación)
- [Notas en una carpeta](#notas-en-una-carpeta)
- [Cambios desde el último commit](#cambios-desde-el-último-commit)
- [Exportar, imprimir y compartir](#exportar-imprimir-y-compartir)
- [Importar documentos de Word](#importar-documentos-de-word)
- [Apariencia y accesibilidad](#apariencia-y-accesibilidad)
- [Idiomas](#idiomas)
- [Atajos de teclado](#atajos-de-teclado)
- [Privacidad y seguridad](#privacidad-y-seguridad)
- [Documentos grandes](#documentos-grandes)
- [Limitaciones conocidas](#limitaciones-conocidas)
- [Solución de problemas](#solución-de-problemas)
- [Para desarrolladores](#para-desarrolladores)
  - [Componentes de terceros](#componentes-de-terceros)
- [Licencia](#licencia)

## Resumen de funciones

- **Dos formas de escribir, un solo texto.** El código fuente y la previsualización son editables. El archivo Markdown es la única verdad: hay un solo historial de deshacer por documento, y cada edición en la previsualización se traduce en el cambio más pequeño posible en el código fuente.
- **El Markdown que ya usas.** CommonMark, GitHub Flavored Markdown, alertas de GitHub, extensiones de Obsidian, Pandoc y Typora, shortcodes de emoji, spoilers, listas de definiciones, anclas de encabezamientos y HTML como en los README de GitHub.
- **Fórmulas y diagramas integrados.** Fórmulas LaTeX con MathJax y un editor visual de fórmulas. Diagramas en Mermaid, Graphviz y PlantUML, renderizados en tu Mac sin Java, sin servidor y sin conexión de red.
- **Un botón para todo.** Cada elemento de Markdown tiene un botón en la barra de herramientas y una opción de menú, y los más habituales también un atajo de teclado. Las tablas tienen su propio menú para filas, columnas y alineación.
- **Una carpeta, no solo un archivo.** La barra lateral lista las notas que rodean a un documento y busca en todas ellas. Al renombrar una nota cambian los enlaces a ella, y todos los bytes a su alrededor se mantienen.
- **Los cambios de un vistazo.** En un repositorio Git, unas barras junto al texto marcan lo que difiere del último commit, en ambos paneles.
- **De Word y la web a Markdown.** Pega texto con formato como Markdown, celdas de una hoja de cálculo como tabla, e importa documentos de Word enteros con sus imágenes.
- **Exporta sin perder nada.** PDF, un único archivo HTML autocontenido o un documento de Word con encabezamientos, listas, tablas, notas al pie e imágenes de verdad. Añade números de página, cabeceras y una portada, o usa la plantilla de Word de tu empresa.
- **Nativa hasta la médula.** Ventanas, pestañas, menús, popovers, Versiones, Autoguardado, el menú de compartir, modo oscuro, corrección ortográfica, dictado y VoiceOver de AppKit, sin imitaciones. Sin Electron y sin vista web para el texto en sí.
- **Rápida con archivos grandes.** Renderizar una pulsación de tecla en un documento de 10 MB tarda unos 10 ms, antes del trabajo propio de la vista.
- **Privada y cuidadosa.** Las imágenes de la web se cargan solo cuando lo permites, los enlaces a otras apps preguntan antes y todo lo que decodifica datos de imagen no fiables se ejecuta en procesos auxiliares aislados en un sandbox.
- **En tu idioma.** La interfaz y la referencia rápida de Markdown integrada están disponibles en 35 idiomas: alemán, árabe, catalán, checo, chino (simplificado), chino (tradicional), coreano, croata, danés, eslovaco, esloveno, español, finés, francés, griego, hebreo, hindi, húngaro, indonesio, inglés, italiano, japonés, malayo, neerlandés, noruego, polaco, portugués (Brasil), portugués (Portugal), rumano, ruso, sueco, tailandés, turco, ucraniano y vietnamita.

## Requisitos

- macOS 14 Sonoma o posterior. Splat se desarrolla y se prueba en macOS 27, donde adopta automáticamente el diseño Liquid Glass.
- Un Mac con Apple Silicon o con procesador Intel.
- Unos 45 MB de espacio en disco.

## Instalación

**Desde la imagen de disco.** Descarga [`Splat.dmg`](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg), ábrela y arrastra Splat a la carpeta Aplicaciones. La imagen está firmada con un Developer ID y notarizada por Apple, así que se abre sin avisos.

**Con Homebrew.** El cask está en un tap propio:

```sh
brew install --cask sventmueller/tap/splat
```

`brew upgrade --cask splat` instala las versiones nuevas, y `brew uninstall --zap --cask splat` elimina además los ajustes y las cachés de Splat.

**Actualizaciones.** Splat, instalado desde la imagen de disco o con Homebrew, se mantiene al día por sí solo con [Sparkle](https://sparkle-project.org). En el segundo arranque pregunta si puede buscar actualizaciones por su cuenta, una vez al día. **Splat › Buscar actualizaciones…** busca al instante; **Buscar actualizaciones automáticamente** y **Descargar e instalar actualizaciones automáticamente**, en el mismo menú, cambian después tu elección. Splat solo instala una actualización si lleva la firma del desarrollador. Una compilación propia no tiene actualizador.

**Desde el código fuente.** Consulta [Compilación](../../../docs/development.md#building) para ver los requisitos previos y después:

```sh
make build
ditto target/release/Splat.app /Applications/Splat.app
open /Applications/Splat.app
```

Para abrir archivos Markdown con Splat haciendo doble clic, selecciona cualquier archivo `.md` en el Finder, elige **Archivo › Obtener información**, escoge Splat en **Abrir con** y haz clic en **Cambiar todo…**. Splat se registra para los archivos `.md`, `.markdown`, `.mdown` y `.mkd`.

## Primeros pasos

### La ventana

Cada documento se abre en su propia ventana, o en una pestaña si prefieres las pestañas:

- **Izquierda: el código fuente.** Tu Markdown en SF Mono con colores de sintaxis.
- **Derecha: la previsualización.** El documento renderizado, listo para editar.
- **Barra lateral (opcional).** Todos los encabezamientos en forma de árbol, un clic en uno lo muestra en ambos paneles: **Visualización › Mostrar esquema** (⌃⌘S). Otras dos pestañas contienen las [notas de la carpeta](#notas-en-una-carpeta) y una búsqueda en ellas. El botón junto al título de la ventana la muestra y la oculta, y las ventanas nuevas la abren tal como la dejó la última.
- **Barra de herramientas.** Botones agrupados según lo que cambian: caracteres, párrafos y elementos insertados, y después mostrar u ocultar la previsualización, Compartir y Nuevo documento. Se personaliza con **Visualización › Personalizar barra de herramientas…**.

Arrastra el divisor para dar más espacio a un lado. **Visualización › Ocultar previsualización** (⌥⌘P) deja solo el código fuente; Splat lo recuerda por ventana. Ambos paneles se desplazan a la vez, y la selección en un panel se resalta en el otro.

La esquina de la ventana muestra las palabras y los caracteres del texto y cuánto se tarda en leerlo, y los de la selección mientras la haya. Las direcciones de enlaces, las etiquetas HTML, los metadatos, los comentarios, las fórmulas y los diagramas no cuentan. Permanece oculto hasta que **Visualización › Mostrar recuento de palabras** lo activa.

### Documentos, pestañas y versiones

Splat es una app de documentos normal de macOS:

- **Autoguardado y Versiones.** Los cambios se guardan en el propio archivo. **Archivo › Volver a › Explorar todas las versiones…** te lleva atrás en el tiempo.
- **Cambios desde otros programas.** Cuando Git, un script u otro editor modifica un documento abierto, Splat lo recarga y conserva el cursor y la posición de desplazamiento. Si tienes cambios propios sin guardar, Splat los conserva, y macOS pregunta antes de sobrescribir la otra versión.
- **Los comandos de archivo de siempre.** Duplicar (⇧⌘S), Renombrar…, Trasladar a… y **Archivo › Abrir recientes**.
- **Pestañas.** **Archivo › Nueva pestaña** o el botón **+** de la barra de herramientas abre un documento nuevo como pestaña; el menú Ventana estándar fusiona, traslada y recorre las pestañas.
- **Posición de las ventanas.** Se recuerdan el tamaño y la posición de tu última ventana, también entre reinicios. Si ya hay una ventana en ese sitio, la nueva se abre algo desplazada.
- **El icono del Dock.** Un clic en él, después de cerrar la última ventana, abre un documento nuevo.
- **Abrir archivos Markdown con Splat.** **Splat › Definir como app por omisión para Markdown** hace que un doble clic en un archivo `.md` o `.markdown` abra Splat; macOS pide confirmación una vez. La opción muestra una marca mientras Splat es la app por omisión.

Los archivos deben ser texto UTF-8, o UTF-16 con marca de orden de bytes. Splat rechaza otras codificaciones en vez de adivinar y estropear el texto. La marca de orden de bytes se conserva, UTF-16 sigue siendo UTF-16, y en un archivo cuyas líneas terminan todas en CRLF, las líneas que añadas también terminan en CRLF.

**Tras un cierre inesperado**, como mucho se pierden los últimos segundos de escritura: cada cinco segundos, Splat copia el texto sin guardar en `~/Library/Application Support/Splat/Unsaved` y borra las copias cuando el documento se guarda o Splat se cierra con normalidad. En el siguiente arranque, lo que dejó un cierre inesperado pasa a `~/Library/Application Support/Splat/Recovery` y se avisa una sola vez, con un botón que abre la carpeta. Un cierre inesperado al que Splat aún puede reaccionar escribe allí también el texto de todos los documentos abiertos.

### Referencia rápida

**Ayuda › Referencia rápida de Markdown** abre una guía de consulta en tu idioma como documento nuevo sin título. Es un documento normal, así que puedes probarlo todo ahí mismo, en el código fuente o en la previsualización.

## Escritura

### En el código fuente

El código fuente es un editor de texto sin formato ajustado para Markdown:

- **Colores de sintaxis** como en VS Code: encabezamientos, énfasis, enlaces, código, tablas, HTML, front matter, y LaTeX y Mermaid dentro de fórmulas y diagramas. Hay paletas para los modos claro, oscuro y Aumentar contraste.
- **Los bloques de código delimitados** se resaltan para Bash, C, C++, C#, CSS (también SCSS y Less), Go, HTML, XML, Java, JavaScript, JSON, Kotlin, PHP, Python, Ruby, Rust, SQL, Swift, TypeScript, YAML, TeX y Mermaid, incluidos sus nombres abreviados habituales como `js`, `py` o `sh`.
- **Sin sustituciones sorpresa.** Las comillas tipográficas, los guiones inteligentes y la sustitución de texto están desactivados por omisión, porque estropean `--`, `---` y `"`. Actívalos en **Edición › Sustituciones**.
- **Las herramientas de escritura del sistema.** Corrección ortográfica y gramatical, dictado, Buscar y reemplazar (⌥⌘F) y Habla.
- **Arrastrar y soltar.** Suelta un archivo de imagen para insertar `![nombre](ruta/relativa.png)`, cualquier otro archivo para insertar un enlace a él, y una URL para insertar `[url](url)`. Cada acción de soltar es un solo paso de deshacer.
- **Pegar imágenes.** Una imagen del portapapeles, como una captura de pantalla, se guarda como archivo PNG junto al documento (`Imagen 2026-10-29 13.09.07.png`) y se inserta como imagen. Para ello, el documento debe estar guardado al menos una vez.

### En la previsualización

La previsualización se comporta como un procesador de textos, pero todo lo que haces en ella acaba como Markdown:

- **Escribir.** Los caracteres que significan algo en Markdown, como `*` o `#`, se escapan para que aparezcan tal como los escribes.
- **Return** empieza un párrafo o un elemento de lista nuevo. **⌥Return** o **⌃Return** inserta un salto de línea.
- **Formato en el cursor.** Con el cursor al final de una palabra, Negrita, Cursiva y los demás formatos de carácter se aplican a lo que escribas a continuación. Vuelve a elegir el formato para detenerlo. Dentro de una palabra, se da formato a la palabra entera.
- **Líneas vacías.** Return en una línea vacía añade otra línea vacía, y Retroceso la quita. En el código fuente, cada línea vacía adicional son dos líneas en blanco más; otros programas de Markdown las muestran como un espacio normal entre párrafos.
- **Deshacer** funciona en ambos paneles. Las acciones de deshacer llevan nombre («Deshacer Negrita»), escribir es un paso por ráfaga y, tras Deshacer y Rehacer, ambos paneles se desplazan hasta el cambio.
- **Las fórmulas y los diagramas** aparecen como gráficos terminados. Haz clic en uno, sitúa el cursor sobre él o pulsa Return sobre él para abrir su editor (consulta [Fórmulas](#fórmulas) y [Diagramas](#diagramas)).
- **El HTML incrustado, las notas incrustadas y el front matter** se muestran, pero se editan en el código fuente. Un clic en ellos coloca el cursor en el lugar correspondiente del código fuente.

Todo lo que Markdown no puede expresar está bloqueado en la previsualización: los paneles Tipos de letra y Colores, la regla, y pegar o soltar texto enriquecido e imágenes. El HTML y el RTF pegados llegan como texto sin formato, salvo que elijas [Pegar como Markdown](#pegar-texto-con-formato).

### Comandos de formato

Todos los comandos están disponibles en la barra de herramientas, en la barra de menús y en el menú contextual de ambos paneles. Las opciones de menú muestran una marca para el formato que hay en el cursor.

| Menú | Comandos |
|---|---|
| **Formato › Tipo de letra** | Negrita, Cursiva, Subrayado, Tachado, Resaltado, Código, Superíndice, Subíndice |
| **Formato › Encabezamiento** | Cuerpo de texto, Encabezamiento 1 a 6 |
| **Formato › Lista** | Lista con viñetas, Lista numerada, Lista de tareas |
| **Formato** | Cita, Bloque de código (Sin lenguaje o uno de 20 lenguajes; también cambia el lenguaje de un bloque existente), Recuadro (Nota, Consejo, Importante, Advertencia, Precaución; el mismo tipo otra vez lo quita), Editar fórmula o diagrama…, Borrar formato |
| **Insertar** | Enlace…, Imagen…, Tabla…, Fórmula, Diagrama (Diagrama de flujo, Diagrama de secuencia, Diagrama de Gantt), Tabla de contenido, Nota al pie, Separador |
| **Tabla** | Insertar fila encima/debajo, Insertar columna a la izquierda/derecha, Eliminar fila, Eliminar columna, Alineación (Izquierda, Centrado, Derecha, Por omisión), Alinear columnas de la tabla |
| **Visualización** | Ocultar/mostrar previsualización, Mostrar/ocultar esquema, Puntuación tipográfica, Acercar/alejar, Tamaño real, barra de herramientas, barra de pestañas, pantalla completa |

El subrayado, el resaltado, el superíndice y el subíndice se escriben como `<ins>`, `<mark>`, `<sup>` y `<sub>`, que GitHub también muestra. Splat lee además `<u>`, que GitHub elimina, y las formas más cortas `==resaltado==`, `^sup^` y `~sub~`. GitHub muestra `==` y `^` tal cual y `~sub~` como tachado.

### Listas y tareas

- Return continúa una lista; Return en un elemento vacío la termina.
- **Tab** y **⇧Tab** aumentan y reducen la sangría de los elementos de lista en la previsualización.
- Un clic en la casilla de una tarea en la previsualización alterna `- [ ]` y `- [x]`.
- **Formato › Lista** convierte los párrafos seleccionados en una lista o cambia su tipo.

### Tablas

- **Insertar › Tabla…** pide el número de filas (incluida la de encabezado) y de columnas.
- En la previsualización, **Tab** pasa a la celda siguiente, **⇧Tab** a la anterior y **Return** a la celda de abajo. Tab en la última celda añade una fila.
- El menú **Tabla** inserta y elimina filas y columnas y define la alineación de una columna (⌘{, ⌘|, ⌘}).
- **Tabla › Alinear columnas de la tabla** rellena el código fuente para que las columnas queden alineadas.

### Pegar texto con formato

**Edición › Pegar como Markdown** (⌃⌘V), también en el menú contextual de ambos paneles, convierte lo que hay en el portapapeles en Markdown en la selección, como un solo paso de deshacer. ⌘V sigue insertando texto sin formato.

- **Desde un navegador, Word, Google Docs, Confluence, Notas o Pages:** se conservan los encabezamientos, la negrita, la cursiva, el subrayado, el tachado, el superíndice y el subíndice, los enlaces, las listas con su anidamiento, las citas, el código y las tablas.
- **Desde una hoja de cálculo:** las celdas se convierten en una tabla con fila de encabezado y columnas de números alineadas a la derecha. Lo mismo ocurre con el texto separado por tabulaciones, punto y coma o comas, si todas las líneas tienen el mismo número de columnas.
- **Las imágenes** con una dirección web siguen siendo enlaces. Una imagen dentro del HTML pegado se convierte en un archivo junto al documento, que para ello debe haberse guardado una vez. Los archivos de imagen temporales de Word no se incorporan, y su texto alternativo se conserva.
- **Los enlaces** conservan solo las direcciones `http`, `https`, `mailto`, relativas y `#`; de cualquier otro enlace queda el texto.

### Metadatos

**Formato › Editar metadatos…**, o un clic en la línea de metadatos de la parte superior de la previsualización, abre un formulario con los campos del front matter YAML. Los campos se pueden cambiar, añadir y eliminar. Un campo con una lista la mantiene como lista: sus valores van uno tras otro separados por comas, y un valor que contiene una coma va entre comillas. Un campo nuevo llamado `tags`, `aliases`, `keywords`, `categories`, `authors` o `cssclasses` también se convierte en lista. Los campos anidados y el texto de varias líneas se ven allí y se editan en el código fuente.

Lo que cambia el formulario es un solo paso de deshacer y solo afecta a las líneas de los campos que has cambiado: los comentarios, el orden, los espacios inusuales, las comillas y los finales de línea de todos los demás siguen idénticos byte a byte. Un documento sin metadatos recibe un bloque al principio.

## Markdown compatible

Splat analiza el texto con [comrak](https://github.com/kivikakk/comrak), un parser compatible con CommonMark y GFM, y activa la mayoría de sus extensiones. Todo lo que sigue se renderiza en la previsualización y en todas las exportaciones.

### CommonMark y GitHub Flavored Markdown

- Encabezamientos (`#` y subrayados), párrafos, saltos de línea, énfasis, énfasis fuerte, código en línea, citas en bloque, separadores.
- Listas con viñetas, numeradas y anidadas; listas de tareas.
- Enlaces (en línea, de referencia y automáticos), imágenes, definiciones de referencia de enlaces.
- Bloques de código delimitados y con sangría, con colores de sintaxis.
- Tablas con alineación de columnas.
- Tachado con `~~dos tildes~~`.
- Notas al pie: `texto[^1]` con `[^1]: La nota.` en cualquier lugar del documento. Un clic en la marca salta a la nota.
- Front matter YAML entre líneas `---` al principio. La previsualización lo resume en una línea, por ejemplo «Metadatos: title, date, tags».

### Recuadros

Las alertas de GitHub se renderizan como cuadros de colores con icono y título:

```markdown
> [!NOTE]
> Información útil que los usuarios deberían conocer.

> [!WARNING]
> Contenido crítico que exige atención inmediata.
```

Los cinco tipos son Nota, Consejo, Importante, Advertencia y Precaución. **Formato › Recuadro** convierte en uno el párrafo en el que está el cursor.

Los recuadros de Obsidian también funcionan, con cualquier título y un estado de plegado:

```markdown
> [!tip]- Haz clic en ▸ para desplegar
> Oculto hasta que se abre.

> [!question]+ Abierto por omisión
> Un `+` empieza desplegado, un `-` contraído.
```

Los demás tipos de Obsidian se muestran con el más cercano de los cinco colores: `success`, `check`, `done` y `hint` como Consejo; `question`, `help` y `faq` como Importante; `attention` como Advertencia; `failure`, `fail`, `missing`, `danger`, `error` y `bug` como Precaución; todo lo demás como Nota.

### Sintaxis ampliada

| Función | Sintaxis | Procede de |
|---|---|---|
| Resaltado | `==texto marcado==` | Obsidian, Typora |
| Superíndice | `x^2^` | Pandoc, Typora |
| Subíndice | `H~2~O` | Pandoc, Typora |
| Shortcode de emoji | `:rocket:` se convierte en 🚀 | GitHub |
| Spoiler | `\|\|texto oculto\|\|`, selecciónalo para leerlo | Discord |
| Nota al pie en línea | `texto^[la propia nota]` | Pandoc, Obsidian |
| Lista de definiciones | `Término`, línea en blanco, `: Explicación` | PHP Markdown Extra, Pandoc |
| Ancla de encabezamiento | `## Instalación {#setup}`, enlaza con `[ver](#setup)` | Pandoc, kramdown |

Las listas de definiciones tienen este aspecto en el código fuente:

```markdown
Término

: Explicación del término.
```

Sin un ancla propia, cada encabezamiento recibe una derivada de su texto, como en GitHub: `## Primeros pasos` se alcanza como `#primeros-pasos`.

### Obsidian

- **Enlaces wiki.** `[[Nota]]` enlaza con `Nota.md` en la misma carpeta o en una carpeta de más abajo. `[[Nota|Texto mostrado]]` muestra tu propio texto, y `[[Nota#Encabezamiento]]` abre la nota. ⌘-clic la abre en Splat.
- **Incrustaciones.** `![[Nota]]` muestra la otra nota dentro de esta; las notas incrustadas en ella aparecen como enlaces. `![[diagrama.png]]` muestra una imagen. Como en Obsidian, las incrustaciones nombran notas e imágenes de la carpeta del documento o de más abajo, nunca con una ruta absoluta, y solo los archivos Markdown se muestran como notas.
- **Comentarios.** `%%Solo en el código fuente%%` sigue visible en el código fuente y se omite en la previsualización y en todas las exportaciones, tanto en línea como en líneas propias.
- **Etiquetas.** `#idea` o `#proyecto/splat` se resalta. ⌘-clic busca la etiqueta en el documento y, en la barra lateral, en toda la carpeta.
- **Recuadros plegables.** Consulta [Recuadros](#recuadros).

### Citas y referencias

- **Citas de Pandoc.** `[@doe2020]` o `[@doe2020; @roe2021]` se lee como «(Doe 2020; Roe 2021)». Las entradas proceden del archivo BibTeX que el front matter nombra como `bibliography: refs.bib`. Sin él, Splat usa `references.bib`, `literatur.bib` o `bibliography.bib` junto al documento.
- **Referencias de GitHub y GitLab.** Si el documento está en un repositorio Git cuyo remoto `origin` está en github.com o en un host de GitLab, se resaltan `@nombre`, `#12` (incidencia) y `!7` (merge request, solo en GitLab). ⌘-clic las abre en el navegador.

### HTML incrustado

El HTML tal como aparece en los README de GitHub se renderiza, con un lector propio en lugar de un motor de navegador:

- párrafos y bloques centrados (`<p align="center">`, `<div align="center">`)
- imágenes con tamaño (`<img src="logo.png" width="120">`)
- enlaces, `<kbd>⌘</kbd>`, `<br>`, `<u>`, `<ins>`, `<mark>`, `<sup>`, `<sub>`
- listas y tablas, incluidas las celdas que abarcan varias columnas (`colspan`)
- secciones plegables: `<details><summary>Más</summary>…</details>`, que se abren con un clic en ▸

El Markdown entre etiquetas HTML se renderiza como Markdown. Los scripts, los estilos, los marcos y los formularios se omiten por completo, y los enlaces y las imágenes conservan solo las direcciones `http`, `https`, `mailto` y relativas.

### Puntuación tipográfica

**Visualización › Puntuación tipográfica** muestra las comillas rectas como las comillas del idioma de tu interfaz, `--` como raya corta, `---` como raya larga y `...` como puntos suspensivos. En alemán, `"Ja"` se convierte en „Ja“, en francés en «Oui», en japonés en 「はい」, también donde el chino y el japonés ponen las comillas sin espacio; las abreviaturas hebreas como `צה"ל` reciben sus guershayim, צה״ל. El código fuente conserva los caracteres sin formato; la previsualización y todas las exportaciones muestran los tipográficos. El ajuste está desactivado hasta que lo actives.

### Resumen de sintaxis

| Elemento | Markdown |
|---|---|
| **Negrita** | `**Negrita**` |
| *Cursiva* | `*Cursiva*` |
| Subrayado | `<ins>Subrayado</ins>` o `<u>Subrayado</u>` |
| ~~Tachado~~ | `~~Tachado~~` |
| Resaltado | `<mark>Resaltado</mark>` o `==Resaltado==` |
| `Código` | `` `Código` `` |
| x² | `x<sup>2</sup>` o `x^2^` |
| H₂O | `H<sub>2</sub>O` o `H~2~O` |
| [Enlace](https://example.com) | `[Enlace](https://example.com)` |
| Encabezamiento | `# Encabezamiento` a `###### Encabezamiento` |
| Cita | `> Cita` |
| Lista con viñetas | `- Elemento` |
| Lista numerada | `1. Elemento` |
| Tarea | `- [ ] Pendiente` y `- [x] Hecha` |
| Bloque de código | ```` ```rust ```` … ```` ``` ```` |
| Tabla | `\| A \| B \|` con `\|---\|---\|` debajo del encabezado |
| Nota al pie | `Texto[^1]` y `[^1]: Nota` |
| Separador | `---` |
| Imagen | `![Texto alternativo](imagen.png)` |
| Fórmula | `$E = mc^2$`, `$$ … $$` |
| Diagrama | ```` ```mermaid ````, ```` ```dot ````, ```` ```plantuml ```` |
| Recuadro | `> [!TIP]` |
| Enlace wiki | `[[Nota]]`, `[[Nota\|Texto]]` |
| Incrustación | `![[Nota]]`, `![[imagen.png]]` |
| Comentario | `%%Comentario%%` |
| Etiqueta | `#etiqueta` |
| Cita bibliográfica | `[@clave]` |
| Spoiler | `\|\|Spoiler\|\|` |
| Emoji | `:tada:` |

## Fórmulas

Las fórmulas las renderiza [MathJax](https://www.mathjax.org) con su matemática TeX y AMS, como nítidos gráficos vectoriales. El texto de `\text{…}` puede estar en cualquier escritura: lo que les falta a las fuentes de MathJax sale de las fuentes de tu Mac, con el árabe y el hebreo escritos de derecha a izquierda, y los emoji en color.

| Tipo | Sintaxis |
|---|---|
| En línea | `$E = mc^2$`, `` $`E = mc^2`$ `` (GitHub, GitLab), `\(E = mc^2\)` |
| En bloque | `$$` … `$$`, `\[` … `\]`, o un bloque de código marcado como `math` |

**Editar una fórmula.** Haz clic en una fórmula en la previsualización, haz doble clic, pulsa Return sobre ella o sitúa el cursor sobre ella para abrir su editor en un popover. **Insertar › Fórmula** inserta una nueva y abre el editor de inmediato.

- **Visual o LaTeX.** El conmutador de arriba alterna entre un editor visual ([MathLive](https://cortexjs.io/mathlive/)) y el código LaTeX. Splat recuerda tu elección.
- **Bloques** para fracción, raíz cuadrada, superíndice y subíndice, suma e integral con límites, límite, paréntesis que crecen con el contenido, matriz 2×2, letras griegas, y relaciones, operadores y flechas. Dejan marcadores de posición □, y **Tab** salta al siguiente.
- **Previsualización en directo.** Los cambios aparecen en el documento mientras escribes. Los errores de MathJax se muestran en una línea debajo del código.
- **Confirmar o descartar.** Hacer clic fuera conserva el cambio como un solo paso de deshacer; **Esc** o **Cancelar** lo descarta. Pulsar Esc en una fórmula nueva y vacía la elimina de nuevo.
- El menú contextual ofrece **Editar fórmula…** para la fórmula que hay bajo el puntero.

VoiceOver lee una fórmula como su código LaTeX. Splat renderiza fórmulas, no documentos LaTeX completos: no hay paquetes, ni TikZ, ni `\documentclass`.

## Diagramas

Los diagramas se renderizan dentro de Splat, sin conexión y en una variante clara y otra oscura.

| Motor | Bloque de código | Qué dibuja |
|---|---|---|
| [Mermaid](https://mermaid.js.org) | ```` ```mermaid ```` | diagramas de flujo, de secuencia, de clases, de estados, entidad-relación, de Gantt, de sectores, mapas mentales, líneas de tiempo y todos los demás tipos de Mermaid |
| [Graphviz](https://graphviz.org) | ```` ```dot ```` o ```` ```graphviz ```` | grafos en el lenguaje DOT |
| [PlantUML](https://plantuml.com) | ```` ```plantuml ```` o ```` ```puml ```` | diagramas UML; funciona sin Java |

```mermaid
flowchart LR
    A[Markdown] --> B[Previsualización]
    B --> C[PDF]
```

**Editar un diagrama** funciona como editar una fórmula: haz clic en él para abrir el código en un popover, con colores de sintaxis y mensajes de error en una línea debajo del código. **Insertar › Diagrama** parte de una plantilla de diagrama de flujo, de secuencia o de Gantt. Los bloques ayudan con los tres tipos más habituales de Mermaid:

- **Diagrama de flujo:** Nodo ▾ (rectángulo, redondeado, círculo, rombo, base de datos), Conexión, Conexión con etiqueta, Grupo, Dirección ▾
- **Diagrama de secuencia:** Participante, Mensaje, Respuesta, Nota, Bucle, Rutas alternativas
- **Diagrama de Gantt:** Sección, Tarea, Hito

Los demás tipos de diagrama se editan como código. VoiceOver lee el título o el texto alternativo de un diagrama.

El primer diagrama tras el arranque tarda cerca de un segundo, porque el motor de renderizado se inicia en segundo plano. Un diagrama que tarda más de 10 segundos muestra un error, y el motor de renderizado se reinicia para el siguiente. El motor de PlantUML (3 MB) se carga solo con el primer diagrama de PlantUML. Un `!include` de PlantUML desde la web está bloqueado.

## Imágenes

- **Las imágenes locales** usan rutas relativas al documento: `![Logotipo](images/logo.png)`. Splat muestra PNG, JPEG, GIF, WebP, BMP, SVG, HEIC, TIFF, AVIF y PDF.
- **Las imágenes de la web** siguen bloqueadas hasta que las permites: una barra sobre la previsualización explica que cargarlas informa a sus servidores de que abres el documento y cuándo. **Cargar imágenes** las permite para este documento; Splat recuerda la elección por archivo.
- **Tamaño.** `<img width="200">` da tamaño a una imagen; si no, las imágenes se ajustan al ancho de la previsualización.
- **Cambios en el disco.** Una imagen que sustituyes, o una que aparece después de que el documento la nombrara, se muestra sin volver a abrir el documento. Las imágenes de la web que fallaron por la red se reintentan mientras un documento las muestra.
- **Memoria.** Las imágenes se muestran con hasta 2560 píxeles en su lado más largo, lo que también llena una página A4. Se decodifican más pequeñas cuando ocupan una octava parte de la memoria del Mac, y mientras macOS indica presión de memoria.
- **Modo oscuro.** Las imágenes con fondo blanco se atenúan ligeramente en la previsualización oscura.
- **Pegar y soltar.** Consulta [En el código fuente](#en-el-código-fuente).

## Enlaces y navegación

En la previsualización, un clic coloca el cursor para que puedas editar el texto del enlace; **⌘-clic** sigue el enlace:

- `http`, `https` y `mailto` se abren en tu navegador o en tu app de correo.
- Los archivos Markdown se abren en Splat; los demás archivos y carpetas se muestran en el Finder.
- `#ancla` salta al encabezamiento del documento.
- Las direcciones que abren otra app, como `slack://` o `vscode://`, preguntan antes y muestran la dirección completa y la app. **Cancelar** está preseleccionado.

**Insertar › Tabla de contenido** inserta una lista con enlaces de todos los encabezamientos. Con el cursor dentro de ella, el mismo comando la actualiza. La barra lateral de esquema (⌃⌘S) salta a cualquier encabezamiento en ambos paneles.

## Notas en una carpeta

Los botones de la parte superior de la barra lateral alternan entre el esquema, los archivos y una búsqueda.

- **Archivos** (**Visualización › Mostrar archivos**, ⌃⌘E). Todos los archivos Markdown de la carpeta del documento o, si este está en un repositorio Git, de todo el repositorio. Un clic o Return abre una nota como pestaña de esta ventana; si ya está abierta, pasa al frente. El botón junto al nombre de la carpeta muestra otra carpeta en esta ventana. Los archivos ocultos, `node_modules`, `target`, `Pods`, `DerivedData`, `__pycache__` y las carpetas sin notas quedan fuera. Un documento situado directamente en tu carpeta de inicio muestra solo las notas que tiene al lado, para que Splat no recorra todas tus carpetas pidiendo permiso para cada una.
- **Búsqueda** (**Edición › Buscar › Buscar en todas las notas…**, ⇧⌘F). Encuentra texto en todas las notas, sea cual sea su uso de mayúsculas y la codificación de sus acentos. `#tag` encuentra una etiqueta, `#project` también `#project/splat`, y nada dentro de código. Un clic en un resultado abre la nota en ese lugar.
- **Enlaces a una nota.** El menú contextual de una nota lista las notas que enlazan con ella, mediante enlace wiki, incrustación, enlace relativo o definición de referencia de enlace.
- **Renombrar una nota.** El mismo menú la renombra y cambia esos enlaces. Splat dice primero cuántos enlaces en cuántos archivos supone. En cada enlace solo cambia el nombre, y todos los demás bytes del archivo se mantienen. Los documentos abiertos reciben el cambio como un paso de deshacer; en otros archivos solo Git o Versiones pueden revertirlo. Un archivo que otro programa haya cambiado entretanto detiene el cambio de nombre de todos los enlaces, y Splat lo nombra.

La lista sigue a la carpeta cuando la ventana pasa al frente. La búsqueda ve los archivos tal como están en el disco, y el texto que aún no has guardado cuenta a partir del siguiente Autoguardado.

## Cambios desde el último commit

En un documento de un repositorio Git, unas barras junto al texto marcan lo que difiere del último commit, en el código fuente y en la previsualización: verde para las líneas añadidas, azul para las líneas cambiadas y una marca roja corta donde se ha eliminado algo. Se actualizan mientras escribes y cuando la rama se mueve. **Visualización › Cambios desde el último commit** las desactiva, para todas las ventanas.

Las líneas que solo difieren en su final de línea no cuentan, así que un archivo con CRLF frente a un commit con LF no muestra ningún cambio. Un archivo nuevo no tiene marcas hasta su primer commit, ni tampoco un documento de más de 32 MB.

## Exportar, imprimir y compartir

| Formato | Cómo | Qué obtienes |
|---|---|---|
| **PDF** | **Archivo › Exportar como PDF…** o el menú Compartir | Páginas del tamaño del papel, siempre en la apariencia clara, con fórmulas y diagramas como gráficos vectoriales. **Archivo › Ajustar página…** define el papel. |
| **Imprimir** | **Archivo › Imprimir…** (⌘P) | La misma maquetación que el PDF. |
| **HTML** | **Archivo › Exportar como HTML…** o el menú Compartir | Un archivo autocontenido: imágenes locales incrustadas, fórmulas y diagramas como imágenes nítidas, y anclas de encabezamientos para que funcione la tabla de contenido. Sin scripts. |
| **Word** | **Archivo › Exportar como documento de Word…** o el menú Compartir | Un `.docx` con los estilos de encabezamiento de Word, listas y tablas reales (incluidas las celdas combinadas), notas al pie como notas al pie de Word, imágenes incrustadas, fórmulas y diagramas. |
| **Markdown** | El botón **Compartir** de la barra de herramientas | El documento como archivo `.md` para Mail, Mensajes, AirDrop y cualquier otro servicio de compartir. |

El menú **Archivo › Compartir** ofrece todo esto en un solo lugar. Las exportaciones respetan el ajuste de puntuación tipográfica y las imágenes de la web que hayas permitido; en HTML, las imágenes de la web siguen siendo enlaces.

**Archivo › Opciones de exportación…** define lo que el PDF, la impresión y Word añaden al texto, para todos los documentos:

- **Números de página** en el pie de página de PDF e impresión, como `2 / 9`.
- **Cabecera y pie de página** para PDF e impresión. `{title}`, `{author}`, `{date}`, `{page}` y `{pages}` representan sus valores, los tres primeros tomados de los metadatos del documento.
- **Una portada** para PDF, impresión y Word, con `title`, `subtitle`, `author` y `date` de los metadatos, o el nombre del documento como título. No tiene cabecera ni pie de página.
- **Una plantilla de Word**, un `.docx` o `.dotx` cuyo aspecto adopta la exportación: sus estilos, también con nombres localizados, su tema, el tamaño de página, los márgenes, las cabeceras y los pies de página. Una plantilla que no se puede leer detiene la exportación y lo indica, en lugar de darte un documento sin ella.

## Importar documentos de Word

**Archivo › Importar documento de Word…** pide un archivo `.docx` o `.dotx` y dónde guardar su Markdown, y después abre el resultado. Las imágenes van a una carpeta a su lado, con el nombre del documento más `-images`.

Los encabezamientos y las listas llegan tal como los definen los estilos y la numeración de Word, también en documentos cuyos estilos tienen nombres localizados. Se conservan la negrita, la cursiva, el subrayado, el tachado, el resaltado, el superíndice y el subíndice, los enlaces, las citas, el código, las notas al pie, las notas al final y las tablas, con las celdas que abarcan columnas como tabla HTML. Las fórmulas se convierten en LaTeX. Los cambios registrados cuentan como aceptados, y una tabla de contenido queda fuera, ya que **Insertar › Tabla de contenido** crea una nueva. Los archivos `.doc` antiguos y los documentos protegidos con contraseña se rechazan.

## Apariencia y accesibilidad

- **El modo oscuro** sigue al sistema. Las fórmulas y los diagramas se renderizan para cada apariencia.
- **Aumentar contraste** en Ajustes del Sistema cambia los colores de sintaxis a paletas de alto contraste.
- **Zoom.** ⌘+, ⌘− y ⌘0 escalan ambos paneles.
- **Pantalla completa, pestañas y una barra de herramientas personalizable**, como en cualquier app del Mac.
- **VoiceOver.** Los paneles se llaman «Código fuente Markdown» y «Previsualización», las fórmulas se leen como su código y los diagramas como su título. Todos los comandos son accesibles con el teclado.

Splat no tiene ventana de ajustes: las pocas opciones están donde se aplican, en **Visualización**, **Edición › Sustituciones** y **Archivo › Opciones de exportación…**.

## Idiomas

La interfaz, los menús y la referencia rápida de Markdown están traducidos a 35 idiomas:

alemán, árabe, catalán, checo, chino (simplificado), chino (tradicional), coreano, croata, danés, eslovaco, esloveno, español, finés, francés, griego, hebreo, hindi, húngaro, indonesio, inglés, italiano, japonés, malayo, neerlandés, noruego, polaco, portugués (Brasil), portugués (Portugal), rumano, ruso, sueco, tailandés, turco, ucraniano y vietnamita.

Splat sigue el orden de idiomas de **Ajustes del Sistema › General › Idioma y región**; allí también puedes elegir un idioma solo para Splat en **Aplicaciones**. Si no hay traducción para tu idioma, usa el inglés. Los términos de los menús siguen las traducciones de Apple, así que Splat se lee como el resto de tu Mac.

El texto en hebreo y en árabe se lee de derecha a izquierda dondequiera que aparezca, sea cual sea el idioma de la interfaz: cada párrafo, encabezamiento y elemento de lista toma la dirección de su primera letra, las citas y los recuadros dibujan su barra a la derecha, y una tabla cuyo texto empieza de derecha a izquierda coloca sus columnas desde la derecha. Los bloques de código siguen de izquierda a derecha. Las exportaciones a PDF, HTML y Word conservan estas direcciones. Con el hebreo o el árabe como idioma del sistema, macOS también refleja la ventana, de modo que la previsualización queda a la izquierda.

## Atajos de teclado

| Acción | Atajo |
|---|---|
| Negrita, Cursiva, Subrayado | ⌘B, ⌘I, ⌘U |
| Tachado | ⇧⌘X |
| Enlace… | ⌘K |
| Cuerpo de texto, Encabezamiento 1 a 6 | ⌥⌘0, ⌥⌘1 a ⌥⌘6 |
| Alinear la columna de la tabla a la izquierda, al centro, a la derecha | ⌘{, ⌘\|, ⌘} |
| Ocultar o mostrar la previsualización | ⌥⌘P |
| Mostrar u ocultar el esquema | ⌃⌘S |
| Mostrar u ocultar los archivos | ⌃⌘E |
| Buscar en todas las notas | ⇧⌘F |
| Pegar como Markdown | ⌃⌘V |
| Acercar, alejar, tamaño real | ⌘+, ⌘−, ⌘0 |
| Nuevo documento, abrir, guardar, duplicar | ⌘N, ⌘O, ⌘S, ⇧⌘S |
| Ajustar página, imprimir | ⇧⌘P, ⌘P |
| Buscar, buscar y reemplazar | ⌘F, ⌥⌘F |
| Buscar siguiente, anterior; usar la selección; ir a la selección | ⌘G, ⇧⌘G; ⌘E; ⌘J |
| Mostrar ortografía y gramática; comprobar documento | ⌘:, ⌘; |
| Pegar con el mismo estilo | ⌥⇧⌘V |
| Ocultar barra de herramientas, mostrar todas las pestañas, pantalla completa | ⌥⌘T, ⇧⌘\\, ⌃⌘F |
| Salto de línea (previsualización) | ⌥Return o ⌃Return |
| Celda siguiente / anterior en una tabla (previsualización) | Tab / ⇧Tab |
| Aumentar / reducir la sangría de un elemento de lista (previsualización) | Tab / ⇧Tab |
| Marcador de posición siguiente en el editor de fórmulas | Tab |
| Cerrar el editor de fórmulas o de diagramas sin cambios | Esc |
| Seguir un enlace, una etiqueta o una referencia (previsualización) | ⌘-clic |

Splat deja los atajos estándar de Apple donde están.

## Privacidad y seguridad

Un archivo Markdown puede venir de cualquiera, así que Splat trata todo documento como no fiable.

- **Sin red por omisión.** Splat no tiene telemetría. Busca actualizaciones solo cuando lo aceptas o eliges **Buscar actualizaciones…**: entonces carga un pequeño archivo de feed desde GitHub, y la solicitud no nombra más que la app y su versión. Las fórmulas y los diagramas se renderizan en local; las vistas web que lo hacen no pueden acceder a la red. **Ayuda › Informar de un problema…** abre un formulario de GitHub en tu navegador; su dirección lleva la versión de Splat, la de macOS y el tipo de Mac, nada de tus documentos.
- **Las imágenes de la web** se cargan solo con tu permiso, solo por `https`, nunca desde hosts de tu red local, de 10 MB como máximo cada una y en una sesión privada sin cookies.
- **Decodificación aislada.** Las imágenes y los gráficos de fórmulas y diagramas se decodifican en procesos auxiliares que cierran todos los archivos heredados y se encierran en un sandbox antes de leer el primer byte: sin acceso a archivos, sin red. Solo devuelven píxeles, cuyo tamaño se comprueba, y se detienen tras 10 segundos. Los decodificadores de imagen propios de Apple solo ven HEIC, TIFF, AVIF y PDF, y solo cuando los datos son realmente de ese formato; se ejecutan en un auxiliar propio, que además puede leer las carpetas de tipos de letra del sistema y usar el decodificador de vídeo del sistema. Los SVG, las fórmulas y los diagramas pasan también por ese auxiliar, así que en Splat son imágenes con una resolución fija: cuatro píxeles por punto, hasta 2560 en el lado más largo.
- **Los archivos locales** llegan a un documento solo por la ruta que este nombra. Las incrustaciones muestran notas Markdown de la carpeta del documento y de más abajo, y una imagen que falta se busca solo allí. Una ruta de imagen en otro lugar, absoluta o que empieza por `~`, se sigue cargando en la vista previa; las exportaciones HTML y Word, la impresión y los PDF solo incluyen lo que está en la carpeta del documento o en su repositorio Git, salvo que ese repositorio también contenga tu carpeta de inicio, para que un documento ajeno no pueda arrastrar tus archivos a una exportación que compartas.
- **Las carpetas de notas** solo se leen mientras la barra lateral las muestra, y renombrar una nota solo cambia otros archivos después de que confirmes cuántos.
- **Los repositorios Git** los lee el propio Splat. Nunca ejecuta el programa `git`, cuya configuración en un repositorio ajeno puede iniciar otros programas, y no lee más que la versión del documento abierto en el último commit. No escribe nada allí y no usa ninguna red.
- **Los documentos de Word y el texto pegado** los leen los propios parsers de Splat, con límites de tamaño, de anidamiento y de cuánto puede descomprimirse un archivo. Sus enlaces conservan solo las direcciones que Splat permite en otros lugares, y los nombres de las imágenes de un archivo de Word nunca deciden dónde se escriben.
- **Las copias del texto sin guardar** se quedan en este Mac, en tu carpeta Library, para la recuperación tras un cierre inesperado. Desaparecen al guardar o al terminar Splat con normalidad; las copias que dejó un cierre inesperado permanecen en la carpeta de recuperación hasta que las borres.
- **Los enlaces** nunca inician un programa con un clic. Los enlaces web y de correo se abren como siempre; los enlaces a otras apps preguntan antes.
- **El HTML de los documentos** lo lee el propio parser de Splat. Los scripts, los estilos, los marcos y los formularios nunca se ejecutan.
- **Las exportaciones** no llevan scripts. Las páginas HTML conservan solo una lista fija de etiquetas y atributos inofensivos, como hace GitHub, más una política de seguridad de contenido. Los archivos de Word enlazan solo direcciones `http`, `https` y `mailto`.
- **Firma de código** con Hardened Runtime. Splat se ejecuta sin App Sandbox para poder mostrar imágenes junto a tus documentos sin pedir permiso para cada carpeta; el aislamiento de toda la decodificación lo compensa.

## Documentos grandes

Splat sigue respondiendo con documentos de varios megabytes:

- El código fuente usa TextKit 2, que maqueta solo la parte visible.
- Mientras escribes en la previsualización, y en el código fuente de documentos de hasta 2 MB, solo se vuelven a analizar, colorear y maquetar los párrafos afectados. En un documento de 10 MB, analizar y colorear tarda unos 3 ms, renderizar unos 10 ms, y una pulsación completa en su previsualización unos 30 ms con el trabajo propio de la vista.
- Las previsualizaciones grandes renderizan primero la parte visible y el resto mientras la app está inactiva. Los bloques que se desplazan a la vista se renderizan antes de dibujarse.
- En el código fuente de los documentos más grandes, el análisis para la previsualización se ejecuta en segundo plano mientras escribes.
- Un documento sigue siendo texto sin formato cuando necesitaría unos 4 GB de memoria, es decir, unos 50 a 120 MB de Markdown corriente, pero solo de 10 a 20 MB si todo son tablas o encabezados: el código fuente no tiene colores y la previsualización explica por qué sigue vacía. Las exportaciones siguen funcionando.
- A partir de unos 200 KB, un documento abre su ventana antes de analizarse; con 5 MB, los colores y la previsualización llegan aproximadamente un cuarto de segundo después.
- Todas las imágenes juntas, incluidas las fórmulas y los diagramas, ocupan como máximo una cuarta parte de la memoria del Mac, y la mitad de eso mientras escasea la memoria. A partir de ahí, las demás imágenes se muestran como marcadores de posición hasta que vuelve a haber espacio.

Algunos cambios se procesan por completo, con unos 300 ms por pulsación a 10 MB: los de la definición o las referencias de una nota al pie, junto a una nota al pie en línea, justo al final de un documento junto a una nota al pie, en el front matter o en definiciones de referencia de enlaces, y todo cambio en un documento con bloques de comentarios `%%` o con una nota al pie definida dos veces o dentro de una cita o lista. La memoria crece con el archivo, hasta unos 115–160 MB para 1 MB de Markdown y 340–510 MB para 10 MB.

## Limitaciones conocidas

- **Una sola tilde significa subíndice.** `~texto~` es subíndice como en Pandoc; el tachado necesita `~~dos~~`. GitHub también tacha con una sola tilde.
- **Solo lectura en la previsualización:** el HTML incrustado y las notas incrustadas se editan en el código fuente, el front matter en su formulario o en el código fuente.
- **Anidamiento muy profundo:** el HTML anidado a más de 64 niveles, y el HTML en línea a más de 16, cede el paso a su contenido.
- **Las celdas combinadas de tablas** existen solo en tablas HTML, y solo entre columnas (`colspan`, sin `rowspan`). La sintaxis de tablas de MultiMarkdown para celdas combinadas no es compatible.
- **Las bibliografías** se leen solo de BibTeX, no de CSL JSON ni de YAML.
- **Las referencias** necesitan un repositorio en github.com o en un host de GitLab; `!7` existe solo en GitLab.
- **Puntuación tipográfica:** una comilla simple de cierre alemana no se distingue de un apóstrofo y se escribe como ’.
- **Fórmulas, no documentos:** sin paquetes de LaTeX, sin TikZ y sin documentos completos.
- **El editor visual de fórmulas** escribe LaTeX en la notación de MathLive: los espacios desaparecen (`E = mc^2` pasa a `E=mc^2`), y el resultado renderizado es el mismo.
- **Recuento de palabras:** el tailandés, el lao, el jemer y el birmano se escriben sin espacios y se cuentan por tramo de texto, no por palabra. `3.14` y `snake_case` son dos palabras cada uno.
- **Renombrar una nota** no cambia los enlaces en HTML incrustado, y un enlace wiki queda como está si el nombre nuevo contiene `#`, `|`, `^`, `[`, `]` o `\`.
- **Las marcas de cambios** leen repositorios con objetos SHA-1 y refs sueltas o empaquetadas; uno con SHA-256 o reftable no muestra ninguna. Un archivo bajo un filtro de Git como LFS se compara con lo que guardó el filtro.
- **La importación de Word** no incorpora el texto de cabeceras, pies de página y comentarios, y las imágenes en `.emz` o SVG conservan solo su texto alternativo.
- **Pegar como Markdown** lee el RTF como texto occidental, y allí solo toma los encabezamientos de los tamaños de letra. La mayoría de los programas también ofrecen HTML, que es lo que prefiere.
- **Por diseño:** sin números de línea, sin ventana de ajustes, sin Help Book y no está en la App Store.
- **Menos probada en Intel.** Las pruebas automáticas de la ventana se ejecutan solo en Apple Silicon; en Intel, la CI ejecuta `cargo test`.

## Solución de problemas

**La lista de documentos recientes está vacía tras recompilar Splat.** macOS vincula los documentos recientes y los permisos de carpeta a la firma de código de la app. Las compilaciones locales se firman con el certificado «Splat Local Signing» de tu llavero de inicio de sesión, así que las recompilaciones conservan ambos. Si se borró el certificado, la siguiente compilación crea uno nuevo y las listas empiezan de cero una vez.

**La app de la imagen de disco muestra otros documentos recientes que mi propia compilación.** Las dos llevan firmas distintas, así que macOS mantiene una lista aparte y permisos de carpeta aparte para cada una.

**El HTML u otra sintaxis se muestra como código sin formato.** Puede que haya abierto el documento una copia antigua de Splat. Ciérrala, elige **Archivo › Obtener información** en el documento desde el Finder y comprueba que **Abrir con** nombra el Splat de tu carpeta Aplicaciones.

**No hay Buscar actualizaciones… en el menú de Splat.** Solo la versión de la imagen de disco o de Homebrew se actualiza por sí sola; una compilación propia no tiene actualizador.

**Las imágenes de la web no aparecen.** Haz clic en **Cargar imágenes** en la barra sobre la previsualización. Solo se cargan las imágenes `https` de hosts públicos.

**Al pegar una captura de pantalla se me pide guardar primero.** Las imágenes pegadas se guardan como archivos junto al documento, lo que requiere un lugar en el disco. Guárdalo y vuelve a pegar.

**«El archivo no es texto UTF-8.»** El archivo está en otra codificación y no tiene una marca de orden de bytes que la indique. Conviértelo antes, por ejemplo con `iconv -f ISO-8859-1 -t UTF-8 old.md > new.md`.

**Una fórmula o un diagrama muestra un error.** Haz clic en él: el editor muestra el mensaje de MathJax, Mermaid, Graphviz o PlantUML debajo del código.

## Para desarrolladores

La compilación, las pruebas, la estructura del código y las reglas que sigue se describen en [docs/development.md](../../../docs/development.md) (en inglés).

### Componentes de terceros

| Componente | Versión | Para qué se usa | Licencia |
|---|---|---|---|
| [comrak](https://github.com/kivikakk/comrak) | 0.59 con [una línea cambiada](../../../vendor/comrak/PATCH.md) | parser de Markdown | BSD-2-Clause |
| [objc2](https://github.com/madsmtm/objc2), objc2-foundation, block2 | 0.6 / 0.3 | runtime de Objective-C, Foundation, bloques | MIT |
| objc2-app-kit, objc2-web-kit, dispatch2 | 0.3 | bindings de AppKit, WebKit y Grand Central Dispatch | Zlib, Apache-2.0 o MIT |
| [krilla](https://github.com/LaurenzV/krilla), krilla-svg, [usvg](https://github.com/linebender/resvg) | 0.8 / 0.47 | fórmulas e imágenes SVG como PDF vectorial | MIT o Apache-2.0 |
| [image](https://github.com/image-rs/image) | 0.25 | decodificación de PNG, JPEG, GIF, WebP y BMP | MIT o Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2 | alineación de columnas de tablas | MIT o Apache-2.0 |
| [entities](https://github.com/p-jackson/entities) | 1 | los nombres de las referencias de caracteres HTML | MIT |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide) | 0.9 | descomprimir archivos de Word y objetos de Git | MIT, Zlib o Apache-2.0 |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.21 | leer el XML de los archivos de Word | MIT o Apache-2.0 |
| [unicode-normalization](https://github.com/unicode-rs/unicode-normalization) | 0.1 | nombres y búsqueda con letras compuestas y descompuestas | MIT o Apache-2.0, Unicode-3.0 |
| [emojis](https://github.com/rossmacarthur/emojis) | 0.9 | mantener literal el texto tipo `:shortcode:` en el texto importado | MIT o Apache-2.0, Unicode-3.0 |
| [MathJax](https://www.mathjax.org) | 4.1.3 | renderizado de fórmulas | Apache-2.0 |
| [MathLive](https://cortexjs.io/mathlive/) | 0.111.4 | editor visual de fórmulas, con fuentes KaTeX | MIT |
| [Mermaid](https://mermaid.js.org) | 12.3.0 | diagramas de Mermaid | MIT |
| [Viz.js](https://github.com/mdaines/viz-js) | 3.24.0 | Graphviz compilado a WebAssembly, contiene Graphviz (EPL-1.0) y Expat (MIT) | MIT |
| [PlantUML](https://github.com/plantuml/plantuml) (`@plantuml/core`) | 1.2026.9 | diagramas de PlantUML, compilados a JavaScript | MIT |
| [Sparkle](https://sparkle-project.org) | 2.10.1 | actualizaciones de la versión de la imagen de disco | MIT, con partes bajo licencias BSD y zlib |

Las bibliotecas JavaScript se guardan en `resources/js/` exactamente como se publicaron; [vendor.lock](../../../vendor.lock) registra sus fuentes y sumas de verificación SHA-256. Sus textos de licencia, incluidos los de Graphviz y Expat dentro de Viz.js, están junto a ellas como `resources/js/LICENSE-*` y se incluyen en la app. Sparkle no se guarda en el repositorio: `cargo xtask dmg` descarga su release y la comprueba con la suma de verificación de xtask, y su licencia se incluye en la app como `LICENSE-sparkle`.

## Licencia

Splat se distribuye bajo cualquiera de las dos licencias siguientes:

- Licencia Apache, versión 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE))
- Licencia MIT ([LICENSE-MIT](../../../LICENSE-MIT))

a tu elección.

Salvo que declares expresamente lo contrario, toda contribución enviada intencionadamente para su inclusión en Splat por tu parte, tal como se define en la licencia Apache-2.0, tendrá doble licencia como arriba, sin términos ni condiciones adicionales.
