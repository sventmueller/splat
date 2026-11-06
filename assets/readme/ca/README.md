<p><img src="../../readme-icon.png" alt="Icona del Splat" width="168" height="168">&nbsp;&nbsp;&nbsp;<img src="screenshot.png" alt="El Splat amb Demo.md: el codi font Markdown a l’esquerra, la previsualització editable a la dreta" width="274" height="168"></p>

# Splat

**L’editor de Markdown per a macOS que et deixa escriure al codi font o directament a la previsualització.**

El Splat mostra el teu codi font Markdown amb colors de sintaxi a l’esquerra i una previsualització en directe, totalment editable, a la dreta. Escriu en qualsevol dels dos: tot el que escriguis a la previsualització esdevé un canvi mínim al codi font, de manera que les línies que no has tocat es mantenen idèntiques byte a byte i els teus diffs de Git queden nets. El Splat entén el Markdown que ja fas servir: CommonMark i GitHub Flavored Markdown, alertes de GitHub, enllaços wiki, incrustacions i requadres d’Obsidian, extensions de Pandoc i de Typora, HTML incrustat, fórmules LaTeX i diagrames de Mermaid, Graphviz i PlantUML, tot renderitzat sense connexió. Navega per tota una carpeta de notes i cerca-hi, mira què ha canviat des de l’últim commit de Git, enganxa des del Word o d’una pàgina web com a Markdown i exporta a PDF, HTML o Word amb un sol clic. Està escrit en Rust amb controls AppKit autèntics, parla 35 idiomes i no envia res per la xarxa si no li ho demanes.
 


