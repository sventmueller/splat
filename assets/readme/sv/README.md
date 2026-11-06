<p><img src="../../readme-icon.png" alt="Splats appikon" width="168" height="168">&nbsp;&nbsp;&nbsp;<img src="screenshot.png" alt="Splat med Demo.md: Markdown-källtexten till vänster, den redigerbara förhandsvisningen till höger" width="274" height="168"></p>

# Splat

**Markdown-editorn för macOS där du skriver i källtexten eller direkt i förhandsvisningen.**

Splat visar din Markdown-källtext med syntaxfärger till vänster och en levande, helt redigerbar förhandsvisning till höger. Skriv i
någon av dem: allt du skriver i förhandsvisningen blir en minimal ändring i källtexten, så rader du inte rört
förblir identiska byte för byte och dina Git-diffar hålls rena. Splat läser den Markdown du redan använder: CommonMark och
GitHub Flavored Markdown, GitHub-alerts, Obsidians wikilänkar, inbäddningar och inforutor, tillägg från Pandoc och Typora,
inbäddad HTML, LaTeX-formler samt diagram i Mermaid, Graphviz och PlantUML, allt renderat offline. Bläddra och sök i en hel
mapp med anteckningar, se vad som ändrats sedan senaste Git-commit, klistra in från Word eller en webbsida som Markdown och
exportera till PDF, HTML eller Word med ett klick. Splat är skrivet i Rust med äkta AppKit-kontroller, finns på 35 språk och
skickar ingenting över nätverket om du inte ber om det.
 


[![Ladda ner Splat för macOS](https://img.shields.io/badge/Download-Splat_for_macOS-0A84FF?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)

**[Ladda ner Splat.dmg](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)**: signerad och notariserad av Apple, för macOS 14 eller senare på Apple Silicon och Intel.

**Hittat en bugg?** Splat håller rader du inte har rört identiska byte för byte. Om en ändå ändras, eller om en fil visas fel, [öppna ett issue](https://github.com/sventmueller/splat/issues/new/choose) och bifoga filen. Om du har nytta av Splat hjälper en [stjärna på GitHub](https://github.com/sventmueller/splat) andra att hitta det.
 
 

[![Rust 2024 edition](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![macOS 14 eller senare](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)
![Universell: Apple Silicon och Intel](https://img.shields.io/badge/Universal-Apple_Silicon_%7C_Intel-333333?logo=apple&logoColor=white)
[![AppKit via objc2](https://img.shields.io/badge/AppKit-objc2_0.6-147EFB?logo=apple&logoColor=white)](https://github.com/madsmtm/objc2)
[![WebKit](https://img.shields.io/badge/WebKit-offline-0A84FF?logo=safari&logoColor=white)](https://webkit.org)
[![Comrak 0.59](https://img.shields.io/badge/comrak-0.59-DEA584?logo=markdown&logoColor=white)](https://github.com/kivikakk/comrak)
[![CommonMark och GFM](https://img.shields.io/badge/CommonMark-GFM-24292F?logo=github&logoColor=white)](https://github.github.com/gfm/)
[![MathJax 4.1.3](https://img.shields.io/badge/MathJax-4.1.3-1E6E9E?logo=latex&logoColor=white)](https://www.mathjax.org)
[![MathLive 0.111.4](https://img.shields.io/badge/MathLive-0.111.4-5A3FC0)](https://cortexjs.io/mathlive/)
[![Mermaid 12.3.0](https://img.shields.io/badge/Mermaid-12.3.0-FF3670?logo=mermaid&logoColor=white)](https://mermaid.js.org)
[![Graphviz via Viz.js 3.24](https://img.shields.io/badge/Graphviz-Viz.js_3.24-2596BE)](https://graphviz.org)
[![PlantUML 1.2026.9](https://img.shields.io/badge/PlantUML-1.2026.9-A80036)](https://plantuml.com)
![35 gränssnittsspråk](https://img.shields.io/badge/languages-35-4C9A2A)
![Offline som standard](https://img.shields.io/badge/network-offline_by_default-4C9A2A)
![Signerad med Hardened Runtime](https://img.shields.io/badge/Hardened_Runtime-signed-4C9A2A)
[![Licens: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-4C9A2A)](#licens)

[Bahasa Indonesia](../id/README.md) ·
[Bahasa Melayu](../ms/README.md) ·
[Català](../ca/README.md) ·
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
**Svenska** ·
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

## Innehåll

- [Funktioner i korthet](#funktioner-i-korthet)
- [Krav](#krav)
- [Installation](#installation)
- [Komma igång](#komma-igång)
- [Skriva](#skriva)
  - [I källtexten](#i-källtexten)
  - [I förhandsvisningen](#i-förhandsvisningen)
  - [Formateringskommandon](#formateringskommandon)
  - [Listor och uppgifter](#listor-och-uppgifter)
  - [Tabeller](#tabeller)
  - [Klistra in formaterad text](#klistra-in-formaterad-text)
  - [Metadata](#metadata)
- [Markdown som stöds](#markdown-som-stöds)
  - [CommonMark och GitHub Flavored Markdown](#commonmark-och-github-flavored-markdown)
  - [Inforutor](#inforutor)
  - [Utökad syntax](#utökad-syntax)
  - [Obsidian](#obsidian)
  - [Citat och referenser](#citat-och-referenser)
  - [Inbäddad HTML](#inbäddad-html)
  - [Typografiska skiljetecken](#typografiska-skiljetecken)
  - [Syntaxöversikt](#syntaxöversikt)
- [Formler](#formler)
- [Diagram](#diagram)
- [Bilder](#bilder)
- [Länkar och navigering](#länkar-och-navigering)
- [Anteckningar i en mapp](#anteckningar-i-en-mapp)
- [Ändringar sedan senaste commit](#ändringar-sedan-senaste-commit)
- [Exportera, skriva ut och dela](#exportera-skriva-ut-och-dela)
- [Importera Word-dokument](#importera-word-dokument)
- [Utseende och tillgänglighet](#utseende-och-tillgänglighet)
- [Språk](#språk)
- [Kortkommandon](#kortkommandon)
- [Integritet och säkerhet](#integritet-och-säkerhet)
- [Stora dokument](#stora-dokument)
- [Kända begränsningar](#kända-begränsningar)
- [Felsökning](#felsökning)
- [För utvecklare](#för-utvecklare)
  - [Komponenter från tredje part](#komponenter-från-tredje-part)
- [Licens](#licens)

## Funktioner i korthet

- **Två sätt att skriva, en text.** Källtext och förhandsvisning går båda att redigera. Markdown-filen är den enda sanningen: det
  finns en ångrahistorik per dokument, och varje ändring i förhandsvisningen översätts till den minsta möjliga ändringen i källtexten.
- **Den Markdown du redan använder.** CommonMark, GitHub Flavored Markdown, GitHub-alerts, Obsidian, tillägg från Pandoc och Typora,
  emoji-kortkoder, spoilers, definitionslistor, rubrikankare och HTML som i GitHubs README-filer.
- **Formler och diagram inbyggda.** LaTeX-formler via MathJax, med en visuell formelredigerare. Diagram i Mermaid,
  Graphviz och PlantUML, renderade på din Mac utan Java, server eller nätverksanslutning.
- **En knapp för allt.** Varje Markdown-konstruktion har en knapp i verktygsfältet och ett menyalternativ, de vanliga dessutom ett
  kortkommando. Tabeller har en egen meny för rader, kolumner och justering.
- **En mapp, inte bara en fil.** Sidofältet visar anteckningarna runt ett dokument och söker i dem alla. När du byter
  namn på en anteckning ändras länkarna till den, och varje byte runt dem ligger kvar.
- **Ändringar på ett ögonkast.** I ett Git-arkiv markerar streck bredvid texten det som skiljer sig från senaste commit,
  i båda rutorna.
- **Från Word och webben till Markdown.** Klistra in formaterad text som Markdown och celler från ett kalkylark som en
  tabell, och importera hela Word-dokument med deras bilder.
- **Exportera utan att förlora något.** PDF, en enda fristående HTML-fil eller ett Word-dokument med riktiga rubriker,
  listor, tabeller, fotnoter och bilder. Lägg till sidnummer, sidhuvuden och en titelsida, eller använd företagets Word-mall.
- **Native in i märgen.** Äkta AppKit-fönster, flikar, menyer, popovers, Versioner, Spara automatiskt, delningsmenyn, mörkt läge,
  stavningskontroll, diktering och VoiceOver. Inget Electron, ingen webbvy för själva texten.
- **Snabb med stora filer.** Att rendera en tangenttryckning i ett dokument på 10 MB tar cirka 10 ms, före vyns eget arbete.
- **Privat och försiktig.** Bilder från webben läses in först när du tillåter det, länkar till andra appar frågar först, och
  allt som avkodar opålitliga bilddata körs i hjälpprocesser i en sandlåda.
- **På ditt språk.** Gränssnittet och den inbyggda Markdown-snabbreferensen finns på 35 språk: arabiska, katalanska,
  kinesiska (förenklad), kinesiska (traditionell), kroatiska, tjeckiska, danska, nederländska, engelska, finska, franska,
  tyska, grekiska, hebreiska, hindi, ungerska, indonesiska, italienska, japanska, koreanska, malajiska, norska, polska, portugisiska (Brasilien),
  portugisiska (Portugal), rumänska, ryska, slovakiska, slovenska, spanska, svenska, thailändska, turkiska, ukrainska och
  vietnamesiska.

## Krav

- macOS 14 Sonoma eller senare. Splat utvecklas och testas på macOS 27, där appen automatiskt får designen Liquid Glass.
- En Mac med Apple Silicon eller en Intel-processor.
- Cirka 45 MB diskutrymme.

## Installation

**Från skivavbilden.** Ladda ner [`Splat.dmg`](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg), öppna den och dra Splat till mappen Appar. Avbilden är signerad med ett
Developer ID och notariserad av Apple, så den öppnas utan varningar.

**Med Homebrew.** Cask-paketet finns i en egen tap:

```sh
brew install --cask sventmueller/tap/splat
```

`brew upgrade --cask splat` installerar nya versioner, och `brew uninstall --zap --cask splat` tar dessutom bort Splats inställningar
och cacheminnen.

**Uppdateringar.** Splat från skivavbilden eller Homebrew håller sig uppdaterat med [Sparkle](https://sparkle-project.org).
Vid andra starten frågar appen om den får leta efter uppdateringar på egen hand, en gång om dagen. **Splat › Leta efter uppdateringar…**
letar direkt; **Leta efter uppdateringar automatiskt** och **Hämta och installera uppdateringar automatiskt** i samma
meny ändrar ditt val senare. Splat installerar en uppdatering bara om den bär utvecklarens signatur. En egen
build har ingen uppdateringsfunktion.

**Från källkod.** Se [Building](../../../docs/development.md#building) för förutsättningarna och kör sedan:

```sh
make build
ditto target/release/Splat.app /Applications/Splat.app
open /Applications/Splat.app
```

För att öppna Markdown-filer med Splat genom dubbelklick markerar du en valfri `.md`-fil i Finder, väljer **Arkiv › Visa info**,
väljer Splat under **Öppna med** och klickar på **Ändra alla…**. Splat registrerar sig för filerna `.md`, `.markdown`, `.mdown` och
`.mkd`.

## Komma igång

### Fönstret

Varje dokument öppnas i ett eget fönster, eller som flik om du föredrar flikar:

- **Vänster: källtexten.** Din Markdown i SF Mono med syntaxfärger.
- **Höger: förhandsvisningen.** Det renderade dokumentet, redo att redigeras.
- **Sidofält (valfritt).** Alla rubriker som ett träd, ett klick på en visar den i båda rutorna: **Innehåll › Visa
  disposition** (⌃⌘S). Två flikar till innehåller [anteckningarna i mappen](#anteckningar-i-en-mapp) och en sökning i
  dem.
- **Verktygsfält.** Knappar grupperade efter vad de ändrar: tecken, stycken och infogade element, därefter förhandsvisning på/av,
  Dela och Nytt dokument. Anpassa det med **Innehåll › Anpassa verktygsfältet…**.

Dra i avgränsaren för att ge någon sida mer utrymme. **Innehåll › Göm förhandsvisning** (⌥⌘P) lämnar bara källtexten kvar; Splat
minns detta per fönster. Båda rutorna rullar tillsammans, och en markering i den ena rutan framhävs i den andra.

Fönstrets hörn visar textens ord och tecken och hur lång tid den tar att läsa, och samma sak för markeringen så länge
det finns en. Länkadresser, HTML-taggar, metadata, kommentarer, formler och diagram räknas inte. **Innehåll › Göm antal
ord** tar bort det.

### Dokument, flikar och versioner

Splat är en vanlig dokumentapp för macOS:

- **Spara automatiskt och Versioner.** Ändringar sparas på plats. **Arkiv › Återgå till › Bläddra bland alla versioner…** tar dig tillbaka
  i tiden.
- **Ändringar från andra program.** När Git, ett skript eller en annan editor ändrar ett öppet dokument läser Splat in det på nytt
  och behåller markören och rullningsläget. Har du egna osparade ändringar behåller Splat dem, och macOS
  frågar innan den andra versionen skrivs över.
- **De vanliga filkommandona.** Duplicera (⇧⌘S), Byt namn…, Flytta till… och **Arkiv › Öppna senaste**.
- **Flikar.** **Arkiv › Ny flik** eller knappen **+** i verktygsfältet öppnar ett nytt dokument som flik; den vanliga menyn
  Fönster slår ihop, flyttar och växlar mellan flikar.
- **Fönsterplacering.** Storleken och placeringen på ditt senaste fönster minns appen, även efter omstarter. Om ett fönster
  redan står där öppnas det nya något förskjutet.
- **Dockikonen.** Ett klick på den, sedan du stängt det sista fönstret, öppnar ett nytt dokument.
- **Öppna Markdown-filer i Splat.** **Splat › Ställ in som förvald app för Markdown** gör att ett dubbelklick på en `.md`- eller
  `.markdown`-fil öppnar Splat; macOS ber om bekräftelse en gång. Alternativet har en bockmarkering så länge Splat är förvald app.

Filer måste vara UTF-8-text eller UTF-16 med byteordningsmärke. Splat vägrar andra teckenkodningar i stället för att gissa
och förstöra texten. Ett byteordningsmärke ligger kvar, UTF-16 förblir UTF-16, och i en fil där alla rader slutar med CRLF
får de rader du lägger till också CRLF.

**Efter en krasch** går som mest de senaste sekundernas skrivande förlorat: var femte sekund kopierar Splat osparad text till
`~/Library/Application Support/Splat/Unsaved` och tar bort kopiorna när ett dokument sparas eller Splat avslutas normalt.
Vid nästa start flyttas det en krasch lämnat där till `~/Library/Application Support/Splat/Recovery`, och Splat säger till om det en gång, med en
knapp som öppnar mappen. En krasch som Splat ännu hinner reagera på skriver dessutom texten i alla öppna dokument dit.

### Snabbreferens

**Hjälp › Markdown-snabbreferens** öppnar ett fusklapp på ditt språk som ett nytt, namnlöst dokument. Det är ett
vanligt dokument, så prova allt direkt där, i källtexten eller i förhandsvisningen.

## Skriva

### I källtexten

Källtexten är en ren textredigerare som är anpassad för Markdown:

- **Syntaxfärger** som i VS Code: rubriker, betoning, länkar, kod, tabeller, HTML, front matter samt LaTeX och
  Mermaid inuti formler och diagram. Det finns paletter för ljust och mörkt läge samt för Öka kontrast.
- **Kodblock med staket** färgmarkeras för Bash, C, C++, C#, CSS (även SCSS och Less), Go, HTML, XML, Java,
  JavaScript, JSON, Kotlin, PHP, Python, Ruby, Rust, SQL, Swift, TypeScript, YAML, TeX och Mermaid, även med deras
  vanliga korta namn som `js`, `py` eller `sh`.
- **Inga överraskande ersättningar.** Smarta citationstecken, smarta streck och textersättning är avstängda som standard, eftersom de
  förstör `--`, `---` och `"`. Slå på dem under **Redigera › Ersättningar**.
- **Systemets skrivverktyg.** Stavnings- och grammatikkontroll, diktering, Sök och ersätt (⌥⌘F) samt Tal.
- **Dra och släpp.** Släpp en bildfil för att infoga `![namn](relative/path.png)`, en annan fil för att infoga en länk till den
  och en URL för att infoga `[url](url)`. Varje släpp är ett ångrasteg.
- **Klistra in bilder.** En bild i urklipp, till exempel en skärmavbild, sparas som en PNG-fil bredvid dokumentet
  (`Bild 2026-10-29 13.09.07.png`) och infogas som bild. Dokumentet måste ha sparats en gång för det.

### I förhandsvisningen

Förhandsvisningen beter sig som ett ordbehandlingsprogram, men allt du gör i den hamnar som Markdown:

- **Skriva.** Tecken som betyder något i Markdown, till exempel `*` eller `#`, skyddas med omvänt snedstreck så att de visas som du skrev dem.
- **Return** påbörjar ett nytt stycke eller en ny listpunkt. **⌥Return** eller **⌃Return** infogar en radbrytning.
- **Formatering vid markören.** Står markören i slutet av ett ord gäller Fet, Kursiv och de andra teckenformaten
  det du skriver därefter. Välj formatet igen för att avsluta. Inuti ett ord formateras hela ordet.
- **Tomma rader.** Return på en tom rad lägger till ytterligare en tom rad, och backstegstangenten tar bort den. I källtexten blir varje
  extra tom rad två tomma rader till; andra Markdown-program visar dem som vanligt styckeavstånd.
- **Ångra** fungerar över båda rutorna. Ångra-åtgärderna har namn (”Ångra Fet”), skrivande är ett steg per skrivsvit, och efter Ångra
  och Gör om rullar båda rutorna till ändringen.
- **Formler och diagram** visas som färdig grafik. Klicka på en, flytta markören dit eller tryck Return på den för att
  öppna dess redigerare (se [Formulas](#formler) och [Diagrams](#diagram)).
- **Inbäddad HTML, inbäddade anteckningar och front matter** visas, men redigeras i källtexten. Ett klick på dem sätter
  markören på motsvarande ställe där.

Allt som Markdown inte kan uttrycka blockeras i förhandsvisningen: panelerna Typsnitt och Färger, linjalen samt inklistring
eller släpp av formaterad text och bilder. Inklistrad HTML och RTF kommer in som oformaterad text, om du inte väljer
[Klistra in som Markdown](#klistra-in-formaterad-text).

### Formateringskommandon

Varje kommando finns i verktygsfältet, i menyraden och i snabbmenyn i båda rutorna. Menyalternativen visar en
bockmarkering för formateringen vid markören.

| Meny | Kommandon |
|---|---|
| **Format › Typsnitt** | Fet, Kursiv, Understrykning, Genomstrykning, Markera, Kod, Upphöjd, Nedsänkt |
| **Format › Rubrik** | Brödtext, Rubrik 1 till 6 |
| **Format › Lista** | Lista i punktformat, Numrerad lista, Uppgiftslista |
| **Format** | Citat, Kodblock (Inget språk eller ett av 20 språk; ändrar även språket i ett befintligt block), Inforuta (Anteckning, Tips, Viktigt, Varning, Försiktighet; samma typ igen tar bort den), Redigera formel eller diagram…, Ta bort formatering |
| **Infoga** | Länka…, Bild…, Tabell…, Formel, Diagram (Flödesschema, Sekvensdiagram, Gantt-schema), Innehållsförteckning, Fotnot, Avgränsare |
| **Tabell** | Infoga rad ovanför/nedanför, Infoga kolumn till vänster/höger, Radera rad, Radera kolumn, Justering (Vänster, Centrera, Höger, Förval), Justera tabellkolumner |
| **Innehåll** | Göm/Visa förhandsvisning, Visa/Göm disposition, Typografiska skiljetecken, Zooma in/ut, Verklig storlek, verktygsfält, flikfält, helskärmsläge |

Understrykning, markering, upphöjd och nedsänkt text skrivs som `<ins>`, `<mark>`, `<sup>` och `<sub>`, vilket GitHub
också visar. Splat läser även `<u>`, som GitHub tar bort, och de kortare `==highlight==`, `^sup^` och `~sub~`.
GitHub visar `==` och `^` som de skrivits och `~sub~` som genomstrykning.

### Listor och uppgifter

- Return fortsätter en lista; Return på en tom listpunkt avslutar den.
- **Tab** och **⇧Tab** drar in och minskar indrag för listpunkter i förhandsvisningen.
- Ett klick på en uppgifts kryssruta i förhandsvisningen växlar mellan `- [ ]` och `- [x]`.
- **Format › Lista** gör om de markerade styckena till en lista eller byter dess slag.

### Tabeller

- **Infoga › Tabell…** frågar efter antalet rader (inklusive rubrikraden) och kolumner.
- I förhandsvisningen flyttar **Tab** till nästa cell, **⇧Tab** till föregående och **Return** till cellen nedanför. Tab
  i den sista cellen lägger till en rad.
- Menyn **Tabell** infogar och raderar rader och kolumner och anger en kolumns justering (⌘{, ⌘|, ⌘}).
- **Tabell › Justera tabellkolumner** fyller ut källtexten så att kolumnerna hamnar under varandra.

### Klistra in formaterad text

**Redigera › Klistra in som Markdown** (⌃⌘V), som också finns i snabbmenyn i båda rutorna, gör innehållet i urklipp till
Markdown vid markeringen, som ett ångrasteg. ⌘V fortsätter att infoga oformaterad text.

- **Från en webbläsare, Word, Google Docs, Confluence, Anteckningar eller Pages:** rubriker, fet, kursiv,
  understrykning, genomstrykning, upphöjd och nedsänkt text, länkar, listor med sin nästling, citat, kod och tabeller
  följer med.
- **Från ett kalkylark:** celler blir en tabell med en rubrikrad och högerjusterade kolumner med tal. Det gäller också
  text avgränsad med tabbar, semikolon eller kommatecken, om varje rad har lika många kolumner.
- **Bilder** med en webbadress förblir länkar. En bild inuti den inklistrade HTML-koden blir en fil bredvid dokumentet,
  som måste ha sparats en gång för det. Words tillfälliga bildfiler följer inte med, men deras alternativtext finns
  kvar.
- **Länkar** behåller bara adresser med `http`, `https`, `mailto`, relativa adresser och `#`-adresser; av alla andra
  länkar finns bara texten kvar.

### Metadata

**Format › Redigera metadata…**, eller ett klick på metadataraden högst upp i förhandsvisningen, öppnar ett formulär med
fälten i YAML-front matter. Fält kan ändras, läggas till och tas bort. Ett fält med en lista behåller den som lista:
värdena står efter varandra med kommatecken emellan, och ett värde som innehåller ett kommatecken står inom citattecken.
Ett nytt fält som heter `tags`, `aliases`, `keywords`, `categories`, `authors` eller `cssclasses` blir också en lista.
Nästlade fält och text på flera rader går att se där och redigeras i källtexten.

Det formuläret ändrar är ett ångrasteg och når bara raderna för de fält du har ändrat: kommentarer, ordningen, udda
mellanrum, citattecken och radslut i alla andra förblir identiska byte för byte. Ett dokument utan metadata får ett
block i början.

## Markdown som stöds

Splat tolkar med [comrak](https://github.com/kivikakk/comrak), en parser som är kompatibel med CommonMark och GFM, och slår på
de flesta av dess tillägg. Allt nedan renderas i förhandsvisningen och i alla exporter.

### CommonMark och GitHub Flavored Markdown

- Rubriker (`#` och understrukna), stycken, radbrytningar, betoning, stark betoning, kod i löpande text, blockcitat,
  avgränsare.
- Punktlistor, numrerade och nästlade listor; uppgiftslistor.
- Länkar (i texten, referenser och automatiska), bilder, definitioner av länkreferenser.
- Kodblock med staket och indragna kodblock med syntaxfärger.
- Tabeller med kolumnjustering.
- Genomstrykning med `~~two tildes~~`.
- Fotnoter: `text[^1]` med `[^1]: The note.` var som helst i dokumentet. Ett klick på markören hoppar till fotnoten.
- YAML front matter mellan `---`-rader överst. Förhandsvisningen sammanfattar den på en rad, till exempel
  ”Metadata: title, date, tags”.

### Inforutor

GitHub-alerts renderas som färgade rutor med ikon och titel:

```markdown
> [!NOTE]
> Användbar information som användare bör känna till.

> [!WARNING]
> Kritiskt innehåll som kräver omedelbar uppmärksamhet.
```

De fem typerna är Anteckning, Tips, Viktigt, Varning och Försiktighet. **Format › Inforuta** gör om stycket vid markören
till en sådan.

Obsidians inforutor fungerar också, med valfri titel och utfällningsläge:

```markdown
> [!tip]- Klicka på ▸ för att fälla ut
> Dold tills den öppnas.

> [!question]+ Öppen från början
> Ett `+` börjar utfälld, ett `-` hopfälld.
```

Obsidians övriga typer visas i den närmaste av de fem färgerna: `success`, `check`, `done` och `hint` som Tips;
`question`, `help` och `faq` som Viktigt; `attention` som Varning; `failure`, `fail`, `missing`, `danger`, `error` och
`bug` som Försiktighet; allt annat som Anteckning.

### Utökad syntax

| Funktion | Syntax | Från |
|---|---|---|
| Markering | `==marked text==` | Obsidian, Typora |
| Upphöjd text | `x^2^` | Pandoc, Typora |
| Nedsänkt text | `H~2~O` | Pandoc, Typora |
| Emoji-kortkod | `:rocket:` blir 🚀 | GitHub |
| Spoiler | `\|\|hidden text\|\|`, markera den för att läsa | Discord |
| Fotnot i texten | `text^[the note itself]` | Pandoc, Obsidian |
| Definitionslista | `Term`, tom rad, `: Explanation` | PHP Markdown Extra, Pandoc |
| Rubrikankare | `## Install {#setup}`, länka med `[see](#setup)` | Pandoc, kramdown |

Definitionslistor ser ut så här i källtexten:

```markdown
Begrepp

: Förklaring av begreppet.
```

Utan eget ankare får varje rubrik ett som härleds ur texten, som på GitHub: `## Komma igång` nås som `#komma-igång`.

### Obsidian

- **Wikilänkar.** `[[Anteckning]]` länkar till `Anteckning.md` i samma mapp eller en mapp längre ned. `[[Anteckning|Visad text]]` visar
  din egen text, och `[[Anteckning#Rubrik]]` öppnar anteckningen. ⌘-klick öppnar den i Splat.
- **Inbäddningar.** `![[Anteckning]]` visar den andra anteckningen inuti denna; anteckningar som är inbäddade i den visas som länkar.
  `![[diagram.png]]` visar en bild. Som i Obsidian pekar inbäddningar ut anteckningar och bilder i dokumentets mapp eller längre ned,
  aldrig med en absolut sökväg, och bara Markdown-filer visas som anteckningar.
- **Kommentarer.** `%%Bara i källtexten%%` förblir synlig i källtexten och utelämnas ur förhandsvisningen och alla exporter,
  både i löpande text och på egna rader.
- **Taggar.** `#idea` eller `#project/splat` markeras. ⌘-klick söker efter taggen i dokumentet och, i sidofältet, i hela
  mappen.
- **Hopfällbara inforutor.** Se [Callouts](#inforutor).

### Citat och referenser

- **Pandoc-citat.** `[@doe2020]` eller `[@doe2020; @roe2021]` visas som ”(Doe 2020; Roe 2021)”. Posterna hämtas
  från den BibTeX-fil som front matter anger som `bibliography: refs.bib`. Utan den använder Splat
  `references.bib`, `literatur.bib` eller `bibliography.bib` bredvid dokumentet.
- **Referenser till GitHub och GitLab.** Om dokumentet ligger i ett Git-arkiv vars fjärrarkiv `origin` finns på github.com
  eller en GitLab-värd markeras `@name`, `#12` (ärende) och `!7` (merge request, bara GitLab). ⌘-klick öppnar dem
  i webbläsaren.

### Inbäddad HTML

HTML som den ser ut i GitHubs README-filer renderas, med en egen läsare i stället för en webbläsarmotor:

- centrerade stycken och block (`<p align="center">`, `<div align="center">`)
- bilder med storlek (`<img src="logo.png" width="120">`)
- länkar, `<kbd>⌘</kbd>`, `<br>`, `<u>`, `<ins>`, `<mark>`, `<sup>`, `<sub>`
- listor och tabeller, även celler som spänner över flera kolumner (`colspan`)
- hopfällbara avsnitt: `<details><summary>Mer</summary>…</details>`, som öppnas med ett klick på ▸

Markdown mellan HTML-taggar renderas som Markdown. Skript, stilar, ramar och formulär utelämnas helt, och länkar
och bilder behåller bara adresser med `http`, `https`, `mailto` samt relativa adresser.

### Typografiska skiljetecken

**Innehåll › Typografiska skiljetecken** visar raka citationstecken som citattecknen i ditt gränssnittsspråk, `--` som
tankstreck (kort), `---` som långt tankstreck och `...` som tre punkter. På tyska blir `"Ja"` „Ja“, på franska «Oui», på japanska
「はい」, också där kinesiska och japanska sätter citattecken utan mellanslag; hebreiska förkortningar som `צה"ל` får sina
gershayim, צה״ל. Källtexten behåller de enkla tecknen; förhandsvisningen och alla exporter visar de typografiska. Inställningen
är avstängd tills du slår på den.

### Syntaxöversikt

| Element | Markdown |
|---|---|
| **Fet** | `**Fet**` |
| *Kursiv* | `*Kursiv*` |
| Understrykning | `<ins>Understrykning</ins>` eller `<u>Understrykning</u>` |
| ~~Genomstrykning~~ | `~~Genomstrykning~~` |
| Markering | `<mark>Markering</mark>` eller `==Markering==` |
| `Kod` | `` `Kod` `` |
| x² | `x<sup>2</sup>` eller `x^2^` |
| H₂O | `H<sub>2</sub>O` eller `H~2~O` |
| [Länk](https://example.com) | `[Länk](https://example.com)` |
| Rubrik | `# Rubrik` till `###### Rubrik` |
| Citat | `> Citat` |
| Punktlista | `- Punkt` |
| Numrerad lista | `1. Punkt` |
| Uppgift | `- [ ] Öppen` och `- [x] Klar` |
| Kodblock | ```` ```rust ```` … ```` ``` ```` |
| Tabell | `\| A \| B \|` med `\|---\|---\|` under rubrikraden |
| Fotnot | `Text[^1]` och `[^1]: Anmärkning` |
| Avgränsare | `---` |
| Bild | `![Alternativtext](picture.png)` |
| Formel | `$E = mc^2$`, `$$ … $$` |
| Diagram | ```` ```mermaid ````, ```` ```dot ````, ```` ```plantuml ```` |
| Inforuta | `> [!TIP]` |
| Wikilänk | `[[Anteckning]]`, `[[Anteckning\|Text]]` |
| Inbäddning | `![[Anteckning]]`, `![[image.png]]` |
| Kommentar | `%%Kommentar%%` |
| Tagg | `#tagg` |
| Citat (källa) | `[@key]` |
| Spoiler | `\|\|Spoiler\|\|` |
| Emoji | `:tada:` |

## Formler

Formler renderas av [MathJax](https://www.mathjax.org) med dess TeX- och AMS-matematik, som skarp vektorgrafik. Text i
`\text{…}` kan vara på vilket skriftsystem som helst: det som MathJax typsnitt saknar hämtas från typsnitten på din Mac, med arabiska och hebreiska
från höger till vänster, och emoji i färg.

| Slag | Syntax |
|---|---|
| I texten | `$E = mc^2$`, `` $`E = mc^2`$ `` (GitHub, GitLab), `\(E = mc^2\)` |
| Fristående | `$$` … `$$`, `\[` … `\]`, eller ett kodblock markerat `math` |

**Redigera en formel.** Klicka på en formel i förhandsvisningen, dubbelklicka på den, tryck Return på den eller flytta markören dit
för att öppna dess redigerare i en popover. **Infoga › Formel** infogar en ny och öppnar redigeraren direkt.

- **Visuell eller LaTeX.** Omkopplaren överst växlar mellan en visuell redigerare ([MathLive](https://cortexjs.io/mathlive/))
  och LaTeX-koden. Splat minns ditt val.
- **Byggblock** för bråk, kvadratrot, upphöjd och nedsänkt text, summa och integral med gränser, gränsvärde,
  parenteser som växer med innehållet, 2×2-matris, grekiska bokstäver samt relationer, operatorer och pilar. De lämnar
  platshållare □, och **Tab** hoppar till nästa.
- **Direkt förhandsvisning.** Ändringar syns i dokumentet medan du skriver. Fel från MathJax visas på en rad under koden.
- **Spara eller förkasta.** Ett klick utanför behåller ändringen som ett enda ångrasteg; **Esc** eller **Avbryt** kastar bort den.
  Esc på en ny, tom formel tar bort den igen.
- Snabbmenyn erbjuder **Redigera formel…** för formeln under pekaren.

VoiceOver läser upp en formel som dess LaTeX-kod. Splat renderar formler, inte kompletta LaTeX-dokument: det finns inga
paket, ingen TikZ och inget `\documentclass`.

## Diagram

Diagram renderas inuti Splat, offline och i en ljus och en mörk variant.

| Motor | Kodblock | Vad den ritar |
|---|---|---|
| [Mermaid](https://mermaid.js.org) | ```` ```mermaid ```` | flödesscheman, sekvens-, klass-, tillstånds-, entitetsrelations-, Gantt-, cirkeldiagram, tankekartor, tidslinjer och alla andra Mermaid-typer |
| [Graphviz](https://graphviz.org) | ```` ```dot ```` eller ```` ```graphviz ```` | grafer i DOT-språket |
| [PlantUML](https://plantuml.com) | ```` ```plantuml ```` eller ```` ```puml ```` | UML-diagram; körs utan Java |

```mermaid
flowchart LR
    A[Markdown] --> B[Förhandsvisning]
    B --> C[PDF]
```

**Att redigera ett diagram** fungerar som att redigera en formel: klicka på det för att öppna koden i en popover, med syntaxfärger och
felmeddelanden på en rad under koden. **Infoga › Diagram** utgår från en mall för ett flödesschema, ett sekvensdiagram
eller ett Gantt-schema. Byggblock hjälper till med de tre vanligaste Mermaid-typerna:

- **Flödesschema:** Nod ▾ (rektangel, rundad, cirkel, romb, databas), Koppling, Koppling med etikett, Grupp,
  Riktning ▾
- **Sekvensdiagram:** Deltagare, Meddelande, Svar, Anteckning, Slinga, Alternativa flöden
- **Gantt-schema:** Sektion, Uppgift, Milstolpe

Andra diagramtyper redigeras som kod. VoiceOver läser upp ett diagrams titel eller alternativtext.

Det första diagrammet efter start tar cirka en sekund, eftersom renderingsmotorn startar i bakgrunden. Ett diagram
som tar längre tid än 10 sekunder visar ett fel, och renderingsmotorn startar om för nästa.
PlantUML:s motor (3 MB) läses in först med det första PlantUML-diagrammet. En PlantUML-`!include` från webben blockeras.

## Bilder

- **Lokala bilder** använder sökvägar relativa till dokumentet: `![Logo](images/logo.png)`. Splat visar PNG, JPEG, GIF, WebP,
  BMP, SVG, HEIC, TIFF, AVIF och PDF.
- **Bilder från webben** förblir blockerade tills du tillåter dem: ett fält ovanför förhandsvisningen förklarar att när de läses in får
  deras servrar veta att och när du öppnar dokumentet. **Läs in bilder** tillåter dem för det här dokumentet; Splat minns
  valet per fil.
- **Storlek.** `<img width="200">` anger en bilds storlek; annars anpassas bilder efter förhandsvisningens bredd.
- **Ändringar på disk.** En bild som du byter ut, eller en som dyker upp sedan dokumentet nämnt den, visas utan att dokumentet
  öppnas igen. Bilder från webben som misslyckades på grund av nätverket provas på nytt medan ett dokument visar
  dem.
- **Minne.** Bilder visas med upp till 2560 pixlar på den längsta sidan, vilket också fyller en A4-sida. De avkodas i mindre storlek
  när de fyller en åttondel av Macens minne, och så länge macOS rapporterar minnesbrist.
- **Mörkt läge.** Bilder med vit bakgrund dämpas något i den mörka förhandsvisningen.
- **Klistra in och släppa.** Se [In the source](#i-källtexten).

## Länkar och navigering

I förhandsvisningen placerar ett klick markören så att du kan redigera länktexten; **⌘-klick** följer länken:

- `http`, `https` och `mailto` öppnas i din webbläsare eller e-postapp.
- Markdown-filer öppnas i Splat; andra filer och mappar visas i Finder.
- `#anchor` hoppar till rubriken i dokumentet.
- Adresser som öppnar en annan app, till exempel `slack://` eller `vscode://`, frågar först och visar den fullständiga adressen och appen.
  **Avbryt** är förvalt.

**Infoga › Innehållsförteckning** infogar en länkad lista över alla rubriker. Står markören i den uppdaterar samma kommando
den. Dispositionspanelen (⌃⌘S) hoppar till vilken rubrik som helst i båda rutorna.

## Anteckningar i en mapp

Knapparna högst upp i sidofältet växlar mellan dispositionen, filerna och en sökning.

- **Filer** (**Innehåll › Visa filer**, ⌃⌘E). Alla Markdown-filer i dokumentets mapp eller, om dokumentet ligger i ett
  Git-arkiv, i hela arkivet. Ett klick eller Return öppnar en anteckning som en flik i det här fönstret; är den redan
  öppen kommer den längst fram. Knappen bredvid mappens namn visar en annan mapp i det här fönstret. Dolda filer,
  `node_modules`, `target`, `Pods`, `DerivedData`, `__pycache__` och mappar utan anteckningar hålls utanför. Ett
  dokument som ligger direkt i din hemmapp visar bara anteckningarna bredvid sig, så att Splat inte går igenom alla dina
  mappar och frågar om var och en.
- **Sök** (**Redigera › Sök › Sök i alla anteckningar…**, ⇧⌘F). Hittar text i alla anteckningar, oavsett versaler och
  gemener och oavsett hur accenterna är kodade. `#tag` hittar en tagg, `#project` även `#project/splat`, och inget inuti
  kod. Ett klick på en träff öppnar anteckningen på det stället.
- **Länkar till en anteckning.** Snabbmenyn för en anteckning visar de anteckningar som länkar till den, med wikilänk,
  inbäddning, relativ länk eller definition av en länkreferens.
- **Byta namn på en anteckning.** Samma meny byter namn på den och ändrar de länkarna. Splat säger först hur många
  länkar i hur många filer det gäller. Bara namnet ändras i varje länk, och alla andra byte i filen ligger kvar. Öppna
  dokument tar emot ändringen som ett ångrasteg; i andra filer kan bara Git eller Versioner ta tillbaka den. En fil som
  ett annat program har ändrat under tiden stoppar namnbytet av alla länkar, och Splat talar om vilken.

Listan följer mappen när fönstret kommer längst fram. Sökningen tittar på filerna som de ligger på disken, och text du
ännu inte har sparat räknas från nästa automatiska sparning.

## Ändringar sedan senaste commit

I ett dokument i ett Git-arkiv markerar streck bredvid texten det som skiljer sig från senaste commit, i källtexten och
i förhandsvisningen: grönt för tillagda rader, blått för ändrade rader och ett kort rött märke där något har tagits
bort. De följer med medan du skriver och när grenen flyttas. **Innehåll › Ändringar sedan senaste commit** stänger av
dem i alla fönster.

Rader som bara skiljer sig i sitt radslut räknas inte, så en fil med CRLF mot en commit med LF visar ingen ändring. En
ny fil har inga markeringar före sin första commit, och det har inte heller ett dokument över 32 MB.

## Exportera, skriva ut och dela

| Format | Så här | Det här får du |
|---|---|---|
| **PDF** | **Arkiv › Exportera som PDF…** eller menyn Dela | Sidor i pappersformat, alltid i ljust utseende, formler och diagram som vektorgrafik. **Arkiv › Utskriftsformat…** anger papperet. |
| **Utskrift** | **Arkiv › Skriv ut…** (⌘P) | Samma layout som PDF-filen. |
| **HTML** | **Arkiv › Exportera som HTML…** eller menyn Dela | En fristående fil: lokala bilder inbäddade, formler och diagram som skarpa bilder, rubrikankare så att innehållsförteckningen fungerar. Inga skript. |
| **Word** | **Arkiv › Exportera som Word-dokument…** eller menyn Dela | En `.docx` med Words rubrikformat, riktiga listor och tabeller (även sammanslagna celler), fotnoter som Word-fotnoter, inbäddade bilder, formler och diagram. |
| **Markdown** | Knappen **Dela** i verktygsfältet | Dokumentet som en `.md`-fil för Mail, Meddelanden, AirDrop och alla andra delningstjänster. |

Menyn **Arkiv › Dela** erbjuder alla dessa på ett ställe. Exporterna använder inställningen för typografiska skiljetecken och de
bilder från webben som du tillåtit; bilder från webben förblir länkar i HTML.

**Arkiv › Exportalternativ…** anger vad PDF, utskrift och Word lägger till i texten, för alla dokument:

- **Sidnummer** i sidfoten i PDF och utskrift, som `2 / 9`.
- **Sidhuvud och sidfot** för PDF och utskrift. `{title}`, `{author}`, `{date}`, `{page}` och `{pages}` står för sina
  värden, de tre första från dokumentets metadata.
- **En titelsida** för PDF, utskrift och Word, med `title`, `subtitle`, `author` och `date` från metadata, eller
  dokumentets namn som titel. Den har varken sidhuvud eller sidfot.
- **En Word-mall**, en `.docx` eller `.dotx` vars utseende exporten tar över: dess format, även med lokaliserade namn,
  dess tema, pappersstorlek, marginaler, sidhuvuden och sidfötter. En mall som inte går att läsa stoppar exporten och
  säger det, i stället för att ge dig ett dokument utan den.

## Importera Word-dokument

**Arkiv › Importera Word-dokument…** frågar efter en `.docx`- eller `.dotx`-fil och vart dess Markdown ska, och öppnar
sedan resultatet. Bilder hamnar i en mapp bredvid, namngiven efter dokumentet med `-images`.

Rubriker och listor kommer igenom så som Words format och numrering definierar dem, även i dokument vars format har
lokaliserade namn. Fet, kursiv, understrykning, genomstrykning, färgmarkering, upphöjd och nedsänkt text, länkar, citat,
kod, fotnoter, slutnoter och tabeller följer med, celler som sträcker sig över kolumner som en HTML-tabell. Formler blir
LaTeX. Spårade ändringar räknas som godkända, och en innehållsförteckning hålls utanför, eftersom **Infoga ›
Innehållsförteckning** gör en ny. Gamla `.doc`-filer och lösenordsskyddade dokument avvisas.

## Utseende och tillgänglighet

- **Mörkt läge** följer systemet. Formler och diagram renderas för varje utseende.
- **Öka kontrast** i Systeminställningar växlar syntaxfärgerna till paletter med hög kontrast.
- **Zoom.** ⌘+, ⌘− och ⌘0 skalar båda rutorna.
- **Helskärmsläge, flikar och ett anpassningsbart verktygsfält**, som i alla Mac-appar.
- **VoiceOver.** Rutorna heter ”Markdown-källtext” och ”Förhandsvisning”, formler läses upp som sin kod och diagram som
  sin titel. Varje kommando går att nå från tangentbordet.

Splat har inget inställningsfönster: de få alternativen finns där de gäller, i **Innehåll**, **Redigera › Ersättningar**
och **Arkiv › Exportalternativ…**.

## Språk

Gränssnittet, menyerna och Markdown-snabbreferensen är översatta till 35 språk:

arabiska, katalanska, kinesiska (förenklad), kinesiska (traditionell), kroatiska, tjeckiska, danska, nederländska, engelska, finska, franska,
tyska, grekiska, hebreiska, hindi, ungerska, indonesiska, italienska, japanska, koreanska, malajiska, norska, polska, portugisiska
(Brasilien), portugisiska (Portugal), rumänska, ryska, slovakiska, slovenska, spanska, svenska, thailändska, turkiska, ukrainska och
vietnamesiska.

Splat följer språkordningen under **Systeminställningar › Allmänt › Språk och region**; där kan du också välja ett språk
enbart för Splat under **Appar**. Saknas en översättning till ditt språk används engelska. Menytermerna
följer Apples egna översättningar, så Splat låter som resten av din Mac.

Hebreisk och arabisk text läses från höger till vänster överallt där den förekommer, oavsett gränssnittsspråk: varje stycke,
rubrik och listpunkt tar riktningen från sin första bokstav, citat och inforutor ritar sitt streck till höger, och en
tabell vars text börjar från höger till vänster listar sina kolumner från höger. Kodblock förblir från vänster till höger. PDF-, HTML-
och Word-exporterna behåller dessa riktningar. Med hebreiska eller arabiska som systemspråk speglar macOS också fönstret,
så förhandsvisningen hamnar till vänster.

## Kortkommandon

| Åtgärd | Kortkommando |
|---|---|
| Fet, Kursiv, Understrykning | ⌘B, ⌘I, ⌘U |
| Genomstrykning | ⇧⌘X |
| Länka… | ⌘K |
| Brödtext, Rubrik 1 till 6 | ⌥⌘0, ⌥⌘1 till ⌥⌘6 |
| Justera tabellkolumn vänster, centrerat, höger | ⌘{, ⌘\|, ⌘} |
| Göm eller visa förhandsvisningen | ⌥⌘P |
| Visa eller göm dispositionen | ⌃⌘S |
| Visa eller göm filerna | ⌃⌘E |
| Sök i alla anteckningar | ⇧⌘F |
| Klistra in som Markdown | ⌃⌘V |
| Zooma in, ut, verklig storlek | ⌘+, ⌘−, ⌘0 |
| Nytt dokument, öppna, spara, duplicera | ⌘N, ⌘O, ⌘S, ⇧⌘S |
| Utskriftsformat, skriv ut | ⇧⌘P, ⌘P |
| Sök, sök och ersätt | ⌘F, ⌥⌘F |
| Sök nästa, föregående; använd markering; gå till markering | ⌘G, ⇧⌘G; ⌘E; ⌘J |
| Visa stavning och grammatik; kontrollera dokument | ⌘:, ⌘; |
| Klistra in och matcha stilen | ⌥⇧⌘V |
| Göm verktygsfältet, visa alla flikar, helskärm | ⌥⌘T, ⇧⌘\\, ⌃⌘F |
| Radbrytning (förhandsvisning) | ⌥Return eller ⌃Return |
| Nästa cell / föregående cell i en tabell (förhandsvisning) | Tab / ⇧Tab |
| Öka / minska indrag för en listpunkt (förhandsvisning) | Tab / ⇧Tab |
| Nästa platshållare i formelredigeraren | Tab |
| Stäng formel- eller diagramredigeraren utan ändringar | Esc |
| Följ en länk, tagg eller referens (förhandsvisning) | ⌘-klick |

Splat lämnar Apples standardkortkommandon som de är.

## Integritet och säkerhet

En Markdown-fil kan komma från vem som helst, så Splat behandlar varje dokument som opålitligt.

- **Inget nätverk som standard.** Splat har ingen telemetri. Appen letar efter uppdateringar först när du går med på det eller väljer **Leta efter
  uppdateringar…**: den läser då in en liten flödesfil från GitHub, och förfrågan innehåller bara appens namn och version.
  Formler och diagram renderas lokalt; webbvyerna som gör det kan inte nå nätverket. **Hjälp › Rapportera ett problem…** öppnar ett formulär på GitHub i webbläsaren; adressen innehåller versionen av Splat och macOS och typen av Mac, inget från dina dokument.
- **Bilder från webben** läses in först efter ditt tillstånd, bara över `https`, aldrig från värdar i ditt lokala nätverk,
  högst 10 MB vardera och i en privat session utan cookies.
- **Isolerad avkodning.** Bilder och grafiken i formler och diagram avkodas i hjälpprocesser som stänger
  varje fil de ärvt och låser in sig i en sandlåda innan de läser den första byten: ingen filåtkomst, inget
  nätverk. De lämnar bara tillbaka pixlar, vars storlek kontrolleras, och stoppas efter 10 sekunder. Apples egna bildavkodare
  får bara se HEIC, TIFF, AVIF och PDF, och bara när datan verkligen har det formatet; de körs i en egen hjälpprocess, som dessutom får
  läsa systemets typsnittsmappar och använda systemets videoavkodare. SVG-filer, formler
  och diagram passerar också den hjälpprocessen, så de är bilder med fast upplösning i Splat: fyra pixlar per punkt,
  upp till 2560 på den längre sidan.
- **Lokala filer** når ett dokument bara via den sökväg det anger. Inbäddningar visar Markdown-anteckningar från dokumentets mapp
  och längre ned, och en saknad bild eftersöks bara där. En bildsökväg någon annanstans, absolut eller med början på
  `~`, läses fortfarande in i förhandsvisningen; HTML- och Word-exporter, utskrifter och PDF:er tar bara med det som
  finns i dokumentets mapp eller dess Git-arkiv, såvida inte det arkivet också innehåller din hemmapp, så att ett
  dokument från någon annan inte kan dra in dina filer i en export som du skickar vidare.
- **Mappar med anteckningar** läses bara medan sidofältet visar dem, och ett namnbyte på en anteckning ändrar andra
  filer först efter att du har bekräftat hur många.
- **Git-arkiv** läses av Splat självt. Appen kör aldrig programmet `git`, vars konfiguration i ett arkiv från någon
  annan kan starta andra program, och läser inget mer än det öppna dokumentets version i senaste commit. Den skriver
  inget där och går inte ut på nätverket.
- **Word-dokument och inklistrad text** läses av Splats egna parsrar, med gränser för storlek, nästling och hur mycket
  en fil får packa upp sig. Länkar i dem behåller bara de adresser Splat tillåter i övrigt, och namnen på bilder i en
  Word-fil avgör aldrig var de skrivs.
- **Kopior av osparad text** stannar på den här Macen, i din Library-mapp, för återställning efter en krasch. De försvinner
  vid sparning eller när Splat avslutas normalt; de kopior en krasch lämnat kvar ligger kvar i återställningsmappen tills du raderar
  dem.
- **Länkar** startar aldrig ett program med ett klick. Webb- och e-postlänkar öppnas som vanligt; länkar till andra appar frågar först.
- **HTML i dokument** läses av Splats egen parser. Skript, stilar, ramar och formulär körs aldrig.
- **Exporter** innehåller inga skript. HTML-sidor behåller bara en fast lista över ofarliga taggar och attribut, som GitHub gör, plus
  en innehållssäkerhetspolicy. Word-filer länkar bara adresser med `http`, `https` och `mailto`.
- **Kodsignering** med Hardened Runtime. Splat körs utan App Sandbox så att appen kan visa bilder bredvid
  dina dokument utan att fråga om varje mapp; isoleringen av all avkodning väger upp det.

## Stora dokument

Splat förblir responsivt med dokument på flera megabyte:

- Källtexten använder TextKit 2, som bara lägger ut den synliga delen.
- Medan du skriver i förhandsvisningen, och i källtexten i dokument upp till 2 MB, tolkas, färgläggs och läggs bara de
  berörda styckena ut på nytt. I ett dokument på 10 MB tar tolkning och färgläggning cirka 3 ms, rendering cirka 10 ms
  och en hel tangenttryckning i dess förhandsvisning cirka 30 ms med vyns eget arbete.
- Stora förhandsvisningar renderar den synliga delen först och resten medan appen är overksam. Block som rullas in i bild renderas
  innan de ritas.
- I källtexten i större dokument sker tolkningen för förhandsvisningen i bakgrunden medan du skriver.
- Ett dokument förblir ren text när det skulle kräva ungefär 4 GB minne, vilket motsvarar cirka 50 till 120 MB vanlig
  Markdown men bara 10 till 20 MB om det enbart består av tabeller eller rubriker: källtexten har inga färger och
  förhandsvisningen förklarar varför den förblir tom. Exporter fungerar fortfarande.
- Från cirka 200 KB öppnar ett dokument sitt fönster innan det tolkats; vid 5 MB följer färgerna och förhandsvisningen
  cirka en kvarts sekund senare.
- Alla bilder tillsammans, inklusive formler och diagram, tar högst en fjärdedel av Macens minne och hälften av det
  medan minnet är knappt. Utöver det visas ytterligare bilder som platshållare tills det finns plats igen.

Vissa ändringar behandlas i sin helhet, med cirka 300 ms per tangenttryckning vid 10 MB: ändringar i en fotnots definition eller
hänvisningar, intill en fotnot i texten, allra sist i ett dokument intill en fotnot, i front matter eller i
definitioner av länkreferenser, och varje ändring i ett dokument med `%%`-kommentarsblock eller med en fotnot som definierats två gånger
eller inuti ett citat eller en lista. Minnet växer med filen, till ungefär 115–160 MB för 1 MB Markdown och 340–510 MB för 10 MB.

## Kända begränsningar

- **Enkla tilden betyder nedsänkt text.** `~text~` är nedsänkt text som i Pandoc; genomstrykning kräver `~~two~~`. GitHub
  stryker också över med en enda tilde.
- **Skrivskyddat i förhandsvisningen:** inbäddad HTML och inbäddade anteckningar redigeras i källtexten, front matter i
  sitt formulär eller i källtexten.
- **Mycket djup nästling:** HTML som är nästlad djupare än 64 nivåer, och HTML i löpande text djupare än 16, ger vika för sitt innehåll.
- **Sammanslagna tabellceller** finns bara i HTML-tabeller, och bara över kolumner (`colspan`, inget `rowspan`). MultiMarkdowns
  tabellsyntax för sammanslagna celler stöds inte.
- **Litteraturlistor** läses bara från BibTeX, inte från CSL JSON eller YAML.
- **Referenser** kräver ett arkiv på github.com eller en GitLab-värd; `!7` finns bara på GitLab.
- **Typografiska skiljetecken:** ett tyskt avslutande enkelt citattecken kan inte skiljas från en apostrof och sätts som ’.
- **Formler, inte dokument:** inga LaTeX-paket, ingen TikZ och inga fullständiga dokument.
- **Den visuella formelredigeraren** skriver LaTeX i MathLives notation: mellanslag försvinner (`E = mc^2` blir `E=mc^2`),
  renderingen förblir densamma.
- **Antal ord:** thailändska, laotiska, khmer och burmesiska skrivs utan mellanslag och räknas per textavsnitt, inte per
  ord. `3.14` och `snake_case` är två ord vardera.
- **Namnbyte på en anteckning** ändrar inte länkar i inbäddad HTML, och en wikilänk förblir som den är om det nya namnet
  innehåller `#`, `|`, `^`, `[`, `]` eller `\`.
- **Ändringsmarkeringar** läser arkiv med SHA-1-objekt och vanliga eller packade referenser; ett arkiv med SHA-256 eller
  reftable visar inga. En fil under ett Git-filter som LFS jämförs med det filtret har lagrat.
- **Word-import** tar inte med text i sidhuvuden, sidfötter och kommentarer, och bilder som `.emz` eller SVG behåller
  bara sin alternativtext.
- **Klistra in som Markdown** läser RTF som västerländsk text och hämtar rubriker där bara från teckenstorlekar. De
  flesta program erbjuder också HTML, som den föredrar.
- **Av design:** inga radnummer, inget inställningsfönster, ingen Help Book, inte i App Store.
- **Mindre testat på Intel.** Fönstertesterna körs bara på Apple Silicon; på Intel kör CI `cargo test`.

## Felsökning

**Listan över senaste dokument är tom efter att jag byggt om Splat.** macOS knyter senaste dokument och mappbehörigheter till
appens kodsignatur. Lokala byggen signeras med certifikatet ”Splat Local Signing” i din inloggningsnyckelring,
så ombyggen behåller båda. Om certifikatet har raderats skapar nästa build ett nytt och listorna börjar om en gång.

**Appen från skivavbilden visar andra senaste dokument än min egen build.** De två har olika signaturer,
så macOS för en separat lista och separata mappbehörigheter för vardera.

**HTML eller annan syntax visas som vanlig kod.** En föråldrad kopia av Splat kan ha öppnat dokumentet. Avsluta den,
välj **Arkiv › Visa info** på dokumentet i Finder och kontrollera att **Öppna med** anger Splat i din
mapp Appar.

**Det finns ingen Leta efter uppdateringar… i Splat-menyn.** Bara versionen från skivavbilden eller Homebrew uppdaterar
sig själv; en egen build har ingen uppdateringsfunktion.

**Bilder från webben visas inte.** Klicka på **Läs in bilder** i fältet ovanför förhandsvisningen. Bara `https`-bilder från
offentliga värdar läses in.

**När jag klistrar in en skärmavbild ombeds jag spara först.** Inklistrade bilder sparas som filer bredvid dokumentet, vilket kräver en
plats på disken. Spara det och klistra sedan in igen.

**”Filen är inte UTF-8-text.”** Filen har en annan teckenkodning och saknar byteordningsmärke som skulle ange den.
Konvertera den först, till exempel med `iconv -f ISO-8859-1 -t UTF-8 old.md > new.md`.

**En formel eller ett diagram visar ett fel.** Klicka på det: redigeraren visar meddelandet från MathJax, Mermaid, Graphviz eller
PlantUML under koden.

## För utvecklare

Bygge, testning, kodens uppbyggnad och de regler den följer beskrivs i [docs/development.md](../../../docs/development.md) (på engelska).

### Komponenter från tredje part

| Komponent | Version | Används för | Licens |
|---|---|---|---|
| [comrak](https://github.com/kivikakk/comrak) | 0.59 med [en ändrad rad](../../../vendor/comrak/PATCH.md) | Markdown-parser | BSD-2-Clause |
| [objc2](https://github.com/madsmtm/objc2), objc2-foundation, block2 | 0.6 / 0.3 | Objective-C-körtid, Foundation, block | MIT |
| objc2-app-kit, objc2-web-kit, dispatch2 | 0.3 | bindningar till AppKit, WebKit och Grand Central Dispatch | Zlib, Apache-2.0 eller MIT |
| [svg2pdf](https://github.com/typst/svg2pdf) | 0.13 | formler och SVG-bilder som vektor-PDF | MIT eller Apache-2.0 |
| [image](https://github.com/image-rs/image) | 0.25 | avkodning av PNG, JPEG, GIF, WebP och BMP | MIT eller Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2 | justering av tabellkolumner | MIT eller Apache-2.0 |
| [entities](https://github.com/p-jackson/entities) | 1 | namnen på HTML-teckenreferenser | MIT |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide) | 0.9 | uppackning av Word-filer och Git-objekt | MIT, Zlib eller Apache-2.0 |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.21 | läsning av XML:en i Word-filer | MIT eller Apache-2.0 |
| [unicode-normalization](https://github.com/unicode-rs/unicode-normalization) | 0.1 | namn och sökning över sammansatta och uppdelade bokstäver | MIT eller Apache-2.0, Unicode-3.0 |
| [emojis](https://github.com/rossmacarthur/emojis) | 0.9 | håller text som liknar `:shortcode:` bokstavlig i importerad text | MIT eller Apache-2.0, Unicode-3.0 |
| [MathJax](https://www.mathjax.org) | 4.1.3 | rendering av formler | Apache-2.0 |
| [MathLive](https://cortexjs.io/mathlive/) | 0.111.4 | visuell formelredigerare, med KaTeX-typsnitt | MIT |
| [Mermaid](https://mermaid.js.org) | 12.3.0 | Mermaid-diagram | MIT |
| [Viz.js](https://github.com/mdaines/viz-js) | 3.24.0 | Graphviz kompilerat till WebAssembly, innehåller Graphviz (EPL-1.0) och Expat (MIT) | MIT |
| [PlantUML](https://github.com/plantuml/plantuml) (`@plantuml/core`) | 1.2026.9 | PlantUML-diagram, kompilerade till JavaScript | MIT |
| [Sparkle](https://sparkle-project.org) | 2.10.1 | uppdateringar av skivavbildsversionen | MIT, med delar under BSD- och zlib-licenser |

JavaScript-biblioteken lagras i `resources/js/` precis som de publicerats; [vendor.lock](../../../vendor.lock) anger deras
källor och SHA-256-kontrollsummor. Deras licenstexter, inklusive Graphviz och Expat inuti Viz.js, ligger bredvid
dem som `resources/js/LICENSE-*` och levereras i appen. Sparkle lagras inte i arkivet: `cargo xtask dmg`
laddar ner dess release och kontrollerar den mot kontrollsumman i xtask, och dess licens levereras i appen som
`LICENSE-sparkle`.

## Licens

Splat är licensierat under någondera av

- Apache-licensen, version 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE))
- MIT-licensen ([LICENSE-MIT](../../../LICENSE-MIT))

efter eget val.

Om du inte uttryckligen anger något annat ska varje bidrag som du avsiktligt lämnar in för att ingå i Splat, enligt definitionen i licensen Apache-2.0,
dubbellicensieras som ovan, utan ytterligare villkor.