[![Descarrega el Splat per a macOS](https://img.shields.io/badge/Download-Splat_for_macOS-0A84FF?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)

**[Descarrega Splat.dmg](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)**: signat i notaritzat per Apple, per a macOS 14 o posterior, amb Apple Silicon i Intel.

**Has trobat un error?** El Splat manté idèntiques byte a byte les línies que no has tocat. Si tot i així en canvia una, o un fitxer es mostra malament, [obre una incidència](https://github.com/sventmueller/splat/issues/new/choose) i adjunta-hi el fitxer. Si el Splat et resulta útil, una [estrella a GitHub](https://github.com/sventmueller/splat) ajuda altres persones a trobar-lo.
 
 

[![Rust edició 2024](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![macOS 14 o posterior](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)
![Universal: Apple Silicon i Intel](https://img.shields.io/badge/Universal-Apple_Silicon_%7C_Intel-333333?logo=apple&logoColor=white)
[![AppKit mitjançant objc2](https://img.shields.io/badge/AppKit-objc2_0.6-147EFB?logo=apple&logoColor=white)](https://github.com/madsmtm/objc2)
[![WebKit](https://img.shields.io/badge/WebKit-offline-0A84FF?logo=safari&logoColor=white)](https://webkit.org)
[![Comrak 0.59](https://img.shields.io/badge/comrak-0.59-DEA584?logo=markdown&logoColor=white)](https://github.com/kivikakk/comrak)
[![CommonMark i GFM](https://img.shields.io/badge/CommonMark-GFM-24292F?logo=github&logoColor=white)](https://github.github.com/gfm/)
[![MathJax 4.1.3](https://img.shields.io/badge/MathJax-4.1.3-1E6E9E?logo=latex&logoColor=white)](https://www.mathjax.org)
[![MathLive 0.111.4](https://img.shields.io/badge/MathLive-0.111.4-5A3FC0)](https://cortexjs.io/mathlive/)
[![Mermaid 12.3.0](https://img.shields.io/badge/Mermaid-12.3.0-FF3670?logo=mermaid&logoColor=white)](https://mermaid.js.org)
[![Graphviz mitjançant Viz.js 3.24](https://img.shields.io/badge/Graphviz-Viz.js_3.24-2596BE)](https://graphviz.org)
[![PlantUML 1.2026.9](https://img.shields.io/badge/PlantUML-1.2026.9-A80036)](https://plantuml.com)
![35 idiomes d’interfície](https://img.shields.io/badge/languages-35-4C9A2A)
![Sense connexió per omissió](https://img.shields.io/badge/network-offline_by_default-4C9A2A)
![Signat amb Hardened Runtime](https://img.shields.io/badge/Hardened_Runtime-signed-4C9A2A)
[![Llicència: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-4C9A2A)](#llicència)

[Bahasa Indonesia](../id/README.md) ·
[Bahasa Melayu](../ms/README.md) ·
**Català** ·
[Čeština](../cs/README.md) ·
[Dansk](../da/README.md) ·
[Deutsch](../de/README.md) ·
[English](../../../README.md) ·
[Español](../es/README.md) ·
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

## Continguts

- [Resum de les funcions](#resum-de-les-funcions)
- [Requisits](#requisits)
- [Instal·lació](#installació)
- [Primers passos](#primers-passos)
- [Escriptura](#escriptura)
  - [Al codi font](#al-codi-font)
  - [A la previsualització](#a-la-previsualització)
  - [Ordres de format](#ordres-de-format)
  - [Llistes i tasques](#llistes-i-tasques)
  - [Taules](#taules)
  - [Enganxar text amb format](#enganxar-text-amb-format)
  - [Metadades](#metadades)
- [Markdown compatible](#markdown-compatible)
  - [CommonMark i GitHub Flavored Markdown](#commonmark-i-github-flavored-markdown)
  - [Requadres](#requadres)
  - [Sintaxi estesa](#sintaxi-estesa)
  - [Obsidian](#obsidian)
  - [Cites i referències](#cites-i-referències)
  - [HTML incrustat](#html-incrustat)
  - [Puntuació tipogràfica](#puntuació-tipogràfica)
  - [Resum de la sintaxi](#resum-de-la-sintaxi)
- [Fórmules](#fórmules)
- [Diagrames](#diagrames)
- [Imatges](#imatges)
- [Enllaços i navegació](#enllaços-i-navegació)
- [Notes en una carpeta](#notes-en-una-carpeta)
- [Canvis des de l’últim commit](#canvis-des-de-lúltim-commit)
- [Exportar, imprimir i compartir](#exportar-imprimir-i-compartir)
- [Importar documents del Word](#importar-documents-del-word)
- [Aparença i accessibilitat](#aparença-i-accessibilitat)
- [Idiomes](#idiomes)
- [Dreceres de teclat](#dreceres-de-teclat)
- [Privadesa i seguretat](#privadesa-i-seguretat)
- [Documents grans](#documents-grans)
- [Limitacions conegudes](#limitacions-conegudes)
- [Resolució de problemes](#resolució-de-problemes)
- [Per a desenvolupadors](#per-a-desenvolupadors)
  - [Components de tercers](#components-de-tercers)
- [Llicència](#llicència)

## Resum de les funcions

- **Dues maneres d’escriure, un sol text.** El codi font i la previsualització són editables. El fitxer Markdown és l’única veritat: hi ha un sol historial de desfer per document, i cada edició a la previsualització es tradueix en el canvi més petit possible al codi font.
- **El Markdown que ja fas servir.** CommonMark, GitHub Flavored Markdown, alertes de GitHub, extensions d’Obsidian, de Pandoc i de Typora, codis curts d’emojis, spoilers, llistes de definicions, àncores de capçalera i HTML com als README de GitHub.
- **Fórmules i diagrames integrats.** Fórmules LaTeX amb MathJax i un editor visual de fórmules. Diagrames en Mermaid, Graphviz i PlantUML, renderitzats al teu Mac sense Java, sense servidor i sense connexió a la xarxa.
- **Un botó per a tot.** Cada construcció de Markdown té un botó a la barra d’eines i un element de menú, i les més habituals també una drecera de teclat. Les taules tenen el seu propi menú per a files, columnes i alineació.
- **Una carpeta, no només un fitxer.** La barra lateral llista les notes que envolten un document i les cerca totes. Canviar el nom d’una nota canvia els enllaços que hi apunten, i tots els bytes del voltant es mantenen.
- **Els canvis d’un cop d’ull.** En un repositori Git, unes barres al costat del text marquen què difereix de l’últim commit, als dos panells.
- **Del Word i del web a Markdown.** Enganxa text amb format com a Markdown, cel·les d’un full de càlcul com a taula, i importa documents del Word sencers amb les seves imatges.
- **Exporta sense perdre res.** PDF, un únic fitxer HTML autònom o un document de Word amb capçaleres, llistes, taules, notes a peu de pàgina i imatges de debò. Afegeix números de pàgina, capçaleres i una portada, o fes servir la plantilla del Word de la teva empresa.
- **Natiu fins al moll de l’os.** Finestres, pestanyes, menús, finestres emergents, Versions, desament automàtic, el menú de compartir, mode fosc, correcció ortogràfica, dictat i VoiceOver autèntics d’AppKit. Sense Electron, i sense vista web per al text mateix.
- **Àgil amb fitxers grans.** Renderitzar una pulsació de tecla en un document de 10 MB triga uns 10 ms, abans de la feina pròpia de la vista.
- **Privat i prudent.** Les imatges d’internet només es carreguen quan ho permets, els enllaços a altres apps demanen confirmació, i tot el que descodifica dades d’imatge no fiables s’executa en processos auxiliars aïllats en una caixa de proves.
- **En el teu idioma.** La interfície i la referència ràpida de Markdown integrada estan disponibles en 35 idiomes: àrab, català, xinès (simplificat), xinès (tradicional), croat, txec, danès, neerlandès, anglès, finès, francès, alemany, grec, hebreu, hindi, hongarès, indonesi, italià, japonès, coreà, malai, noruec, polonès, portuguès (Brasil), portuguès (Portugal), romanès, rus, eslovac, eslovè, castellà, suec, tailandès, turc, ucraïnès i vietnamita.

## Requisits

- macOS 14 Sonoma o posterior. El Splat es desenvolupa i es prova amb macOS 27, on adopta automàticament el disseny Liquid Glass.
- Un Mac amb Apple Silicon o amb processador Intel.
- Uns 45 MB d’espai al disc.

## Instal·lació

**Des de la imatge de disc.** Descarrega [`Splat.dmg`](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg), obre’l i arrossega el Splat a la carpeta Aplicacions. La imatge està signada amb un Developer ID i notaritzada per Apple, de manera que s’obre sense advertiments.

**Amb Homebrew.** El cask viu en un tap propi:

```sh
brew install --cask sventmueller/tap/splat
```

`brew upgrade --cask splat` instal·la les versions noves, i `brew uninstall --zap --cask splat` elimina també els ajustos i les memòries cau del Splat.

**Actualitzacions.** El Splat de la imatge de disc o de Homebrew s’actualitza sol amb [Sparkle](https://sparkle-project.org). A la segona obertura pregunta si pot cercar actualitzacions pel seu compte, un cop al dia. **Splat › Cerca actualitzacions…** les cerca a l’instant; **Cerca actualitzacions automàticament** i **Descarrega i instal·la actualitzacions automàticament**, al mateix menú, et permeten canviar l’elecció més endavant. El Splat només instal·la una actualització si porta la signatura del desenvolupador. Una compilació pròpia no té actualitzador.

**Des del codi font.** Mira [Compilació](../../../docs/development.md#building) per als requisits previs i després:

```sh
make build
ditto target/release/Splat.app /Applications/Splat.app
open /Applications/Splat.app
```

Per obrir fitxers Markdown amb el Splat fent doble clic, selecciona qualsevol fitxer `.md` al Finder, tria **Arxiu › Obtén informació**, escull el Splat a **Obre amb** i fes clic a **Canvia‑ho tot…**. El Splat es registra per als fitxers `.md`, `.markdown`, `.mdown` i `.mkd`.

## Primers passos

### La finestra

Cada document s’obre en una finestra pròpia, o en una pestanya si prefereixes les pestanyes:

- **Esquerra: el codi font.** El teu Markdown en SF Mono amb colors de sintaxi.
- **Dreta: la previsualització.** El document renderitzat, a punt per editar.
- **Barra lateral (opcional).** Totes les capçaleres en forma d’arbre, un clic en una la mostra als dos panells: **Mostra › Mostra l’esquema** (⌃⌘S). Dues pestanyes més contenen les [notes de la carpeta](#notes-en-una-carpeta) i una cerca entre elles.
- **Barra d’eines.** Botons agrupats segons el que canvien: caràcters, paràgrafs i elements inserits, i després previsualització activada o desactivada, Comparteix i Nou document. Personalitza-la amb **Mostra › Personalitza la barra d’eines…**.

Arrossega el separador per donar més espai a un costat o a l’altre. **Mostra › Oculta la previsualització** (⌥⌘P) deixa només el codi font; el Splat ho recorda per a cada finestra. Els dos panells es desplacen alhora, i una selecció en un es ressalta a l’altre.

La cantonada de la finestra mostra les paraules i els caràcters del text i quant es triga a llegir-lo, i els de la selecció mentre n’hi hagi una. Les adreces d’enllaços, les etiquetes HTML, les metadades, els comentaris, les fórmules i els diagrames no compten. **Mostra › Oculta el recompte de paraules** el treu.

### Documents, pestanyes i versions

El Splat és una app de documents normal de macOS:

- **Desament automàtic i Versions.** Els canvis es desen al mateix fitxer. **Arxiu › Reverteix a › Explora totes les versions…** et fa tornar enrere en el temps.
- **Canvis des d’altres programes.** Quan Git, un script o un altre editor canvia un document obert, el Splat el torna a carregar i conserva el cursor i la posició de desplaçament. Si tens canvis propis sense desar, el Splat els manté, i macOS pregunta abans de desar-los sobre l’altra versió.
- **Les ordres de fitxer habituals.** Duplica (⇧⌘S), Canvia el nom…, Trasllada a… i **Arxiu › Obre els recents**.
- **Pestanyes.** **Arxiu › Nova pestanya** o el botó **+** de la barra d’eines obre un document nou en una pestanya; el menú Finestra estàndard fusiona, trasllada i recorre les pestanyes.
- **Posició de les finestres.** Es recorden la mida i la posició de l’última finestra, també entre reinicis. Si ja hi ha una finestra en aquell lloc, la nova s’obre una mica desplaçada.
- **La icona del Dock.** Un clic en ella, després de tancar l’última finestra, obre un document nou.
- **Obre els fitxers Markdown amb el Splat.** **Splat › Defineix com a app per omissió per a Markdown** fa que un doble clic en un fitxer `.md` o `.markdown` obri el Splat; macOS demana una vegada que ho confirmis. L’element mostra una marca de selecció mentre el Splat és l’app per omissió.

Els fitxers han de ser text UTF-8, o UTF-16 amb una marca d’ordre de bytes. El Splat rebutja les altres codificacions en comptes d’endevinar-les i malmetre el text. La marca d’ordre de bytes es conserva, l’UTF-16 continua sent UTF-16, i en un fitxer on totes les línies acaben en CRLF, les línies que hi afegeixes també acaben en CRLF.

**Després d’una fallada**, com a màxim es perden els últims segons d’escriptura: cada cinc segons, el Splat copia el text sense desar a `~/Library/Application Support/Splat/Unsaved`, i elimina les còpies quan el document es desa o el Splat es tanca amb normalitat. A la propera obertura, el que una fallada hi ha deixat es trasllada a `~/Library/Application Support/Splat/Recovery` i s’avisa una vegada, amb un botó que obre la carpeta. Una fallada a la qual el Splat encara pot reaccionar hi escriu també el text de tots els documents oberts.
### Referència ràpida

**Ajuda › Referència ràpida de Markdown** obre un full de consulta en el teu idioma com a document nou sense títol. És un document normal, així que prova-ho tot allà mateix, al codi font o a la previsualització.

## Escriptura

### Al codi font

El codi font és un editor de text pla pensat per a Markdown:

- **Colors de sintaxi** com a VS Code: capçaleres, èmfasi, enllaços, codi, taules, HTML, front matter, i LaTeX i Mermaid dins de fórmules i diagrames. Hi ha paletes per a mode clar, mode fosc i Augmenta el contrast.
- **Els blocs de codi delimitats** es ressalten per a Bash, C, C++, C#, CSS (també SCSS i Less), Go, HTML, XML, Java, JavaScript, JSON, Kotlin, PHP, Python, Ruby, Rust, SQL, Swift, TypeScript, YAML, TeX i Mermaid, incloent-hi els seus noms curts habituals com `js`, `py` o `sh`.
- **Cap substitució sorpresa.** Les cometes tipogràfiques, els guions intel·ligents i la substitució de text estan desactivats per omissió, perquè trenquen `--`, `---` i `"`. Activa’ls a **Edita › Substitucions**.
- **Les eines d’escriptura del sistema.** Correcció ortogràfica i gramatical, dictat, Cerca i substitueix (⌥⌘F) i Veu.
- **Arrossega i deixa anar.** Deixa anar un fitxer d’imatge per inserir `![nom](ruta/relativa.png)`, qualsevol altre fitxer per inserir-ne un enllaç, i una URL per inserir `[url](url)`. Cada acció és un sol pas de desfer.
- **Enganxa imatges.** Una imatge del porta-retalls, com ara una captura de pantalla, es desa com a fitxer PNG al costat del document (`Imatge 2026-10-29 13.09.07.png`) i s’insereix com a imatge. Per a això cal que el document s’hagi desat una vegada.

### A la previsualització

La previsualització es comporta com un processador de textos, però tot el que hi fas acaba en Markdown:

- **Escriure.** Els caràcters que signifiquen alguna cosa en Markdown, com `*` o `#`, s’escapen perquè apareguin tal com els has escrit.
- **Retorn** comença un paràgraf o un element de llista nou. **⌥Retorn** o **⌃Retorn** insereix un salt de línia.
- **Format al cursor.** Amb el cursor al final d’una paraula, Negreta, Cursiva i els altres formats de caràcter s’apliquen al que escriguis a continuació. Torna a triar el format per aturar-lo. Dins d’una paraula, es formata tota la paraula.
- **Línies buides.** Retorn en una línia buida n’afegeix una altra, i Retrocés l’elimina. Al codi font, cada línia buida addicional són dues línies en blanc més; altres programes de Markdown les mostren com a espaiat normal entre paràgrafs.
- **Desfés** funciona als dos panells. Els elements de desfer tenen nom («Desfés negreta»), escriure és un pas per ràfega, i després de Desfés i Refés els dos panells es desplacen fins al canvi.
- **Les fórmules i els diagrames** apareixen com a gràfics acabats. Fes-hi clic, posa-hi el cursor a sobre o prem Retorn per obrir-ne l’editor (mira [Fórmules](#fórmules) i [Diagrames](#diagrames)).
- **L’HTML incrustat, les notes incrustades i el front matter** es mostren, però s’editen al codi font. Un clic en ells posa el cursor al lloc corresponent del codi font.

A la previsualització es bloqueja tot el que Markdown no pot expressar: els plafons Tipus de lletra i Color, la regla, i enganxar o deixar anar text enriquit i imatges. L’HTML i l’RTF enganxats arriben com a text pla, tret que triïs [Enganxa com a Markdown](#enganxar-text-amb-format).

### Ordres de format

Totes les ordres estan disponibles a la barra d’eines, a la barra de menús i al menú contextual dels dos panells. Els elements de menú mostren una marca de selecció per al format que hi ha al cursor.

| Menú | Ordres |
|---|---|
| **Format › Tipus de lletra** | Negreta, Cursiva, Subratllat, Ratllat, Ressaltat, Codi, Superíndex, Subíndex |
| **Format › Capçalera** | Cos de text, Capçalera de l’1 al 6 |
| **Format › Llista** | Llista amb vinyetes, Llista numerada, Llista de tasques |
| **Format** | Cita, Bloc de codi (Sense llenguatge o un de 20 llenguatges; també canvia el llenguatge d’un bloc existent), Requadre (Nota, Consell, Important, Avís, Precaució; el mateix tipus una altra vegada l’elimina), Edita la fórmula o el diagrama…, Esborra el format |
| **Insereix** | Enllaça…, Imatge…, Taula…, Fórmula, Diagrama (Diagrama de flux, Diagrama de seqüència, Diagrama de Gantt), Taula de contingut, Nota a peu de pàgina, Separador |
| **Taula** | Insereix una fila a sobre/a sota, Insereix una columna a l’esquerra/a la dreta, Elimina la fila, Elimina la columna, Alineació (Esquerra, Centrat, Dreta, Per omissió), Alinea les columnes de la taula |
| **Mostra** | Oculta/Mostra la previsualització, Mostra/Oculta l’esquema, Puntuació tipogràfica, Apropa/Allunya, Mida real, barra d’eines, barra de pestanyes, pantalla completa |

El subratllat, el ressaltat, el superíndex i el subíndex s’escriuen com `<ins>`, `<mark>`, `<sup>` i `<sub>`, que GitHub també mostra. El Splat també entén `<u>`, que GitHub elimina, i les formes més curtes `==ressaltat==`, `^sup^` i `~sub~`. GitHub mostra `==` i `^` tal com estan escrits, i `~sub~` com a ratllat.

### Llistes i tasques

- Retorn continua una llista; Retorn en un element buit la tanca.
- **Tab** i **⇧Tab** augmenten i redueixen el sagnat dels elements de llista a la previsualització.
- Un clic en una casella de tasca a la previsualització canvia entre `- [ ]` i `- [x]`.
- **Format › Llista** converteix els paràgrafs seleccionats en una llista o en canvia el tipus.

### Taules

- **Insereix › Taula…** demana el nombre de files (inclosa la capçalera) i de columnes.
- A la previsualització, **Tab** passa a la cel·la següent, **⇧Tab** a l’anterior i **Retorn** a la cel·la de sota. Tab a l’última cel·la afegeix una fila.
- El menú **Taula** insereix i elimina files i columnes i defineix l’alineació d’una columna (⌘{, ⌘|, ⌘}).
- **Taula › Alinea les columnes de la taula** omple el codi font amb espais perquè les columnes quedin alineades.

### Enganxar text amb format

**Edita › Enganxa com a Markdown** (⌃⌘V), també al menú contextual dels dos panells, converteix el que hi ha al porta-retalls en Markdown a la selecció, com un sol pas de desfer. ⌘V continua inserint text pla.

- **D’un navegador, el Word, Google Docs, Confluence, Notes o Pages:** es conserven les capçaleres, la negreta, la cursiva, el subratllat, el ratllat, el superíndex i el subíndex, els enllaços, les llistes amb la seva imbricació, les citacions, el codi i les taules.
- **D’un full de càlcul:** les cel·les esdevenen una taula amb fila de capçalera i columnes de números alineades a la dreta. El mateix passa amb el text separat per tabulacions, punts i comes o comes, si totes les línies tenen el mateix nombre de columnes.
- **Les imatges** amb una adreça web continuen sent enllaços. Una imatge dins de l’HTML enganxat esdevé un fitxer al costat del document, que per a això s’ha d’haver desat una vegada. Els fitxers d’imatge temporals del Word no s’incorporen, i el seu text alternatiu es manté.
- **Els enllaços** només conserven adreces `http`, `https`, `mailto`, relatives i `#`; de qualsevol altre enllaç en queda el text.

### Metadades

**Format › Edita les metadades…**, o un clic a la línia de metadades de la part superior de la previsualització, obre un formulari amb els camps del front matter YAML. Els camps es poden canviar, afegir i eliminar. Un camp amb una llista la manté com a llista: els seus valors van un darrere l’altre separats per comes, i un valor que conté una coma va entre cometes. Un camp nou anomenat `tags`, `aliases`, `keywords`, `categories`, `authors` o `cssclasses` també esdevé una llista. Els camps imbricats i el text de diverses línies s’hi poden veure i s’editen al codi font.

El que canvia el formulari és un sol pas de desfer i només afecta les línies dels camps que has canviat: els comentaris, l’ordre, els espais inusuals, les cometes i els finals de línia de tots els altres es mantenen idèntics byte a byte. Un document sense metadades rep un bloc al principi.

## Markdown compatible

El Splat analitza amb [comrak](https://github.com/kivikakk/comrak), un analitzador compatible amb CommonMark i GFM, i activa la majoria de les seves extensions. Tot el que hi ha a continuació es renderitza a la previsualització i a totes les exportacions.

### CommonMark i GitHub Flavored Markdown

- Capçaleres (`#` i subratllades), paràgrafs, salts de línia, èmfasi, èmfasi fort, codi en línia, cites en bloc, separadors.
- Llistes amb vinyetes, numerades i imbricades; llistes de tasques.
- Enllaços (en línia, de referència i automàtics), imatges, definicions de referències d’enllaços.
- Blocs de codi delimitats i sagnats amb colors de sintaxi.
- Taules amb alineació de columnes.
- Ratllat amb `~~dues tildes~~`.
- Notes a peu de pàgina: `text[^1]` amb `[^1]: La nota.` en qualsevol lloc del document. Un clic al marcador salta a la nota.
- YAML front matter entre línies `---` a dalt de tot. La previsualització el resumeix en una sola línia, com ara «Metadades: title, date, tags».

### Requadres

Les alertes de GitHub es renderitzen com a requadres de colors amb icona i títol:

```markdown
> [!NOTE]
> Informació útil que els usuaris haurien de conèixer.

> [!WARNING]
> Contingut crític que requereix atenció immediata.
```

Els cinc tipus són Nota, Consell, Important, Avís i Precaució. **Format › Requadre** converteix el paràgraf on és el cursor en un d’ells.

Els requadres d’Obsidian també funcionen, amb qualsevol títol i un estat de plegat:

```markdown
> [!tip]- Fes clic a ▸ per desplegar
> Ocult fins que s’obre.

> [!question]+ Obert per omissió
> Un `+` comença desplegat, un `-` plegat.
```

Els altres tipus d’Obsidian es mostren amb el més proper dels cinc colors: `success`, `check`, `done` i `hint` com a Consell; `question`, `help` i `faq` com a Important; `attention` com a Avís; `failure`, `fail`, `missing`, `danger`, `error` i `bug` com a Precaució; la resta com a Nota.

### Sintaxi estesa

| Funció | Sintaxi | Origen |
|---|---|---|
| Ressaltat | `==text marcat==` | Obsidian, Typora |
| Superíndex | `x^2^` | Pandoc, Typora |
| Subíndex | `H~2~O` | Pandoc, Typora |
| Codi curt d’emoji | `:rocket:` esdevé 🚀 | GitHub |
| Spoiler | `\|\|text ocult\|\|`, selecciona’l per llegir-lo | Discord |
| Nota en línia | `text^[la nota mateixa]` | Pandoc, Obsidian |
| Llista de definicions | `Terme`, línia en blanc, `: Explicació` | PHP Markdown Extra, Pandoc |
| Àncora de capçalera | `## Instal·lació {#setup}`, enllaça amb `[vegeu](#setup)` | Pandoc, kramdown |

Les llistes de definicions tenen aquest aspecte al codi font:

```markdown
Terme

: Explicació del terme.
```

Sense una àncora pròpia, cada capçalera en rep una derivada del seu text, com a GitHub: `## Primers passos` s’hi arriba com a `#primers-passos`.

### Obsidian

- **Enllaços wiki.** `[[Nota]]` enllaça amb `Nota.md` a la mateixa carpeta o en una carpeta de sota. `[[Nota|Text mostrat]]` mostra el teu propi text, i `[[Nota#Capçalera]]` obre la nota. ⌘-clic l’obre al Splat.
- **Incrustacions.** `![[Nota]]` mostra l’altra nota dins d’aquesta; les notes que hi estan incrustades apareixen com a enllaços. `![[diagram.png]]` mostra una imatge. Com a Obsidian, les incrustacions anomenen notes i imatges de la carpeta del document o de sota, mai amb una ruta absoluta, i només els fitxers Markdown es mostren com a notes.
- **Comentaris.** `%%Només al codi font%%` continua visible al codi font i s’omet de la previsualització i de totes les exportacions, tant en línia com en línies pròpies.
- **Etiquetes.** `#idea` o `#project/splat` es ressalta. ⌘-clic cerca l’etiqueta al document i, a la barra lateral, a tota la carpeta.
- **Requadres plegables.** Mira [Requadres](#requadres).

### Cites i referències

- **Cites de Pandoc.** `[@doe2020]` o `[@doe2020; @roe2021]` es llegeix com «(Doe 2020; Roe 2021)». Les entrades vénen del fitxer BibTeX que el front matter indica amb `bibliography: refs.bib`. Si no n’hi ha, el Splat fa servir `references.bib`, `literatur.bib` o `bibliography.bib` al costat del document.
- **Referències de GitHub i GitLab.** Si el document és en un dipòsit de Git el remot `origin` del qual és a github.com o a un servidor GitLab, `@name`, `#12` (incidència) i `!7` (sol·licitud de combinació, només a GitLab) es ressalten. ⌘-clic els obre al navegador.

### HTML incrustat

L’HTML tal com el trobes als README de GitHub es renderitza, amb un lector propi en comptes d’un motor de navegador:

- paràgrafs i blocs centrats (`<p align="center">`, `<div align="center">`)
- imatges amb mida (`<img src="logo.png" width="120">`)
- enllaços, `<kbd>⌘</kbd>`, `<br>`, `<u>`, `<ins>`, `<mark>`, `<sup>`, `<sub>`
- llistes i taules, incloses les cel·les que ocupen diverses columnes (`colspan`)
- seccions plegables: `<details><summary>Més</summary>…</details>`, que s’obren amb un clic a ▸

El Markdown entre etiquetes HTML es renderitza com a Markdown. Els scripts, els estils, els marcs i els formularis s’ometen del tot, i els enllaços i les imatges només conserven adreces `http`, `https`, `mailto` i relatives.

### Puntuació tipogràfica

**Mostra › Puntuació tipogràfica** mostra les cometes rectes com les cometes de l’idioma de la teva interfície, `--` com un guió mitjà, `---` com un guió llarg i `...` com a punts suspensius. En alemany, `"Ja"` esdevé „Ja“, en francès «Oui», en japonès 「はい」, també allà on el xinès i el japonès posen les cometes sense espai; les abreviatures hebrees com `צה"ל` reben la seva gershayim, צה״ל. El codi font conserva els caràcters simples; la previsualització i totes les exportacions mostren els tipogràfics. L’ajust està desactivat fins que l’actives.

### Resum de la sintaxi

| Element | Markdown |
|---|---|
| **Negreta** | `**Negreta**` |
| *Cursiva* | `*Cursiva*` |
| Subratllat | `<ins>Subratllat</ins>` o `<u>Subratllat</u>` |
| ~~Ratllat~~ | `~~Ratllat~~` |
| Ressaltat | `<mark>Ressaltat</mark>` o `==Ressaltat==` |
| `Codi` | `` `Codi` `` |
| x² | `x<sup>2</sup>` o `x^2^` |
| H₂O | `H<sub>2</sub>O` o `H~2~O` |
| [Enllaç](https://example.com) | `[Enllaç](https://example.com)` |
| Capçalera | `# Capçalera` a `###### Capçalera` |
| Cita | `> Cita` |
| Llista amb vinyetes | `- Element` |
| Llista numerada | `1. Element` |
| Tasca | `- [ ] Pendent` i `- [x] Feta` |
| Bloc de codi | ```` ```rust ```` … ```` ``` ```` |
| Taula | `\| A \| B \|` amb `\|---\|---\|` sota la capçalera |
| Nota a peu de pàgina | `Text[^1]` i `[^1]: Nota` |
| Separador | `---` |
| Imatge | `![Text alternatiu](picture.png)` |
| Fórmula | `$E = mc^2$`, `$$ … $$` |
| Diagrama | ```` ```mermaid ````, ```` ```dot ````, ```` ```plantuml ```` |
| Requadre | `> [!TIP]` |
| Enllaç wiki | `[[Nota]]`, `[[Nota\|Text]]` |
| Incrustació | `![[Nota]]`, `![[image.png]]` |
| Comentari | `%%Comentari%%` |
| Etiqueta | `#etiqueta` |
| Cita bibliogràfica | `[@clau]` |
| Spoiler | `\|\|Spoiler\|\|` |
| Emoji | `:tada:` |

## Fórmules

Les fórmules les renderitza [MathJax](https://www.mathjax.org) amb les seves matemàtiques TeX i AMS, com a gràfics vectorials nítids. El text de `\text{…}` pot ser en qualsevol escriptura: el que falta a les tipografies de MathJax ve de les tipografies del teu Mac, amb l’àrab i l’hebreu escrits de dreta a esquerra, i els emojis en color.

| Tipus | Sintaxi |
|---|---|
| En línia | `$E = mc^2$`, `` $`E = mc^2`$ `` (GitHub, GitLab), `\(E = mc^2\)` |
| En bloc | `$$` … `$$`, `\[` … `\]`, o un bloc de codi marcat amb `math` |

**Editar una fórmula.** Fes clic en una fórmula a la previsualització, fes-hi doble clic, prem Retorn a sobre o posa-hi el cursor per obrir-ne l’editor en una finestra emergent. **Insereix › Fórmula** n’insereix una de nova i n’obre l’editor a l’instant.

- **Visual o LaTeX.** L’interruptor de dalt canvia entre un editor visual ([MathLive](https://cortexjs.io/mathlive/)) i el codi LaTeX. El Splat recorda la teva elecció.
- **Blocs de construcció** per a fracció, arrel quadrada, superíndex i subíndex, suma i integral amb límits, límit, parèntesis que creixen amb el contingut, matriu 2×2, lletres gregues, i relacions, operadors i fletxes. Deixen marcadors □, i **Tab** salta al següent.
- **Previsualització en directe.** Els canvis apareixen al document mentre escrius. Els errors de MathJax es mostren en una línia sota el codi.
- **Confirma o descarta.** Fer clic fora conserva el canvi com un sol pas de desfer; **Esc** o **Cancel·la** el descarta. Prémer Esc en una fórmula nova i buida l’elimina de nou.
- El menú contextual ofereix **Edita la fórmula…** per a la fórmula que hi ha sota el punter.

VoiceOver llegeix una fórmula com el seu codi LaTeX. El Splat renderitza fórmules, no documents LaTeX complets: no hi ha paquets, ni TikZ, ni `\documentclass`.

## Diagrames

Els diagrames es renderitzen dins del Splat, sense connexió i en una variant clara i una de fosca.

| Motor | Bloc de codi | Què dibuixa |
|---|---|---|
| [Mermaid](https://mermaid.js.org) | ```` ```mermaid ```` | diagrames de flux, de seqüència, de classes, d’estats, entitat-relació, de Gantt, de sectors, mapes mentals, cronologies i tots els altres tipus de Mermaid |
| [Graphviz](https://graphviz.org) | ```` ```dot ```` o ```` ```graphviz ```` | grafs en el llenguatge DOT |
| [PlantUML](https://plantuml.com) | ```` ```plantuml ```` o ```` ```puml ```` | diagrames UML; funciona sense Java |

```mermaid
flowchart LR
    A[Markdown] --> B[Previsualització]
    B --> C[PDF]
```

**Editar un diagrama** funciona com editar una fórmula: fes-hi clic per obrir el codi en una finestra emergent, amb colors de sintaxi i missatges d’error en una línia sota el codi. **Insereix › Diagrama** parteix d’una plantilla per a un diagrama de flux, un diagrama de seqüència o un diagrama de Gantt. Els blocs de construcció ajuden amb els tres tipus de Mermaid més habituals:

- **Diagrama de flux:** Node ▾ (rectangle, arrodonit, cercle, rombe, base de dades), Connexió, Connexió amb etiqueta, Grup, Direcció ▾
- **Diagrama de seqüència:** Participant, Missatge, Resposta, Nota, Bucle, Camins alternatius
- **Diagrama de Gantt:** Secció, Tasca, Fita

Els altres tipus de diagrama s’editen com a codi. VoiceOver llegeix el títol o el text alternatiu d’un diagrama.

El primer diagrama després d’obrir l’app triga aproximadament un segon, perquè el motor de renderització arrenca en segon pla. Un diagrama que triga més de 10 segons mostra un error, i el motor de renderització arrenca de nou per al següent. El motor de PlantUML (3 MB) només es carrega amb el primer diagrama de PlantUML. Un `!include` de PlantUML des del web es bloqueja.

## Imatges

- **Les imatges locals** fan servir rutes relatives al document: `![Logotip](images/logo.png)`. El Splat mostra PNG, JPEG, GIF, WebP, BMP, SVG, HEIC, TIFF, AVIF i PDF.
- **Les imatges d’internet** continuen bloquejades fins que les permets: una barra sobre la previsualització explica que carregar-les fa saber als seus servidors que obres el document i quan. **Carrega les imatges** les permet per a aquest document; el Splat recorda l’elecció per a cada fitxer.
- **Mida.** `<img width="200">` dona mida a una imatge; si no, les imatges s’ajusten a l’amplada de la previsualització.
- **Canvis al disc.** Una imatge que substitueixes, o una que apareix després que el document la nomeni, es mostra sense tornar a obrir el document. Les imatges d’internet que han fallat per culpa de la xarxa es tornen a provar mentre un document les mostra.
- **Memòria.** Les imatges es mostren amb fins a 2560 píxels al costat més llarg, cosa que també omple una pàgina A4. Es descodifiquen més petites quan ocupen una vuitena part de la memòria del Mac, i mentre macOS informa de pressió de memòria.
- **Mode fosc.** Les imatges amb fons blanc s’atenuen una mica a la previsualització fosca.
- **Enganxar i deixar anar.** Mira [Al codi font](#al-codi-font).

## Enllaços i navegació

A la previsualització, un clic situa el cursor perquè puguis editar el text de l’enllaç; **⌘-clic** segueix l’enllaç:

- `http`, `https` i `mailto` s’obren al teu navegador o a l’app de correu.
- Els fitxers Markdown s’obren al Splat; els altres fitxers i carpetes es mostren al Finder.
- `#anchor` salta a la capçalera del document.
- Les adreces que obren una altra app, com `slack://` o `vscode://`, demanen confirmació abans i mostren l’adreça completa i l’app. **Cancel·la** ve preseleccionat.

**Insereix › Taula de contingut** insereix una llista enllaçada de totes les capçaleres. Amb el cursor a dins, la mateixa ordre l’actualitza. La barra lateral d’esquema (⌃⌘S) salta a qualsevol capçalera als dos panells.

## Notes en una carpeta

Els botons de la part superior de la barra lateral alternen entre l’esquema, els fitxers i una cerca.

- **Fitxers** (**Mostra › Mostra els fitxers**, ⌃⌘E). Tots els fitxers Markdown de la carpeta del document o, si és en un repositori Git, de tot el repositori. Un clic o Retorn obre una nota com a pestanya d’aquesta finestra; si ja està oberta, passa al davant. El botó al costat del nom de la carpeta mostra una altra carpeta en aquesta finestra. Els fitxers ocults, `node_modules`, `target`, `Pods`, `DerivedData`, `__pycache__` i les carpetes sense notes queden fora. Un document directament a la teva carpeta d’inici només mostra les notes que té al costat, perquè el Splat no recorri totes les teves carpetes demanant permís per a cadascuna.
- **Cerca** (**Edita › Cerca › Cerca a totes les notes…**, ⇧⌘F). Troba text a totes les notes, sigui quina sigui la combinació de majúscules i minúscules i la codificació dels accents. `#tag` troba una etiqueta, `#project` també `#project/splat`, i res dins de codi. Un clic en un resultat obre la nota en aquell punt.
- **Enllaços a una nota.** El menú contextual d’una nota llista les notes que hi enllacen, per enllaç wiki, incrustació, enllaç relatiu o definició de referència d’enllaç.
- **Canviar el nom d’una nota.** El mateix menú li canvia el nom i canvia aquests enllaços. El Splat primer diu de quants enllaços en quants fitxers es tracta. A cada enllaç només canvia el nom, i tots els altres bytes del fitxer es mantenen. Els documents oberts reben el canvi com un pas de desfer; en altres fitxers, només Git o Versions el poden desfer. Un fitxer que un altre programa hagi canviat mentrestant atura el canvi de nom de tots els enllaços, i el Splat l’anomena.

La llista segueix la carpeta quan la finestra passa al davant. La cerca veu els fitxers tal com són al disc, i el text que encara no has desat compta a partir del següent desament automàtic.

## Canvis des de l’últim commit

En un document d’un repositori Git, unes barres al costat del text marquen què difereix de l’últim commit, al codi font i a la previsualització: verd per a les línies afegides, blau per a les línies canviades i una marca vermella curta on s’ha eliminat alguna cosa. Segueixen mentre escrius i quan la branca es mou. **Mostra › Canvis des de l’últim commit** les desactiva, per a totes les finestres.

Les línies que només difereixen en el final de línia no compten, de manera que un fitxer amb CRLF davant d’un commit amb LF no mostra cap canvi. Un fitxer nou no té marques fins al seu primer commit, ni tampoc un document de més de 32 MB.

## Exportar, imprimir i compartir

| Format | Com | Què obtens |
|---|---|---|
| **PDF** | **Arxiu › Exporta com a PDF…** o el menú Comparteix | Pàgines de la mida del paper, sempre en l’aparença clara, amb fórmules i diagrames com a gràfics vectorials. **Arxiu › Configuració de pàgina…** defineix el paper. |
| **Impressió** | **Arxiu › Imprimeix…** (⌘P) | El mateix disseny que el PDF. |
| **HTML** | **Arxiu › Exporta com a HTML…** o el menú Comparteix | Un únic fitxer autònom: imatges locals incrustades, fórmules i diagrames com a imatges nítides, àncores de capçalera perquè la taula de contingut funcioni. Sense scripts. |
| **Word** | **Arxiu › Exporta com a document del Word…** o el menú Comparteix | Un `.docx` amb els estils de capçalera de Word, llistes i taules de debò (incloses les cel·les combinades), notes a peu de pàgina com a notes a peu de pàgina de Word, imatges incrustades, fórmules i diagrames. |
| **Markdown** | El botó **Comparteix** de la barra d’eines | El document com a fitxer `.md` per a Mail, Missatges, AirDrop i tots els altres serveis per compartir. |

El menú **Arxiu › Comparteix** ofereix tot això en un sol lloc. Les exportacions fan servir l’ajust de puntuació tipogràfica i les imatges d’internet que has permès; en HTML, les imatges d’internet continuen sent enllaços.

**Arxiu › Opcions d’exportació…** defineix què afegeixen al text el PDF, la impressió i el Word, per a tots els documents:

- **Números de pàgina** al peu de pàgina del PDF i de la impressió, com a `2 / 9`.
- **Capçalera i peu de pàgina** per al PDF i la impressió. `{title}`, `{author}`, `{date}`, `{page}` i `{pages}` representen els seus valors, els tres primers a partir de les metadades del document.
- **Una portada** per al PDF, la impressió i el Word, amb `title`, `subtitle`, `author` i `date` de les metadades, o el nom del document com a títol. No té capçalera ni peu de pàgina.
- **Una plantilla del Word**, un `.docx` o `.dotx` l’aspecte del qual adopta l’exportació: els seus estils, també amb noms localitzats, el seu tema, la mida de pàgina, els marges, les capçaleres i els peus de pàgina. Una plantilla que no es pot llegir atura l’exportació i ho diu, en lloc de donar-te un document sense ella.

## Importar documents del Word

**Arxiu › Importa un document del Word…** demana un fitxer `.docx` o `.dotx` i on ha d’anar el seu Markdown, i després obre el resultat. Les imatges van a una carpeta al costat, amb el nom del document seguit de `-images`.

Les capçaleres i les llistes arriben tal com les defineixen els estils i la numeració del Word, també en documents amb estils de noms localitzats. Es conserven la negreta, la cursiva, el subratllat, el ratllat, el ressaltat, el superíndex i el subíndex, els enllaços, les citacions, el codi, les notes a peu de pàgina, les notes finals i les taules, amb les cel·les que ocupen diverses columnes com a taula HTML. Les fórmules esdevenen LaTeX. Els canvis registrats compten com a acceptats, i una taula de contingut queda fora, ja que **Insereix › Taula de contingut** en crea una de nova. Els fitxers `.doc` antics i els documents protegits amb contrasenya es rebutgen.

## Aparença i accessibilitat

- **El mode fosc** segueix el sistema. Les fórmules i els diagrames es renderitzen per a cada aparença.
- **Augmenta el contrast**, a Configuració del Sistema, canvia els colors de sintaxi a paletes d’alt contrast.
- **Zoom.** ⌘+, ⌘− i ⌘0 escalen els dos panells.
- **Pantalla completa, pestanyes i una barra d’eines personalitzable**, com en qualsevol app del Mac.
- **VoiceOver.** Els panells s’anomenen «Codi font Markdown» i «Previsualització», les fórmules es llegeixen com el seu codi i els diagrames com el seu títol. Totes les ordres es poden fer amb el teclat.

El Splat no té finestra d’ajustos: les poques opcions són on s’apliquen, a **Mostra**, a **Edita › Substitucions** i a **Arxiu › Opcions d’exportació…**.

## Idiomes

La interfície, els menús i la referència ràpida de Markdown estan traduïts a 35 idiomes:

àrab, català, xinès (simplificat), xinès (tradicional), croat, txec, danès, neerlandès, anglès, finès, francès, alemany, grec, hebreu, hindi, hongarès, indonesi, italià, japonès, coreà, malai, noruec, polonès, portuguès (Brasil), portuguès (Portugal), romanès, rus, eslovac, eslovè, castellà, suec, tailandès, turc, ucraïnès i vietnamita.

El Splat segueix l’ordre d’idiomes de **Configuració del Sistema › General › Idioma i regió**; allà també pots triar un idioma només per al Splat, a **Apps**. Si no hi ha traducció per al teu idioma, fa servir l’anglès. Els termes dels menús segueixen les traduccions d’Apple, de manera que el Splat es llegeix com la resta del teu Mac.

El text en hebreu i en àrab es llegeix de dreta a esquerra allà on aparegui, sigui quin sigui l’idioma de la interfície: cada paràgraf, capçalera i element de llista pren la direcció de la seva primera lletra, les cites i els requadres dibuixen la barra a la dreta, i una taula el text de la qual comença de dreta a esquerra ordena les columnes des de la dreta. Els blocs de codi continuen d’esquerra a dreta. Les exportacions a PDF, HTML i Word conserven aquestes direccions. Amb l’hebreu o l’àrab com a idioma del sistema, macOS també emmiralla la finestra, de manera que la previsualització queda a l’esquerra.

## Dreceres de teclat

| Acció | Drecera |
|---|---|
| Negreta, cursiva, subratllat | ⌘B, ⌘I, ⌘U |
| Ratllat | ⇧⌘X |
| Enllaça… | ⌘K |
| Cos de text, capçalera de l’1 al 6 | ⌥⌘0, ⌥⌘1 a ⌥⌘6 |
| Alinea la columna de la taula a l’esquerra, centrada, a la dreta | ⌘{, ⌘\|, ⌘} |
| Oculta o mostra la previsualització | ⌥⌘P |
| Mostra o oculta l’esquema | ⌃⌘S |
| Mostra o oculta els fitxers | ⌃⌘E |
| Cerca a totes les notes | ⇧⌘F |
| Enganxa com a Markdown | ⌃⌘V |
| Apropa, allunya, mida real | ⌘+, ⌘−, ⌘0 |
| Nou document, obre, desa, duplica | ⌘N, ⌘O, ⌘S, ⇧⌘S |
| Configuració de pàgina, imprimeix | ⇧⌘P, ⌘P |
| Cerca, cerca i substitueix | ⌘F, ⌥⌘F |
| Cerca el següent, l’anterior; utilitza la selecció; ves a la selecció | ⌘G, ⇧⌘G; ⌘E; ⌘J |
| Mostra l’ortografia i la gramàtica; revisa el document | ⌘:, ⌘; |
| Enganxa amb el mateix estil | ⌥⇧⌘V |
| Oculta la barra d’eines, mostra totes les pestanyes, pantalla completa | ⌥⌘T, ⇧⌘\\, ⌃⌘F |
| Salt de línia (previsualització) | ⌥Retorn o ⌃Retorn |
| Cel·la següent / anterior d’una taula (previsualització) | Tab / ⇧Tab |
| Augmenta / redueix el sagnat d’un element de llista (previsualització) | Tab / ⇧Tab |
| Següent marcador a l’editor de fórmules | Tab |
| Tanca l’editor de fórmules o de diagrames sense canvis | Esc |
| Segueix un enllaç, una etiqueta o una referència (previsualització) | ⌘-clic |

El Splat deixa les dreceres estàndard d’Apple tal com són.

## Privadesa i seguretat

Un fitxer Markdown pot venir de qualsevol, així que el Splat tracta cada document com a no fiable.

- **Sense xarxa per omissió.** El Splat no té telemetria. Només cerca actualitzacions quan hi estàs d’acord o tries **Cerca actualitzacions…**: llavors carrega un petit fitxer de canal de GitHub, i la sol·licitud no esmenta res més que l’app i la seva versió. Les fórmules i els diagrames es renderitzen en local; les vistes web que ho fan no poden accedir a la xarxa. **Ajuda › Informa d’un problema…** obre un formulari de GitHub al navegador; la seva adreça porta la versió del Splat, la de macOS i el tipus de Mac, però res dels teus documents.
- **Les imatges d’internet** només es carreguen amb el teu permís, només per `https`, mai des de servidors de la teva xarxa local, com a màxim de 10 MB cadascuna i en una sessió privada sense galetes.
- **Descodificació aïllada.** Les imatges i els gràfics de fórmules i diagrames es descodifiquen en processos auxiliars que tanquen tots els fitxers que han heretat i es tanquen ells mateixos en una caixa de proves abans de llegir el primer byte: sense accés a fitxers ni a la xarxa. Només retornen píxels, la mida dels quals es comprova, i s’aturen als 10 segons. Els descodificadors d’imatge d’Apple només veuen HEIC, TIFF, AVIF i PDF, i només quan les dades són realment d’aquest format; s’executen en un procés auxiliar propi, que a més pot llegir les carpetes de tipus de lletra del sistema i fer servir el descodificador de vídeo del sistema. Els SVG, les fórmules i els diagrames també passen per aquest procés, de manera que al Splat són imatges amb una resolució fixa: quatre píxels per punt, fins a 2560 al costat més llarg.
- **Els fitxers locals** només arriben a un document per la ruta que aquest anomena. Les incrustacions mostren notes Markdown de la carpeta del document i de sota, i una imatge que falta només es busca allà. Una ruta d’imatge a un altre lloc, absoluta o que comença per `~`, es carrega igualment a la previsualització; les exportacions HTML i Word, la impressió i els PDF només s’emporten el que hi ha a la carpeta del document o al seu repositori Git, tret que aquest repositori també contingui la teva carpeta d’inici, de manera que un document d’una altra persona no pot arrossegar els teus fitxers a una exportació que passis a algú.
- **Les carpetes de notes** només es llegeixen mentre la barra lateral les mostra, i canviar el nom d’una nota només canvia altres fitxers després que hagis confirmat quants.
- **Els repositoris Git** els llegeix el mateix Splat. Mai no executa el programa `git`, la configuració del qual en un repositori d’algú altre pot iniciar altres programes, i no llegeix més que la versió del document obert de l’últim commit. No hi escriu res i no fa servir cap xarxa.
- **Els documents del Word i el text enganxat** els llegeixen els analitzadors propis del Splat, amb límits de mida, d’imbricació i de quant es pot descomprimir un fitxer. Els seus enllaços només conserven les adreces que el Splat permet en altres llocs, i els noms de les imatges d’un fitxer del Word mai no decideixen on s’escriuen.
- **Les còpies del text sense desar** es queden en aquest Mac, a la carpeta Library, per a la recuperació després d’una fallada. Desapareixen amb el desament o amb el tancament normal del Splat; les còpies que una fallada ha deixat es queden a la carpeta de recuperació fins que les eliminis.
- **Els enllaços** mai inicien un programa amb un clic. Els enllaços web i de correu s’obren com sempre; els enllaços a altres apps demanen confirmació.
- **L’HTML als documents** el llegeix l’analitzador propi del Splat. Els scripts, els estils, els marcs i els formularis no s’executen mai.
- **Les exportacions** no porten scripts. Les pàgines HTML només conserven una llista fixa d’etiquetes i atributs inofensius, com fa GitHub, més una política de seguretat de contingut. Els fitxers de Word només enllacen adreces `http`, `https` i `mailto`.
- **Signatura de codi** amb el Hardened Runtime. El Splat s’executa sense l’App Sandbox perquè pugui mostrar imatges al costat dels teus documents sense demanar permís per a cada carpeta; l’aïllament de tota la descodificació ho compensa.

## Documents grans

El Splat continua responent amb documents de diversos megabytes:

- El codi font fa servir TextKit 2, que només disposa la part visible.
- Mentre escrius a la previsualització, i al codi font de documents de fins a 2 MB, només s’analitzen, es coloregen i es tornen a disposar els paràgrafs afectats. En un document de 10 MB, analitzar i colorejar triga uns 3 ms, renderitzar uns 10 ms, i una pulsació de tecla sencera a la seva previsualització uns 30 ms amb la feina pròpia de la vista.
- Les previsualitzacions grans renderitzen primer la part visible i la resta mentre l’app està inactiva. Els blocs que es desplacen a la vista es renderitzen abans de dibuixar-se.
- Al codi font dels documents més grans, l’anàlisi per a la previsualització s’executa en segon pla mentre escrius.
- Un document es manté com a text pla quan necessitaria uns 4 GB de memòria, és a dir, uns 50 a 120 MB de Markdown habitual, però només de 10 a 20 MB si tot són taules o encapçalaments: el codi font no té colors i la previsualització diu per què es manté buida. Les exportacions continuen funcionant.
- A partir d’uns 200 KB, un document obre la finestra abans d’analitzar-se; a 5 MB, els colors i la previsualització arriben aproximadament un quart de segon més tard.
- Totes les imatges juntes, incloses les fórmules i els diagrames, ocupen com a màxim una quarta part de la memòria del Mac, i la meitat d’això mentre hi ha poca memòria. Més enllà d’això, les altres imatges es mostren com a marcadors de posició fins que torna a haver-hi espai.

Alguns canvis es processen sencers, amb uns 300 ms per pulsació de tecla a 10 MB: els de la definició o de les referències d’una nota a peu de pàgina, els que hi ha al costat d’una nota en línia, al final mateix d’un document al costat d’una nota a peu de pàgina, al front matter o a definicions de referències d’enllaços, i tots els canvis en un document amb blocs de comentaris `%%` o amb una nota a peu de pàgina definida dues vegades o dins d’una cita o d’una llista. La memòria creix amb el fitxer, fins a uns 115–160 MB per a 1 MB de Markdown i 340–510 MB per a 10 MB.

## Limitacions conegudes

- **Les tildes soltes volen dir subíndex.** `~text~` és subíndex com a Pandoc; el ratllat necessita `~~dues~~`. GitHub també ratlla amb una sola tilde.
- **Només de lectura a la previsualització:** l’HTML incrustat i les notes incrustades s’editen al codi font, el front matter al seu formulari o al codi font.
- **Imbricació molt profunda:** l’HTML imbricat a més de 64 nivells, i l’HTML en línia a més de 16, cedeix el pas al seu contingut.
- **Les cel·les combinades de taula** només existeixen en taules HTML, i només entre columnes (`colspan`, no `rowspan`). La sintaxi de taules de MultiMarkdown per a cel·les combinades no és compatible.
- **Les bibliografies** només es llegeixen de BibTeX, no de CSL JSON ni de YAML.
- **Les referències** necessiten un dipòsit a github.com o a un servidor GitLab; `!7` només existeix a GitLab.
- **Puntuació tipogràfica:** una cometa simple de tancament alemanya no es pot distingir d’un apòstrof i es posa com a ’.
- **Fórmules, no documents:** sense paquets LaTeX, TikZ ni documents complets.
- **L’editor visual de fórmules** escriu LaTeX amb la notació de MathLive: els espais desapareixen (`E = mc^2` esdevé `E=mc^2`), la renderització continua igual.
- **Recompte de paraules:** el tai, el lao, el khmer i el birmà s’escriuen sense espais i es compten per tram de text, no per paraula. `3.14` i `snake_case` són dues paraules cadascun.
- **Canviar el nom d’una nota** no canvia els enllaços en HTML incrustat, i un enllaç wiki queda tal com està si el nom nou conté `#`, `|`, `^`, `[`, `]` o `\`.
- **Les marques de canvis** llegeixen repositoris amb objectes SHA-1 i refs simples o empaquetades; un amb SHA-256 o reftable no en mostra cap. Un fitxer sota un filtre de Git com LFS es compara amb el que el filtre ha desat.
- **La importació del Word** no incorpora el text de capçaleres, peus de pàgina i comentaris, i les imatges en `.emz` o SVG només conserven el text alternatiu.
- **Enganxa com a Markdown** llegeix l’RTF com a text occidental, i només en pren les capçaleres a partir de les mides de lletra. La majoria de programes també ofereixen HTML, que és el que prefereix.
- **Per disseny:** sense números de línia, sense finestra d’ajustos, sense Help Book, no és a l’App Store.
- **Provat menys en Intel.** Els autotests de finestra només s’executen en Apple Silicon; en Intel, la CI executa `cargo test`.

## Resolució de problemes

**La llista de documents recents és buida després de tornar a compilar el Splat.** macOS lliga els documents recents i els permisos de carpeta a la signatura de codi de l’app. Les compilacions locals es signen amb el certificat «Splat Local Signing» del teu clauer d’inici de sessió, de manera que en recompilar es conserven tots dos. Si s’ha eliminat el certificat, la compilació següent en crea un de nou i les llistes tornen a començar una vegada.

**L’app de la imatge de disc mostra uns documents recents diferents dels de la meva pròpia compilació.** Les dues porten signatures diferents, de manera que macOS manté una llista i uns permisos de carpeta separats per a cadascuna.

**L’HTML o una altra sintaxi es mostra com a codi pla.** Pot ser que una còpia antiga del Splat hagi obert el document. Tanca-la, tria **Arxiu › Obtén informació** en el document al Finder i comprova que **Obre amb** indiqui el Splat de la teva carpeta Aplicacions.

**No hi ha cap Cerca actualitzacions… al menú Splat.** Només la versió de la imatge de disc o de Homebrew s’actualitza sola; una compilació pròpia no té actualitzador.

**Les imatges d’internet no apareixen.** Fes clic a **Carrega les imatges** a la barra sobre la previsualització. Només es carreguen les imatges `https` de servidors públics.

**Enganxar una captura de pantalla em demana desar primer.** Les imatges enganxades es desen com a fitxers al costat del document, cosa que necessita un lloc al disc. Desa’l i torna a enganxar.

**«El fitxer no és text UTF-8 i no es pot obrir com a Markdown.»** El fitxer està en una altra codificació i no té cap marca d’ordre de bytes que la indiqui. Converteix-lo primer, per exemple amb `iconv -f ISO-8859-1 -t UTF-8 old.md > new.md`.

**Una fórmula o un diagrama mostra un error.** Fes-hi clic: l’editor mostra el missatge de MathJax, Mermaid, Graphviz o PlantUML sota el codi.

## Per a desenvolupadors

La compilació, les proves, l’estructura del codi i les regles que segueix es descriuen a [docs/development.md](../../../docs/development.md) (en anglès).

### Components de tercers

| Component | Versió | Ús | Llicència |
|---|---|---|---|
| [comrak](https://github.com/kivikakk/comrak) | 0.59 amb [una línia canviada](../../../vendor/comrak/PATCH.md) | analitzador de Markdown | BSD-2-Clause |
| [objc2](https://github.com/madsmtm/objc2), objc2-foundation, block2 | 0.6 / 0.3 | entorn d’execució d’Objective-C, Foundation, blocs | MIT |
| objc2-app-kit, objc2-web-kit, dispatch2 | 0.3 | enllaços amb AppKit, WebKit i Grand Central Dispatch | Zlib, Apache-2.0 o MIT |
| [krilla](https://github.com/LaurenzV/krilla), krilla-svg, [usvg](https://github.com/linebender/resvg) | 0.8 / 0.47 | fórmules i imatges SVG com a PDF vectorial | MIT o Apache-2.0 |
| [image](https://github.com/image-rs/image) | 0.25 | descodificació de PNG, JPEG, GIF, WebP i BMP | MIT o Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2 | alineació de les columnes de les taules | MIT o Apache-2.0 |
| [entities](https://github.com/p-jackson/entities) | 1 | els noms de les referències de caràcters HTML | MIT |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide) | 0.9 | descomprimir fitxers del Word i objectes de Git | MIT, Zlib o Apache-2.0 |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.21 | llegir l’XML dels fitxers del Word | MIT o Apache-2.0 |
| [unicode-normalization](https://github.com/unicode-rs/unicode-normalization) | 0.1 | noms i cerca entre lletres compostes i descompostes | MIT o Apache-2.0, Unicode-3.0 |
| [emojis](https://github.com/rossmacarthur/emojis) | 0.9 | mantenir literal el text semblant a `:shortcode:` en text importat | MIT o Apache-2.0, Unicode-3.0 |
| [MathJax](https://www.mathjax.org) | 4.1.3 | renderització de fórmules | Apache-2.0 |
| [MathLive](https://cortexjs.io/mathlive/) | 0.111.4 | editor visual de fórmules, amb tipus de lletra KaTeX | MIT |
| [Mermaid](https://mermaid.js.org) | 12.3.0 | diagrames de Mermaid | MIT |
| [Viz.js](https://github.com/mdaines/viz-js) | 3.24.0 | Graphviz compilat a WebAssembly, conté Graphviz (EPL-1.0) i Expat (MIT) | MIT |
| [PlantUML](https://github.com/plantuml/plantuml) (`@plantuml/core`) | 1.2026.9 | diagrames de PlantUML, compilats a JavaScript | MIT |
| [Sparkle](https://sparkle-project.org) | 2.10.1 | actualitzacions de la versió de la imatge de disc | MIT, amb parts sota llicències BSD i zlib |

Les biblioteques de JavaScript s’emmagatzemen a `resources/js/` exactament tal com es van publicar; [vendor.lock](../../../vendor.lock) n’enregistra les fonts i les sumes de verificació SHA-256. Els seus textos de llicència, inclosos els de Graphviz i Expat dins de Viz.js, són al seu costat com a `resources/js/LICENSE-*` i van dins de l’app. Sparkle no s’emmagatzema al dipòsit: `cargo xtask dmg` en descarrega la versió i la comprova amb la suma de verificació d’xtask, i la seva llicència va dins de l’app com a `LICENSE-sparkle`.

## Llicència

El Splat té llicència sota qualsevol de les dues següents:

- Llicència Apache, versió 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE))
- Llicència MIT ([LICENSE-MIT](../../../LICENSE-MIT))

a la teva elecció.

Tret que indiquis explícitament el contrari, qualsevol contribució enviada intencionadament per tu per ser inclosa al Splat, tal com es defineix a la llicència Apache-2.0, tindrà la doble llicència anterior, sense cap condició addicional.
