<p><img src="../../readme-icon.png" alt="Ikona aplikácie Splat" width="168" height="168">&nbsp;&nbsp;&nbsp;<img src="screenshot.png" alt="Splat s dokumentom Demo.md: zdrojový text Markdown vľavo, upraviteľný náhľad vpravo" width="274" height="168"></p>

# Splat

**Editor Markdownu pre macOS, v ktorom píšete v zdrojovom texte alebo priamo v náhľade.**

Splat zobrazuje váš zdrojový text Markdown so zvýraznením syntaxe vľavo a živý, plne upraviteľný náhľad vpravo. Písať môžete v ktoromkoľvek z nich: všetko, čo napíšete v náhľade, sa prenesie do zdrojového textu ako minimálna zmena, takže riadky, ktorých ste sa nedotkli, zostanú bajt po bajte rovnaké a vaše diffy v Gite zostanú čisté. Splat rozumie Markdownu, ktorý už používate: CommonMark a GitHub Flavored Markdown, upozornenia GitHubu, wiki odkazy, vloženia a informačné rámčeky z Obsidianu, rozšírenia Pandocu a Typory, vložené HTML, vzorce LaTeX a diagramy Mermaid, Graphviz a PlantUML, to všetko vykreslené offline. Export do PDF, HTML alebo Wordu jedným kliknutím. Je napísaný v Ruste na skutočných ovládacích prvkoch AppKit, hovorí 35 jazykmi a nič neposiela po sieti, kým o to nepožiadate.
 


[![Stiahnuť Splat pre macOS](https://img.shields.io/badge/Download-Splat_for_macOS-0A84FF?style=for-the-badge&logo=apple&logoColor=white)](https://raw.githubusercontent.com/sventmueller/splat/main/dist/Splat-1.0.4.dmg)

**[Stiahnuť Splat-1.0.4.dmg](https://raw.githubusercontent.com/sventmueller/splat/main/dist/Splat-1.0.4.dmg)**: podpísaný a overený spoločnosťou Apple (notarizovaný), pre macOS 14 alebo novší na Apple Silicon aj Intel.
 
 

[![Rust edícia 2024](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![macOS 14 alebo novší](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)
![Univerzálny: Apple Silicon a Intel](https://img.shields.io/badge/Universal-Apple_Silicon_%7C_Intel-333333?logo=apple&logoColor=white)
[![AppKit cez objc2](https://img.shields.io/badge/AppKit-objc2_0.6-147EFB?logo=apple&logoColor=white)](https://github.com/madsmtm/objc2)
[![WebKit](https://img.shields.io/badge/WebKit-offline-0A84FF?logo=safari&logoColor=white)](https://webkit.org)
[![Comrak 0.59](https://img.shields.io/badge/comrak-0.59-DEA584?logo=markdown&logoColor=white)](https://github.com/kivikakk/comrak)
[![CommonMark a GFM](https://img.shields.io/badge/CommonMark-GFM-24292F?logo=github&logoColor=white)](https://github.github.com/gfm/)
[![MathJax 3.2.2](https://img.shields.io/badge/MathJax-3.2.2-1E6E9E?logo=latex&logoColor=white)](https://www.mathjax.org)
[![MathLive 0.111.1](https://img.shields.io/badge/MathLive-0.111.1-5A3FC0)](https://cortexjs.io/mathlive/)
[![Mermaid 12.2.0](https://img.shields.io/badge/Mermaid-12.2.0-FF3670?logo=mermaid&logoColor=white)](https://mermaid.js.org)
[![Graphviz cez Viz.js 3.24](https://img.shields.io/badge/Graphviz-Viz.js_3.24-2596BE)](https://graphviz.org)
[![PlantUML 1.2026.6](https://img.shields.io/badge/PlantUML-1.2026.6-A80036)](https://plantuml.com)
![35 jazykov rozhrania](https://img.shields.io/badge/languages-35-4C9A2A)
![Predvolene offline](https://img.shields.io/badge/network-offline_by_default-4C9A2A)
![Podpísaný s Hardened Runtime](https://img.shields.io/badge/Hardened_Runtime-signed-4C9A2A)
[![Licencia: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-4C9A2A)](#licencia)

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
**Slovenčina** ·
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

## Obsah

- [Funkcie na prvý pohľad](#funkcie-na-prvý-pohľad)
- [Požiadavky](#požiadavky)
- [Inštalácia](#inštalácia)
- [Prvé kroky](#prvé-kroky)
- [Písanie](#písanie)
  - [V zdrojovom texte](#v-zdrojovom-texte)
  - [V náhľade](#v-náhľade)
  - [Príkazy formátovania](#príkazy-formátovania)
  - [Zoznamy a úlohy](#zoznamy-a-úlohy)
  - [Tabuľky](#tabuľky)
- [Podporovaný Markdown](#podporovaný-markdown)
  - [CommonMark a GitHub Flavored Markdown](#commonmark-a-github-flavored-markdown)
  - [Informačné rámčeky](#informačné-rámčeky)
  - [Rozšírená syntax](#rozšírená-syntax)
  - [Obsidian](#obsidian)
  - [Citácie a odkazy na zdroje](#citácie-a-odkazy-na-zdroje)
  - [Vložené HTML](#vložené-html)
  - [Typografická interpunkcia](#typografická-interpunkcia)
  - [Prehľad syntaxe](#prehľad-syntaxe)
- [Vzorce](#vzorce)
- [Diagramy](#diagramy)
- [Obrázky](#obrázky)
- [Odkazy a navigácia](#odkazy-a-navigácia)
- [Export, tlač a zdieľanie](#export-tlač-a-zdieľanie)
- [Vzhľad a prístupnosť](#vzhľad-a-prístupnosť)
- [Jazyky](#jazyky)
- [Klávesové skratky](#klávesové-skratky)
- [Súkromie a zabezpečenie](#súkromie-a-zabezpečenie)
- [Veľké dokumenty](#veľké-dokumenty)
- [Známe obmedzenia](#známe-obmedzenia)
- [Riešenie problémov](#riešenie-problémov)
- [Pre vývojárov](#pre-vývojárov)
  - [Zostavenie](#zostavenie)
  - [Distribúcia](#distribúcia)
  - [Testovanie](#testovanie)
  - [Štruktúra projektu](#štruktúra-projektu)
  - [Tok dát](#tok-dát)
  - [Pravidlá v ovládači](#pravidlá-v-ovládači)
  - [Pridanie príkazu](#pridanie-príkazu)
  - [Preklady](#preklady)
  - [Komponenty tretích strán](#komponenty-tretích-strán)
- [Licencia](#licencia)

## Funkcie na prvý pohľad

- **Dva spôsoby písania, jeden text.** Zdrojový text aj náhľad sa dajú upravovať. Jediným zdrojom pravdy je súbor Markdown: každý dokument má jednu históriu odvolania a každá úprava v náhľade sa premieta do zdrojového textu ako najmenšia možná zmena.
- **Markdown, ktorý už používate.** CommonMark, GitHub Flavored Markdown, upozornenia GitHubu, rozšírenia Obsidianu, Pandocu a Typory, emotikony zapísané skratkou, spoilery, zoznamy definícií, kotvy nadpisov a HTML ako v README na GitHube.
- **Vzorce a diagramy priamo v aplikácii.** Vzorce LaTeX cez MathJax s vizuálnym editorom vzorcov. Diagramy Mermaid, Graphviz a PlantUML sa vykresľujú na vašom Macu bez Javy, servera či pripojenia k sieti.
- **Tlačidlo pre všetko.** Každý prvok Markdownu má tlačidlo na paneli s nástrojmi a položku v menu, bežné aj klávesovú skratku. Tabuľky majú vlastné menu pre riadky, stĺpce a zarovnanie.
- **Export bez straty.** PDF, jeden samostatný súbor HTML alebo dokument Word so skutočnými nadpismi, zoznamami, tabuľkami, poznámkami pod čiarou a obrázkami.
- **Natívny od základov.** Skutočné okná AppKit, taby, menu, popovery, Verzie, Automatické ukladanie, menu Zdieľať, tmavý režim, kontrola pravopisu, diktovanie a VoiceOver. Žiadny Electron, žiadny webový prvok pre samotný text.
- **Rýchly aj pri veľkých súboroch.** Stlačenie klávesu v dokumente s veľkosťou 10 MB trvá asi 25 ms.
- **Súkromný a opatrný.** Obrázky z webu sa načítajú, až keď ich povolíte, odkazy do iných aplikácií sa najprv pýtajú a všetko, čo dekóduje nedôveryhodné obrázkové dáta, beží v pomocných procesoch v sandboxe.
- **Vo vašom jazyku.** Rozhranie a vstavaný stručný prehľad Markdownu sú v 35 jazykoch: arabčina, katalánčina, čínština (zjednodušená), čínština (tradičná), chorvátčina, čeština, dánčina, holandčina, angličtina, fínčina, francúzština, nemčina, gréčtina, hebrejčina, hindčina, maďarčina, indonézština, taliančina, japončina, kórejčina, malajčina, nórčina, poľština, portugalčina (Brazília), portugalčina (Portugalsko), rumunčina, ruština, slovenčina, slovinčina, španielčina, švédčina, thajčina, turečtina, ukrajinčina a vietnamčina.

## Požiadavky

- macOS 14 Sonoma alebo novší. Splat sa vyvíja a testuje na macOS 27, kde automaticky preberá dizajn Liquid Glass.
- Mac s procesorom Apple Silicon alebo Intel.
- Asi 45 MB miesta na disku.

## Inštalácia

**Z obrazu disku.** Stiahnite si [`dist/Splat-1.0.4.dmg`](https://raw.githubusercontent.com/sventmueller/splat/main/dist/Splat-1.0.4.dmg), otvorte ho a pretiahnite Splat do priečinka Aplikácie. Obraz je podpísaný certifikátom Developer ID a overený spoločnosťou Apple (notarizovaný), takže sa otvorí bez upozornení.

**Cez Homebrew.** Cask je vo vlastnom tape:

```sh
brew install --cask sventmueller/tap/splat
```

`brew upgrade --cask splat` nainštaluje nové verzie a `brew uninstall --zap --cask splat` odstráni aj nastavenia a vyrovnávacie pamäte Splatu.

**Aktualizácie.** Splat z obrazu disku alebo z Homebrew sa aktualizuje sám cez [Sparkle](https://sparkle-project.org). Pri druhom spustení sa spýta, či môže raz denne sám vyhľadávať aktualizácie. **Splat › Vyhľadať aktualizácie…** vyhľadá ihneď; **Automaticky vyhľadávať aktualizácie** a **Automaticky sťahovať a inštalovať aktualizácie** v tom istom menu vašu voľbu neskôr zmenia. Splat nainštaluje aktualizáciu, iba ak nesie podpis vývojára. Vlastná zostavená verzia nemá aktualizátor.

**Zo zdrojového kódu.** Predpoklady nájdete v časti [Zostavenie](#zostavenie), potom:

```sh
make build
ditto target/release/Splat.app /Applications/Splat.app
open /Applications/Splat.app
```

Ak chcete súbory Markdown otvárať v Splate dvojitým kliknutím, vyberte vo Finderi ľubovoľný súbor `.md`, zvoľte **Súbor › Informácie**, v časti **Otvoriť v aplikácii** vyberte Splat a kliknite na **Zmeniť všetky…**. Splat sa sám zaregistruje pre súbory `.md`, `.markdown`, `.mdown` a `.mkd`.

## Prvé kroky

### Okno

Každý dokument sa otvorí vo vlastnom okne alebo ako tab, ak máte radi taby:

- **Vľavo: zdrojový text.** Váš Markdown v písme SF Mono so zvýraznením syntaxe.
- **Vpravo: náhľad.** Vykreslený dokument pripravený na úpravy.
- **Bočný panel s osnovou (voliteľný).** Všetky nadpisy ako strom; kliknutím na nadpis ho zobrazíte v oboch paneloch. Zobrazíte ho cez **Zobraziť › Zobraziť osnovu** (⌃⌘S).
- **Panel s nástrojmi.** Tlačidlá zoradené podľa toho, čo menia: znaky, odseky a vkladané prvky, potom zapnutie a vypnutie náhľadu, Zdieľať a Nový dokument. Prispôsobíte ho cez **Zobraziť › Prispôsobiť panel s nástrojmi…**.

Potiahnutím deliacej čiary dáte jednej strane viac miesta. **Zobraziť › Skryť náhľad** (⌥⌘P) nechá iba zdrojový text; Splat si to pamätá pre každé okno zvlášť. Oba panely sa posúvajú spolu a výber v jednom z nich sa zvýrazní aj v druhom.

### Dokumenty, taby a verzie

Splat je bežná dokumentová aplikácia pre macOS:

- **Automatické ukladanie a Verzie.** Zmeny sa ukladajú na mieste. **Súbor › Vrátiť na › Prechádzať všetky verzie…** vás vráti v čase.
- **Zmeny z iných programov.** Keď Git, skript alebo iný editor zmení otvorený dokument, Splat ho načíta znova a zachová kurzor aj pozíciu posunu. Ak máte vlastné neuložené zmeny, Splat ich zachová a macOS sa pred prepísaním druhej verzie opýta.
- **Bežné príkazy so súbormi.** Duplikovať (⇧⌘S), Premenovať…, Presunúť do… a **Súbor › Otvoriť posledné**.
- **Taby.** **Súbor › Nový tab** alebo tlačidlo **+** na paneli s nástrojmi otvorí nový dokument ako tab; štandardné menu Okno taby zlučuje, presúva a prepína.
- **Umiestnenie okna.** Veľkosť a poloha posledného okna sa si pamätá, aj po reštarte. Ak už na tom mieste okno stojí, nové sa otvorí mierne posunuté.
- **Ikona v Docku.** Kliknutie na ňu po zatvorení posledného okna otvorí nový dokument.
- **Otvárať súbory Markdown v Splate.** **Splat › Nastaviť ako predvolenú apku pre Markdown** spôsobí, že dvojité kliknutie na súbor `.md` alebo `.markdown` otvorí Splat; macOS sa raz opýta na potvrdenie. Kým je Splat predvolenou aplikáciou, položka má začiarknutie.

Súbory musia byť text v UTF-8 alebo UTF-16 so značkou poradia bajtov. Iné kódovania Splat odmietne, namiesto aby hádal a poškodil text. Značka poradia bajtov zostane, UTF-16 zostane UTF-16 a v súbore, ktorého všetky riadky končia CRLF, dostanú CRLF aj riadky, ktoré pridáte.

**Po páde** sa stratí najviac posledných pár sekúnd písania: Splat každých päť sekúnd kopíruje neuložený text do `~/Library/Application Support/Splat/Unsaved` a kópie odstráni, keď sa dokument uloží alebo Splat normálne skončí. Pri ďalšom spustení presunie to, čo tam pád zanechal, do `~/Library/Application Support/Splat/Recovery` a raz o tom informuje, s tlačidlom, ktoré otvorí priečinok. Pád, na ktorý Splat ešte dokáže zareagovať, tam zapíše aj text každého otvoreného dokumentu.

### Stručný prehľad

**Pomocník › Markdown – stručný prehľad** otvorí tahák vo vašom jazyku ako nový dokument bez názvu. Je to bežný dokument, takže si tam všetko vyskúšajte, v zdrojovom texte alebo v náhľade.

## Písanie

### V zdrojovom texte

Zdrojový text je editor čistého textu prispôsobený pre Markdown:

- **Zvýraznenie syntaxe** ako vo VS Code: nadpisy, zvýraznenie, odkazy, kód, tabuľky, HTML, front matter a LaTeX s Mermaidom vo vzorcoch a diagramoch. K dispozícii sú palety pre svetlý režim, tmavý režim a Zvýšiť kontrast.
- **Ohradené bloky kódu** sa zvýrazňujú pre Bash, C, C++, C#, CSS (aj SCSS a Less), Go, HTML, XML, Javu, JavaScript, JSON, Kotlin, PHP, Python, Ruby, Rust, SQL, Swift, TypeScript, YAML, TeX a Mermaid, vrátane ich bežných skrátených názvov, ako sú `js`, `py` alebo `sh`.
- **Žiadne neočakávané zámeny.** Dynamické úvodzovky, dynamické pomlčky a nahradenie textu sú predvolene vypnuté, pretože rozbíjajú `--`, `---` a `"`. Zapnete ich cez **Upraviť › Zámeny**.
- **Systémové nástroje na písanie.** Kontrola pravopisu a gramatiky, diktovanie, Nájsť a nahradiť (⌥⌘F) a Reč.
- **Pretiahnutie myšou.** Pustením súboru s obrázkom vložíte `![názov](relative/path.png)`, pustením iného súboru odkaz naň a pustením adresy URL vložíte `[url](url)`. Každé pustenie je jeden krok odvolania.
- **Vkladanie obrázkov.** Obrázok v schránke, napríklad snímka obrazovky, sa uloží ako súbor PNG vedľa dokumentu (`Obrázok 2026-10-29 13.09.07.png`) a vloží sa ako obrázok. Dokument musí byť na to raz uložený.

### V náhľade

Náhľad sa správa ako textový procesor, ale všetko, čo v ňom urobíte, skončí ako Markdown:

- **Písanie.** Znaky, ktoré v Markdowne niečo znamenajú, ako `*` alebo `#`, sa ošetria, aby sa zobrazili tak, ako ste ich napísali.
- **Return** začne nový odsek alebo položku zoznamu. **⌥Return** alebo **⌃Return** vloží zlom riadku.
- **Formátovanie pri kurzore.** Ak je kurzor na konci slova, Tučné, Kurzíva a ďalšie formáty znakov platia pre to, čo napíšete ďalej. Ďalším výberom formátu to ukončíte. Vnútri slova sa naformátuje celé slovo.
- **Prázdne riadky.** Return na prázdnom riadku pridá ďalší prázdny riadok a Backspace ho odstráni. V zdrojovom texte sú na to dva prázdne riadky navyše; iné programy pre Markdown ich zobrazia ako bežnú medzeru medzi odsekmi.
- **Odvolať** funguje v oboch paneloch. Položky odvolania majú názvy („Odvolať Tučné“), písanie je jeden krok na jednu dávku a po odvolaní a obnovení sa oba panely posunú na zmenu.
- **Vzorce a diagramy** sa zobrazujú ako hotová grafika. Kliknutím na ne, presunutím kurzora na ne alebo stlačením Return na nich otvoríte ich editor (pozri [Vzorce](#vzorce) a [Diagramy](#diagramy)).
- **Vložené HTML, vložené poznámky a front matter** sa zobrazujú, ale upravujete ich v zdrojovom texte. Kliknutie na ne umiestni kurzor na zodpovedajúce miesto tam.

Všetko, čo Markdown nedokáže vyjadriť, je v náhľade zablokované: panely Písma a Farby, pravítko a vkladanie alebo púšťanie formátovaného textu a obrázkov. Vložené HTML a RTF sa vložia ako čistý text.

### Príkazy formátovania

Každý príkaz je dostupný na paneli s nástrojmi, v lište menu a v kontextovom menu oboch panelov. Položky menu zobrazujú začiarknutie pre formátovanie pri kurzore.

| Menu | Príkazy |
|---|---|
| **Formát › Písmo** | Tučné, Kurzíva, Podčiarknuté, Preškrtnuté, Zvýrazniť, Kód, Horný index, Dolný index |
| **Formát › Nadpis** | Základný text, Nadpis 1 až 6 |
| **Formát › Zoznam** | Zoznam s odrážkami, Číslovaný zoznam, Zoznam úloh |
| **Formát** | Citácia, Blok kódu (Bez jazyka alebo jeden z 20 jazykov; zmení aj jazyk existujúceho bloku), Informačný rámček (Poznámka, Tip, Dôležité, Upozornenie, Pozor; rovnaký typ znova ho odstráni), Upraviť vzorec alebo diagram…, Odstrániť formátovanie |
| **Vložiť** | Odkaz…, Obrázok…, Tabuľka…, Vzorec, Diagram (Vývojový diagram, Sekvenčný diagram, Ganttov diagram), Obsah, Poznámka pod čiarou, Oddeľovač |
| **Tabuľka** | Vložiť riadok nad/pod, Vložiť stĺpec vľavo/vpravo, Vymazať riadok, Vymazať stĺpec, Zarovnanie (Vľavo, Na stred, Vpravo, Predvolené), Zarovnať stĺpce tabuľky |
| **Zobraziť** | Skryť/Zobraziť náhľad, Zobraziť/Skryť osnovu, Typografická interpunkcia, Zväčšiť/Zmenšiť, Skutočná veľkosť, panel s nástrojmi, lišta s tabmi, celá obrazovka |

Podčiarknutie, zvýraznenie, horný a dolný index sa zapisujú ako `<ins>`, `<mark>`, `<sup>` a `<sub>`, ktoré zobrazuje aj GitHub. Splat číta aj `<u>`, ktoré GitHub odstraňuje, a kratšie `==highlight==`, `^sup^` a `~sub~`. GitHub zobrazuje `==` a `^` tak, ako sú napísané, a `~sub~` ako preškrtnutie.

### Zoznamy a úlohy

- Return pokračuje v zozname; Return na prázdnej položke ho ukončí.
- **Tab** a **⇧Tab** v náhľade odsadzujú a zrušia odsadenie položiek zoznamu.
- Kliknutie na zaškrtávacie políčko úlohy v náhľade prepína `- [ ]` a `- [x]`.
- **Formát › Zoznam** zmení označené odseky na zoznam alebo zmení jeho druh.

### Tabuľky

- **Vložiť › Tabuľka…** sa opýta na počet riadkov (vrátane hlavičky) a stĺpcov.
- V náhľade sa **Tab** presunie na ďalšiu bunku, **⇧Tab** na predchádzajúcu a **Return** na bunku pod ňou. Tab v poslednej bunke pridá riadok.
- Menu **Tabuľka** vkladá a maže riadky a stĺpce a nastavuje zarovnanie stĺpca (⌘{, ⌘|, ⌘}).
- **Tabuľka › Zarovnať stĺpce tabuľky** doplní zdrojový text medzerami, aby stĺpce lícovali.

## Podporovaný Markdown

Splat parsuje pomocou [comrak](https://github.com/kivikakk/comrak), parsera kompatibilného s CommonMark a GFM, a zapína väčšinu jeho rozšírení. Všetko nižšie sa vykreslí v náhľade aj vo všetkých exportoch.

### CommonMark a GitHub Flavored Markdown

- Nadpisy (`#` a podčiarknuté), odseky, zlomy riadkov, zvýraznenie, silné zvýraznenie, kód v texte, blokové citácie, oddeľovače.
- Zoznamy s odrážkami, číslované a vnorené; zoznamy úloh.
- Odkazy (v texte, referenčné a automatické), obrázky, definície referenčných odkazov.
- Ohradené a odsadené bloky kódu so zvýraznením syntaxe.
- Tabuľky so zarovnaním stĺpcov.
- Preškrtnutie pomocou `~~dvoch vlnoviek~~`.
- Poznámky pod čiarou: `text[^1]` s `[^1]: Poznámka.` kdekoľvek v dokumente. Kliknutie na značku prejde na poznámku.
- Front matter v YAML medzi riadkami `---` na začiatku. Náhľad ho zhrnie do jedného riadku, napríklad „Metadáta: title, date, tags“.

### Informačné rámčeky

Upozornenia GitHubu sa vykreslia ako farebné rámčeky s ikonou a názvom:

```markdown
> [!NOTE]
> Užitočná informácia, ktorú by mali používatelia poznať.

> [!WARNING]
> Kritický obsah, ktorý si vyžaduje okamžitú pozornosť.
```

Päť typov je Poznámka, Tip, Dôležité, Upozornenie a Pozor. **Formát › Informačný rámček** zmení odsek pri kurzore na jeden z nich.

Fungujú aj informačné rámčeky z Obsidianu, s ľubovoľným názvom a stavom zbalenia:

```markdown
> [!tip]- Kliknutím na ▸ rozbalíte
> Skryté, kým sa neotvorí.

> [!question]+ Predvolene otvorené
> Znak `+` začína rozbalený, znak `-` zbalený.
```

Ostatné typy z Obsidianu sa zobrazia v najbližšej z piatich farieb: `success`, `check`, `done` a `hint` ako Tip; `question`, `help` a `faq` ako Dôležité; `attention` ako Upozornenie; `failure`, `fail`, `missing`, `danger`, `error` a `bug` ako Pozor; všetko ostatné ako Poznámka.

### Rozšírená syntax

| Funkcia | Syntax | Pôvod |
|---|---|---|
| Zvýraznenie | `==označený text==` | Obsidian, Typora |
| Horný index | `x^2^` | Pandoc, Typora |
| Dolný index | `H~2~O` | Pandoc, Typora |
| Skratka emotikonu | `:rocket:` sa zmení na 🚀 | GitHub |
| Spoiler | `\|\|skrytý text\|\|`, na prečítanie ho označte | Discord |
| Poznámka pod čiarou v texte | `text^[samotná poznámka]` | Pandoc, Obsidian |
| Zoznam definícií | `Pojem`, prázdny riadok, `: Vysvetlenie` | PHP Markdown Extra, Pandoc |
| Kotva nadpisu | `## Inštalácia {#setup}`, odkaz cez `[pozri](#setup)` | Pandoc, kramdown |

Zoznamy definícií vyzerajú v zdrojovom texte takto:

```markdown
Pojem

: Vysvetlenie pojmu.
```

Bez vlastnej kotvy dostane každý nadpis kotvu odvodenú z jeho textu, ako na GitHube: `## Prvé kroky` je dostupný ako `#prvé-kroky`.

### Obsidian

- **Wiki odkazy.** `[[Note]]` odkazuje na `Note.md` v rovnakom priečinku alebo v priečinku pod ním. `[[Note|Zobrazený text]]` zobrazí váš vlastný text a `[[Note#Heading]]` otvorí poznámku. ⌘-kliknutie ju otvorí v Splate.
- **Vloženia.** `![[Note]]` zobrazí inú poznámku vo vnútri tejto; poznámky vložené v nej sa zobrazia ako odkazy. `![[diagram.png]]` zobrazí obrázok. Ako v Obsidiane, vloženia pomenúvajú poznámky a obrázky v priečinku dokumentu alebo pod ním, nikdy absolútnou cestou, a ako poznámky sa zobrazujú iba súbory Markdown.
- **Komentáre.** `%%Iba v zdrojovom texte%%` zostane viditeľný v zdrojovom texte a z náhľadu aj zo všetkých exportov sa vynechá, v texte aj na samostatných riadkoch.
- **Štítky.** `#idea` alebo `#project/splat` sa zvýrazní. ⌘-kliknutie vyhľadá štítok v dokumente.
- **Zbaliteľné informačné rámčeky.** Pozri [Informačné rámčeky](#informačné-rámčeky).

### Citácie a odkazy na zdroje

- **Citácie Pandocu.** `[@doe2020]` alebo `[@doe2020; @roe2021]` sa zobrazí ako „(Doe 2020; Roe 2021)“. Záznamy pochádzajú zo súboru BibTeX, ktorý front matter uvádza ako `bibliography: refs.bib`. Bez neho Splat použije `references.bib`, `literatur.bib` alebo `bibliography.bib` vedľa dokumentu.
- **Odkazy na GitHub a GitLab.** Ak dokument leží v repozitári Git, ktorého vzdialený repozitár `origin` je na github.com alebo na hostiteľovi GitLab, zvýraznia sa `@name`, `#12` (issue) a `!7` (merge request, iba GitLab). ⌘-kliknutie ich otvorí v prehliadači.

### Vložené HTML

HTML, aké nájdete v README na GitHube, sa vykresľuje vlastným čitateľom, nie enginom prehliadača:

- vycentrované odseky a bloky (`<p align="center">`, `<div align="center">`)
- obrázky s veľkosťou (`<img src="logo.png" width="120">`)
- odkazy, `<kbd>⌘</kbd>`, `<br>`, `<u>`, `<ins>`, `<mark>`, `<sup>`, `<sub>`
- zoznamy a tabuľky, vrátane buniek cez viac stĺpcov (`colspan`)
- zbaliteľné sekcie: `<details><summary>Viac</summary>…</details>`, otvorené kliknutím na ▸

Markdown medzi značkami HTML sa vykreslí ako Markdown. Skripty, štýly, rámce a formuláre sa úplne vynechajú a odkazy a obrázky si ponechajú iba adresy `http`, `https`, `mailto` a relatívne.

### Typografická interpunkcia

**Zobraziť › Typografická interpunkcia** zobrazí rovné úvodzovky ako úvodzovky jazyka vášho rozhrania, `--` ako polpomlčku, `---` ako pomlčku a `...` ako výpustku. V nemčine sa `"Ja"` zmení na „Ja“, vo francúzštine na «Oui», v japončine na 「はい」, aj tam, kde čínština a japončina píšu úvodzovky bez medzery; hebrejské skratky ako `צה"ל` dostanú gershayim, צה״ל. Zdrojový text si ponechá obyčajné znaky; náhľad a všetky exporty zobrazia typografické. Nastavenie je vypnuté, kým ho nezapnete.

### Prehľad syntaxe

| Prvok | Markdown |
|---|---|
| **Tučné** | `**Tučné**` |
| *Kurzíva* | `*Kurzíva*` |
| Podčiarknuté | `<ins>Podčiarknuté</ins>` alebo `<u>Podčiarknuté</u>` |
| ~~Preškrtnuté~~ | `~~Preškrtnuté~~` |
| Zvýraznenie | `<mark>Zvýraznenie</mark>` alebo `==Zvýraznenie==` |
| `Kód` | `` `Kód` `` |
| x² | `x<sup>2</sup>` alebo `x^2^` |
| H₂O | `H<sub>2</sub>O` alebo `H~2~O` |
| [Odkaz](https://example.com) | `[Odkaz](https://example.com)` |
| Nadpis | `# Nadpis` až `###### Nadpis` |
| Citácia | `> Citácia` |
| Zoznam s odrážkami | `- Položka` |
| Číslovaný zoznam | `1. Položka` |
| Úloha | `- [ ] Otvorená` a `- [x] Hotová` |
| Blok kódu | ```` ```rust ```` … ```` ``` ```` |
| Tabuľka | `\| A \| B \|` s `\|---\|---\|` pod hlavičkou |
| Poznámka pod čiarou | `Text[^1]` a `[^1]: Poznámka` |
| Oddeľovač | `---` |
| Obrázok | `![Alternatívny text](picture.png)` |
| Vzorec | `$E = mc^2$`, `$$ … $$` |
| Diagram | ```` ```mermaid ````, ```` ```dot ````, ```` ```plantuml ```` |
| Informačný rámček | `> [!TIP]` |
| Wiki odkaz | `[[Note]]`, `[[Note\|Text]]` |
| Vloženie | `![[Note]]`, `![[image.png]]` |
| Komentár | `%%Komentár%%` |
| Štítok | `#štítok` |
| Citácia zdroja | `[@key]` |
| Spoiler | `\|\|Spoiler\|\|` |
| Emotikon | `:tada:` |

## Vzorce

Vzorce vykresľuje [MathJax](https://www.mathjax.org) s matematikou TeX a AMS ako ostrú vektorovú grafiku. Text v `\text{…}` môže byť v ľubovoľnom písme: čo v písmach MathJaxu chýba, sa berie z písiem vášho Macu, pričom arabčina a hebrejčina sa píšu sprava doľava a emotikony sú farebné.

| Druh | Syntax |
|---|---|
| V texte | `$E = mc^2$`, `` $`E = mc^2`$ `` (GitHub, GitLab), `\(E = mc^2\)` |
| Samostatný | `$$` … `$$`, `\[` … `\]` alebo blok kódu označený `math` |

**Úprava vzorca.** Kliknutím na vzorec v náhľade, dvojitým kliknutím, stlačením Return na ňom alebo presunutím kurzora naň otvoríte jeho editor v popovere. **Vložiť › Vzorec** vloží nový a hneď otvorí editor.

- **Vizuálne alebo LaTeX.** Prepínač navrchu prepína medzi vizuálnym editorom ([MathLive](https://cortexjs.io/mathlive/)) a kódom LaTeX. Splat si vašu voľbu pamätá.
- **Stavebné bloky** pre zlomok, druhú odmocninu, horný a dolný index, súčet a integrál s hranicami, limitu, zátvorky, ktoré rastú s obsahom, maticu 2×2, grécke písmená a relácie, operátory a šípky. Zanechávajú zástupné symboly □ a **Tab** preskočí na ďalší.
- **Živý náhľad.** Zmeny sa v dokumente zobrazujú počas písania. Chyby z MathJaxu sa zobrazia v riadku pod kódom.
- **Potvrdiť alebo zahodiť.** Kliknutie mimo ponechá zmenu ako jeden krok odvolania; **Esc** alebo **Zrušiť** ju zahodí. Stlačenie Esc pri novom prázdnom vzorci ho opäť odstráni.
- Kontextové menu ponúka **Upraviť vzorec…** pre vzorec pod ukazovateľom.

VoiceOver číta vzorec ako jeho kód LaTeX. Splat vykresľuje vzorce, nie úplné dokumenty LaTeX: nie sú tu žiadne balíky, žiadny TikZ a žiadny `\documentclass`.

## Diagramy

Diagramy sa vykresľujú v Splate, offline a v svetlom aj tmavom variante.

| Nástroj | Blok kódu | Čo kreslí |
|---|---|---|
| [Mermaid](https://mermaid.js.org) | ```` ```mermaid ```` | vývojové diagramy, sekvenčné, triedne, stavové, diagramy vzťahov entít, Ganttove, koláčové, myšlienkové mapy, časové osi a všetky ostatné typy Mermaidu |
| [Graphviz](https://graphviz.org) | ```` ```dot ```` alebo ```` ```graphviz ```` | grafy v jazyku DOT |
| [PlantUML](https://plantuml.com) | ```` ```plantuml ```` alebo ```` ```puml ```` | diagramy UML; beží bez Javy |

```mermaid
flowchart LR
    A[Markdown] --> B[Náhľad]
    B --> C[PDF]
```

**Úprava diagramu** funguje ako úprava vzorca: kliknutím naň otvoríte kód v popovere so zvýraznením syntaxe a chybovými hláseniami v riadku pod kódom. **Vložiť › Diagram** začína od šablóny pre vývojový diagram, sekvenčný diagram alebo Ganttov diagram. Stavebné bloky pomáhajú pri troch najčastejších typoch Mermaidu:

- **Vývojový diagram:** Uzol ▾ (obdĺžnik, zaoblený, kruh, kosoštvorec, databáza), Spojenie, Spojenie s popisom, Skupina, Smer ▾
- **Sekvenčný diagram:** Účastník, Správa, Odpoveď, Poznámka, Slučka, Alternatívne cesty
- **Ganttov diagram:** Sekcia, Úloha, Míľnik

Ostatné typy diagramov sa upravujú ako kód. VoiceOver číta názov diagramu alebo jeho alternatívny text.

Prvý diagram po spustení trvá asi sekundu, pretože vykresľovací engine sa spúšťa na pozadí. Diagram, ktorý trvá dlhšie ako 10 sekúnd, zobrazí chybu a vykresľovací engine sa pre ďalší spustí odznova. Engine PlantUML (7 MB) sa načíta až s prvým diagramom PlantUML. Príkaz `!include` z webu v PlantUML je zablokovaný.

## Obrázky

- **Lokálne obrázky** používajú cesty relatívne k dokumentu: `![Logo](images/logo.png)`. Splat zobrazí PNG, JPEG, GIF, WebP, BMP, SVG, HEIC, TIFF, AVIF a PDF.
- **Obrázky z webu** zostanú zablokované, kým ich nepovolíte: lišta nad náhľadom vysvetľuje, že ich načítaním sa ich servery dozvedia, že a kedy dokument otvárate. **Načítať obrázky** ich povolí pre tento dokument; Splat si voľbu pamätá pre každý súbor zvlášť.
- **Veľkosť.** `<img width="200">` nastaví veľkosť obrázka; inak sa obrázky prispôsobia šírke náhľadu.
- **Zmeny na disku.** Obrázok, ktorý nahradíte, alebo taký, ktorý sa objaví po tom, čo ho dokument uviedol, sa zobrazí bez opätovného otvorenia dokumentu. Obrázky z webu, ktoré zlyhali kvôli sieti, sa skúšajú znova, kým ich dokument zobrazuje.
- **Pamäť.** Obrázky sa zobrazujú do 2560 pixelov na dlhšej strane, čo vyplní aj stranu A4. Dekódujú sa menšie, keď zaberú osminu pamäte Macu a kým macOS hlási nedostatok pamäte.
- **Tmavý režim.** Obrázky s bielym pozadím sa v tmavom náhľade mierne stlmia.
- **Vkladanie a pretiahnutie.** Pozri [V zdrojovom texte](#v-zdrojovom-texte).

## Odkazy a navigácia

V náhľade kliknutie umiestni kurzor, aby ste mohli upraviť text odkazu; **⌘-kliknutie** odkaz otvorí:

- `http`, `https` a `mailto` sa otvoria vo vašom prehliadači alebo poštovej aplikácii.
- Súbory Markdown sa otvoria v Splate; ostatné súbory a priečinky sa zobrazia vo Finderi.
- `#anchor` prejde na nadpis v dokumente.
- Adresy, ktoré otvárajú inú aplikáciu, ako `slack://` alebo `vscode://`, sa najprv opýtajú a zobrazia úplnú adresu a aplikáciu. **Zrušiť** je predvolené.

**Vložiť › Obsah** vloží prepojený zoznam všetkých nadpisov. Ak je kurzor v ňom, ten istý príkaz ho aktualizuje. Bočný panel s osnovou (⌃⌘S) prejde na ľubovoľný nadpis v oboch paneloch.

## Export, tlač a zdieľanie

| Formát | Ako | Čo získate |
|---|---|---|
| **PDF** | **Súbor › Exportovať ako PDF…** alebo menu Zdieľať | Strany vo veľkosti papiera, vždy vo svetlom vzhľade, vzorce a diagramy ako vektorová grafika. **Súbor › Nastavenie strany…** nastaví papier. |
| **Tlač** | **Súbor › Tlačiť…** (⌘P) | Rovnaké rozloženie ako v PDF. |
| **HTML** | **Súbor › Exportovať ako HTML…** alebo menu Zdieľať | Jeden samostatný súbor: lokálne obrázky vložené, vzorce a diagramy ako ostré obrázky, kotvy nadpisov, aby fungoval obsah. Žiadne skripty. |
| **Word** | **Súbor › Exportovať ako dokument Word…** alebo menu Zdieľať | Súbor `.docx` so štýlmi nadpisov Wordu, skutočnými zoznamami a tabuľkami (vrátane zlúčených buniek), poznámkami pod čiarou ako poznámkami Wordu, vloženými obrázkami, vzorcami a diagramami. |
| **Markdown** | Tlačidlo **Zdieľať** na paneli s nástrojmi | Dokument ako súbor `.md` pre Mail, Správy, AirDrop a každú ďalšiu službu zdieľania. |

Menu **Súbor › Zdieľať** ponúka všetko z toho na jednom mieste. Exporty používajú nastavenie typografickej interpunkcie a obrázky z webu, ktoré ste povolili; obrázky z webu zostanú v HTML odkazmi.

## Vzhľad a prístupnosť

- **Tmavý režim** sleduje systém. Vzorce a diagramy sa vykresľujú pre každý vzhľad.
- **Zvýšiť kontrast** v Nastaveniach systému prepne farby syntaxe na vysokokontrastné palety.
- **Zväčšenie a zmenšenie.** ⌘+, ⌘− a ⌘0 mení mierku oboch panelov.
- **Celá obrazovka, taby a prispôsobiteľný panel s nástrojmi**, ako v každej aplikácii pre Mac.
- **VoiceOver.** Panely sa volajú „Zdrojový text Markdown“ a „Náhľad“, vzorce sa čítajú ako ich kód a diagramy ako ich názov. Každý príkaz sa dá vyvolať z klávesnice.

Splat nemá okno s nastaveniami: niekoľko možností je tam, kde sa uplatňujú, v menu **Zobraziť** a **Upraviť › Zámeny**.

## Jazyky

Rozhranie, menu a stručný prehľad Markdownu sú preložené do 35 jazykov:

Arabčina, katalánčina, čínština (zjednodušená), čínština (tradičná), chorvátčina, čeština, dánčina, holandčina, angličtina, fínčina, francúzština, nemčina, gréčtina, hebrejčina, hindčina, maďarčina, indonézština, taliančina, japončina, kórejčina, malajčina, nórčina, poľština, portugalčina (Brazília), portugalčina (Portugalsko), rumunčina, ruština, slovenčina, slovinčina, španielčina, švédčina, thajčina, turečtina, ukrajinčina a vietnamčina.

Splat sa riadi poradím jazykov v **Systémové nastavenia › Všeobecné › Jazyk a región**; tam v časti **Aplikácie** môžete vybrať jazyk aj iba pre Splat. Ak pre váš jazyk nie je preklad, použije angličtinu. Pojmy v menu vychádzajú z prekladov od Applu, takže Splat pôsobí ako zvyšok vášho Macu.

Hebrejský a arabský text sa čítajú sprava doľava všade, kde sa objavia, bez ohľadu na jazyk rozhrania: každý odsek, nadpis a položka zoznamu preberá smer svojho prvého písmena, citácie a informačné rámčeky kreslia pruh vpravo a tabuľka, ktorej text začína sprava doľava, radí stĺpce sprava. Bloky kódu zostávajú zľava doprava. Exporty do PDF, HTML a Wordu tieto smery zachovávajú. Ak je systémovým jazykom hebrejčina alebo arabčina, macOS zrkadlí aj okno, takže náhľad je vľavo.

## Klávesové skratky

| Akcia | Skratka |
|---|---|
| Tučné, Kurzíva, Podčiarknuté | ⌘B, ⌘I, ⌘U |
| Preškrtnuté | ⇧⌘X |
| Odkaz… | ⌘K |
| Základný text, Nadpis 1 až 6 | ⌥⌘0, ⌥⌘1 až ⌥⌘6 |
| Zarovnať stĺpec tabuľky vľavo, na stred, vpravo | ⌘{, ⌘\|, ⌘} |
| Skryť alebo zobraziť náhľad | ⌥⌘P |
| Zobraziť alebo skryť osnovu | ⌃⌘S |
| Zväčšiť, zmenšiť, skutočná veľkosť | ⌘+, ⌘−, ⌘0 |
| Nový dokument, otvoriť, uložiť, duplikovať | ⌘N, ⌘O, ⌘S, ⇧⌘S |
| Nastavenie strany, tlač | ⇧⌘P, ⌘P |
| Nájsť, nájsť a nahradiť | ⌘F, ⌥⌘F |
| Nájsť ďalšie, predchádzajúce; použiť výber; prejsť na výber | ⌘G, ⇧⌘G; ⌘E; ⌘J |
| Zobraziť pravopis a gramatiku; skontrolovať dokument | ⌘:, ⌘; |
| Vložiť a prispôsobiť štýl | ⌥⇧⌘V |
| Skryť panel s nástrojmi, zobraziť všetky taby, celá obrazovka | ⌥⌘T, ⇧⌘\\, ⌃⌘F |
| Zlom riadku (náhľad) | ⌥Return alebo ⌃Return |
| Ďalšia bunka / predchádzajúca bunka v tabuľke (náhľad) | Tab / ⇧Tab |
| Odsadiť / zrušiť odsadenie položky zoznamu (náhľad) | Tab / ⇧Tab |
| Ďalší zástupný symbol v editore vzorcov | Tab |
| Zavrieť editor vzorca alebo diagramu bez zmien | Esc |
| Otvoriť odkaz, štítok alebo odkaz na zdroj (náhľad) | ⌘-kliknutie |

Splat necháva štandardné skratky Applu tam, kde sú.

## Súkromie a zabezpečenie

Súbor Markdown môže pochádzať od kohokoľvek, preto Splat považuje každý dokument za nedôveryhodný.

- **Predvolene bez siete.** Splat nemá telemetriu. Aktualizácie vyhľadáva, až keď s tým súhlasíte alebo zvolíte **Vyhľadať aktualizácie…**: potom načíta malý súbor kanála z GitHubu a požiadavka uvádza len aplikáciu a jej verziu. Vzorce a diagramy sa vykresľujú lokálne; webové zobrazenia, ktoré to robia, nemajú prístup k sieti.
- **Obrázky z webu** sa načítajú až s vaším povolením, len cez `https`, nikdy z hostiteľov vo vašej lokálnej sieti, najviac po 10 MB a v súkromnej relácii bez cookies.
- **Izolované dekódovanie.** Obrázky a grafiku vzorcov a diagramov dekódujú pomocné procesy, ktoré zatvoria každý zdedený súbor a pred čítaním prvého bajtu sa zamknú do sandboxu: bez prístupu k súborom, bez siete. Vracajú iba pixely, ktorých veľkosť sa kontroluje, a po 10 sekundách sa zastavia. Dekodéry obrázkov od Applu vidia iba HEIC, TIFF, AVIF a PDF, a to len vtedy, keď dáta naozaj majú tento formát; bežia vo vlastnom pomocnom procese, ktorý smie navyše čítať systémové priečinky s písmami a používať systémový dekodér videa. Týmto pomocníkom prechádzajú aj SVG, vzorce a diagramy, takže v Splate sú to obrázky s pevným rozlíšením: štyri pixely na bod, najviac 2560 na dlhšej strane.
- **Lokálne súbory** sa do dokumentu dostanú len cez cestu, ktorú uvádza. Vloženia zobrazujú poznámky Markdown z priečinka dokumentu a pod ním a chýbajúci obrázok sa hľadá len tam. Cesta k obrázku inde, absolútna alebo začínajúca `~`, sa stále načíta a exporty do HTML a Wordu ho vložia: export dokumentu, ktorý ste nenapísali, si pred odovzdaním ďalej pozrite.
- **Kópie neuloženého textu** zostávajú na tomto Macu, vo vašom priečinku Knižnica, na obnovenie po páde. Odídu s uložením alebo normálnym ukončením Splatu; kópie, ktoré zanechal pád, zostanú v priečinku obnovy, kým ich nevymažete.
- **Odkazy** nikdy nespustia program kliknutím. Webové a poštové odkazy sa otvoria ako zvyčajne; odkazy do iných aplikácií sa najprv opýtajú.
- **HTML v dokumentoch** číta vlastný parser Splatu. Skripty, štýly, rámce a formuláre sa nikdy nespustia.
- **Exporty** neobsahujú skripty. Stránky HTML si ponechajú iba pevný zoznam neškodných značiek a atribútov, ako na GitHube, plus politiku zabezpečenia obsahu. Súbory Word odkazujú iba na adresy `http`, `https` a `mailto`.
- **Podpis kódu** s Hardened Runtime. Splat beží bez App Sandboxu, aby mohol zobrazovať obrázky vedľa vašich dokumentov bez pýtania sa na každý priečinok; izolácia celého dekódovania to vyvažuje.

## Veľké dokumenty

Splat zostáva pohotový aj pri dokumentoch s veľkosťou niekoľkých megabajtov:

- Zdrojový text používa TextKit 2, ktorý rozvrhuje len viditeľnú časť.
- Počas písania v náhľade a v zdrojovom texte dokumentov do 2 MB sa len dotknuté odseky znova parsujú, farbia a rozvrhujú: asi 25 ms v dokumente s veľkosťou 10 MB a asi 50 ms pre stlačenie klávesu v jeho náhľade, s vlastnou prácou zobrazenia.
- Veľké náhľady vykresľujú najprv viditeľnú časť a zvyšok, keď aplikácia nič nerobí. Bloky, ktoré sa posunú do zobrazenia, sa vykreslia pred vykreslením na obrazovku.
- V zdrojovom texte väčších dokumentov beží parsovanie pre náhľad na pozadí, kým píšete.
- Od 64 MB zostane dokument čistým textom: zdrojový text nemá farby a náhľad uvádza, prečo zostáva prázdny. Exporty stále fungujú.
- Od približne 200 KB sa okno dokumentu otvorí pred parsovaním; pri 5 MB nasledujú farby a náhľad asi o štvrť sekundy neskôr.

Niektoré zmeny sa spracúvajú celé, pri 10 MB okolo 300 ms na stlačenie klávesu: zmeny definície alebo odkazov poznámky pod čiarou, vedľa poznámky pod čiarou v texte, na úplnom konci dokumentu vedľa poznámky pod čiarou, vo front matter alebo blízko definícií referenčných odkazov a každá zmena v dokumente s blokmi komentárov `%%` alebo s dvakrát definovanou poznámkou pod čiarou alebo s poznámkou vnútri citácie či zoznamu. Pamäť rastie so súborom, zhruba na 115 – 160 MB pre 1 MB Markdownu a 340 – 510 MB pre 10 MB.

## Známe obmedzenia

- **Jedna vlnovka znamená dolný index.** `~text~` je dolný index ako v Pandocu; preškrtnutie vyžaduje `~~dve~~`. GitHub preškrtáva aj jednou vlnovkou.
- **Iba na čítanie v náhľade:** vložené HTML, vložené poznámky a front matter sa upravujú v zdrojovom texte.
- **Veľmi hlboké vnorenie:** HTML vnorené hlbšie ako 64 úrovní a HTML v texte hlbšie ako 16 ustúpi svojmu obsahu.
- **Zlúčené bunky tabuľky** existujú len v tabuľkách HTML a len cez stĺpce (`colspan`, nie `rowspan`). Syntax tabuliek MultiMarkdownu pre zlúčené bunky nie je podporovaná.
- **Bibliografie** sa čítajú iba z BibTeXu, nie z CSL JSON ani YAML.
- **Odkazy na zdroje** potrebujú repozitár na github.com alebo na hostiteľovi GitLab; `!7` existuje iba v GitLabe.
- **Typografická interpunkcia:** nemeckú zatváraciu jednoduchú úvodzovku nemožno odlíšiť od apostrofu a nastaví sa ako ’.
- **Vzorce, nie dokumenty:** žiadne balíky LaTeX, TikZ ani úplné dokumenty.
- **Vizuálny editor vzorcov** zapisuje LaTeX v zápise MathLive: medzery miznú (`E = mc^2` sa zmení na `E=mc^2`), vykreslenie zostane rovnaké.
- **Zámerne:** žiadne čísla riadkov, žiadne okno s nastaveniami, žiadny Help Book, nie v App Store.
- **Menej testované na Intele.** Testy okna bežia len na Apple Silicon; na Intele CI spúšťa `cargo test`.

## Riešenie problémov

**Zoznam nedávnych dokumentov je po novom zostavení Splatu prázdny.** macOS viaže nedávne dokumenty a oprávnenia priečinkov na podpis kódu aplikácie. Lokálne zostavenia sú podpísané certifikátom „Splat Local Signing“ vo vašej kľúčenke prihlásenia, takže si po novom zostavení zachovajú oboje. Ak sa certifikát vymazal, ďalšie zostavenie vytvorí nový a zoznamy začnú raz odznova.

**Aplikácia z obrazu disku zobrazuje iné nedávne dokumenty ako moje vlastné zostavenie.** Obe majú rôzne podpisy, takže macOS vedie pre každú samostatný zoznam a samostatné oprávnenia priečinkov.

**HTML alebo iná syntax sa zobrazuje ako obyčajný kód.** Dokument mohla otvoriť zastaraná kópia Splatu. Ukončite ju, vo Finderi zvoľte pre dokument **Súbor › Informácie** a skontrolujte, či **Otvoriť v aplikácii** uvádza Splat vo vašom priečinku Aplikácie.

**V menu Splat chýba Vyhľadať aktualizácie….** Sám sa aktualizuje len verzia z obrazu disku alebo z Homebrew; vlastné zostavenie nemá aktualizátor.

**Obrázky z webu sa nezobrazujú.** Kliknite na **Načítať obrázky** v lište nad náhľadom. Načítajú sa len obrázky `https` z verejných hostiteľov.

**Vloženie snímky obrazovky vyzýva najprv uložiť.** Vložené obrázky sa ukladajú ako súbory vedľa dokumentu, na čo treba miesto na disku. Uložte dokument a vložte znova.

**„Súbor neobsahuje text v kódovaní UTF-8.“** Súbor je v inom kódovaní a nemá značku poradia bajtov, ktorá by ho určovala. Najprv ho preveďte, napríklad pomocou `iconv -f ISO-8859-1 -t UTF-8 old.md > new.md`.

**Vzorec alebo diagram zobrazuje chybu.** Kliknite naň: editor pod kódom zobrazí hlásenie MathJaxu, Mermaidu, Graphvizu alebo PlantUML.

## Pre vývojárov

Splat je aplikácia v Ruste, ktorá komunikuje s AppKit a WebKit priamo cez [objc2](https://github.com/madsmtm/objc2). Nie je tu žiadny Swift, žiadny Objective-C, žiadny projekt Xcode a žiadny súbor Interface Builderu.

### Zostavenie

Predpoklady:

- macOS s Command Line Tools (`xcode-select --install`). Xcode nie je potrebný; Splat sa zostaví bez `actool`, `ibtool` a `metal`.
- Rust 1.99 (`brew install rust`). [rust-toolchain.toml](../../../rust-toolchain.toml) uvádza presnú verziu, ktorú používa CI; rustup ju nainštaluje sám, Rust z Homebrew súbor ignoruje. Jej zvýšením sa do projektu dostanú nové upozornenia kompilátora a clippy.

[Makefile](../../../Makefile) obsahuje krátke vstupné body; `make help` ich vypíše všetky:

```sh
make build                  # target/release/Splat.app, rovnako ako cargo xtask bundle --release
make universal              # target/splat-universal pre Apple Silicon a Intel, ako ho nesie obraz disku
make check                  # formátovanie, clippy bez upozornení, všetky testy, vendorované knižnice; čo spúšťa CI
make test                   # make check, potom self-test bez okna
make test-gui               # self-testy s oknom; zaberú Mac asi na štyri minúty
make test-long              # dlhšie náhodné hľadanie a kontrola čiastočnej aktualizácie pri 10 MB
make coverage               # pokrytie riadkov všetkými testami a self-testami, s limitom pre každý modul
make coverage-library       # rovnaké limity pre moduly bez okna, ako v CI
make bench FILE=<file.md>   # časy otvorenia, písania a posúvania veľkého súboru
make release                # najprv make test, potom notarizovaný obraz disku
```

Samotná logika zostavenia je v crate Rustu [xtask](../../../xtask/src/main.rs):

```sh
cargo xtask bundle --release   # target/release/Splat.app, Hardened Runtime, podpísané "Splat Local Signing"
cargo xtask bundle             # target/debug/Splat Debug.app, samostatná aplikácia, ktorá nikdy neotvára súbory Markdown
cargo xtask dmg                # dist/Splat-<version>.dmg na distribúciu, Developer ID, notarizovaný
cargo xtask icon               # znova vytvorí assets/AppIcon.icns a ikonu README z assets/icon-1024.png
cargo xtask vendor check       # overí pribalený JavaScript voči vendor.lock
cargo xtask vendor fetch       # stiahne ho znova po zmene verzie vo vendor.lock
```

`bundle` zapíše `Info.plist`, skopíruje zdroje a lokalizácie a podpíše aplikáciu certifikátom „Splat Local Signing“. Prvé zostavenie vytvorí tento certifikát vo vašej kľúčenke prihlásenia s platnosťou 20 rokov. Kým zostane, Splat si cez nové zostavenia zachová nedávne dokumenty a oprávnenia priečinkov; s podpisom ad hoc by bolo každé zostavenie pre macOS novou aplikáciou. Zostavenia pre ladenie a self-testy majú vlastné ID balíkov (`local.splat.debug`, `local.splat.selftest`) a neregistrujú sa pre súbory Markdown, takže Finder nikdy neotvorí dokument v zastaranom zostavení a nastavenia bežnej aplikácie nechávajú na pokoji.

Verzia je vo [VERSION](../../../VERSION) ako `major.minor.build` a je to tá, ktorú dostane ďalšie zostavenie. Každé úspešné zostavenie vydania (`make build`, `cargo xtask bundle --release`, `make release`) zvýši číslo zostavenia o jedna; zostavenia pre ladenie a self-testy ho nechávajú. Hlavné a vedľajšie číslo sa zvyšujú ručne: nastavte VERSION napríklad na `1.1.0` a ďalšie zostavenie dostane presne to.

### Distribúcia

`cargo xtask dmg` (alebo `make release`, ktoré najprv spustí `make test`) zostaví vydanie, podpíše ho vaším Developer ID, notarizuje aplikáciu u Applu a pripojí tiket, zabalí ju do komprimovaného obrazu disku len na čítanie s odkazom na Aplikácie, notarizuje a označí tiketom aj obraz a výsledok skontroluje cez Gatekeeper. Aplikácia nesie vlastný tiket, aby sa kópia nainštalovaná bez Findera, ako to robí Homebrew, pri prvom spustení otvorila aj offline; dve kolá so službou notarizácie trvajú niekoľko minút každé. Len obraz, ktorý prešiel všetkými krokmi, sa stane `dist/Splat-<version>.dmg`, kópiou, ktorá sa commituje: odkazy na stiahnutie v tomto README a jeho prekladoch ju sledujú a číslo zostavenia sa zvýši. Predchádzajúci obraz zostane ešte jedno vydanie, pretože raw server GitHubu môže pár minút po pushi vydávať starý kanál aktualizácií; staršie sa presunú do koša. Neúspešný beh nechá `dist/`, README a VERSION tak, ako boli.
Každé vydanie pridá do histórie repozitára celý obraz, asi 16 MB. Beží len na požiadanie, nikdy ako súčasť bežného zostavenia, a potrebuje:

- certifikát „Developer ID Application“ v kľúčenke
- uložené prihlásenie k notárskej službe Applu s heslom špecifickým pre aplikáciu, vytvorené raz cez `xcrun notarytool store-credentials notary --apple-id <Apple ID> --team-id <team ID>`
- súkromný kľúč, ktorým sa podpisujú aktualizácie, v kľúčenke prihlásenia. Vytvoril ho `generate_keys` zo Sparkle; jeho verejná polovica je `UPDATE_KEY` v [xtask/src/main.rs](../../../xtask/src/main.rs) a vo vnútri každej nainštalovanej kópie, ktorá prijíma iba aktualizácie ním podpísané. Uchovávajte zálohu v správcovi hesiel: `target/sparkle-2.10.1/bin/generate_keys -x <file>` ho exportuje, `-f <file>` ho importuje na inom Macu. Bez neho sa už nainštalované kópie nikdy nedajú aktualizovať.
- rustup (`brew install rustup`, ktorý Rust z Homebrew ponechá)

Ak niečo z toho chýba, `dmg` sa zastaví pred zostavením. Pri prvom spustení sa macOS opýta, či smie Terminál ovládať Finder; bez tohto oprávnenia obraz disku nezobrazí ani pozadie, ani rozmiestnenie ikon.

Aktualizátor, [Sparkle](https://sparkle-project.org), nesie iba obraz disku; zostavenie ho raz stiahne do `target/` z jeho vydania na GitHube a overí voči kontrolnému súčtu v xtasku. Hardened runtime načíta framework len vtedy, keď nesie Team ID aplikácie, ktoré podpis Developer ID má a lokálny certifikát nemá; a aktualizácia nesmie nahradiť vlastné zostavenie. Zostavenie pre self-test nesie Sparkle tiež, aby voči nemu overilo menu aktualizácií: s kanálom, ktorý neexistuje, bez otázky Sparkle pri druhom spustení a s oprávnením, ktoré lokálne podpísanej aplikácii dovolí framework načítať. `dmg` podpíše nový obraz kľúčom aktualizácií a zapíše `dist/appcast.xml`, kanál, ktorý nainštalované kópie čítajú z GitHubu. Obraz, kanál a README commitnite a pushnite spolu: odvtedy nainštalované kópie ponúknu aktualizáciu.

Obraz disku je univerzálny binárny súbor pre Apple Silicon aj Intel; ostatné zostavenia sú len pre procesor hostiteľa. Rust z Homebrew dodáva štandardnú knižnicu len pre procesor hostiteľa, preto `dmg` zostavuje cez rustup, ktorý pri prvom použití nainštaluje toolchain a ciele z [rust-toolchain.toml](../../../rust-toolchain.toml) a zastaví sa, ak programu alebo Sparkle chýba kód pre niektorý z procesorov. `make universal` zostaví len program.
Aplikácia v ňom nesie iný podpis ako lokálne zostavenie, takže macOS vedie pre obe samostatné nedávne dokumenty a oprávnenia priečinkov.

### Testovanie

| Príkaz | Čo kontroluje |
|---|---|
| `cargo test --release` | jednotkové, náhodné a prekladové testy |
| `SPLAT_FUZZ_ROUNDS=20000 cargo test --release` | dlhšie náhodné hľadanie; `SPLAT_FUZZ_SEED` mení vstupy |
| `SPLAT_I18N_ONLY=fr cargo test --release i18n` | jeden jazyk, kým iné nie sú úplné |
| `cargo clippy --release --all-targets [--features selftest]` | lintery, vrátane zdôvodnenia každého bloku `unsafe`, dĺžky funkcií a v knižnici aj prevodov čísel |
| `cargo xtask vendor check` | knižnice JavaScriptu sa zhodujú s `vendor.lock` bajt po bajte |
| `cargo xtask selftest probe` | bez okna, každý s očakávaným výsledkom: vzorce, diagramy, obrázky z webu voči serveru vo vnútri procesu, blokované adresy, obrany webových zobrazení |
| `cargo xtask selftest gui [--stop-after "<check>"] [--scenarios]` | bežiaca aplikácia, kliknutie po kliknutí. Otvorí okno a píše do neho; Mac je zaneprázdnený asi štyri minúty. `--scenarios` spustí len druhú časť, asi za minútu a pol |
| `cargo xtask selftest resize` | rozšíri okno po presunutí deliacej čiary: zdrojový text sa zalomí znova, kým sa mení veľkosť okna. Trvá asi desať sekúnd |
| `cargo xtask selftest bench <file.md>` | časy otvorenia, písania a posúvania veľkého súboru |
| `cargo test --release -- --ignored partial_updates` | stlačenie klávesu pri 10 MB: čiastočná oproti úplnej aktualizácii, výsledok a časový limit |
| `cargo test --release --test hostile` | dokumenty vytvorené tak, aby pretiekli zásobník alebo trvali kvadraticky dlho, cez parsovanie, rozvrhnutie, oba exporty, stlačenia klávesov a príkazy |
| `make mutants` | chyby vnesené do `src/markdown/` a `src/preview/` jedna po druhej cez [cargo-mutants](https://mutants.rs) (`cargo install cargo-mutants`); povie, ktoré z nich žiadny test nezaznamená. Trvá hodiny; `FILE=src/preview/bridge.rs` to zúži |
| `cargo test --release --test decoder` | proces dekodéra dekóduje obrázky a nedosiahne ani súbory, ani sieť |
| `make coverage` | pokrytie riadkov pre `cargo test` a všetky self-testy cez [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (`brew install cargo-llvm-cov llvm`); zlyhá pod limitmi. Zaberie Mac asi na päť minút |
| `make coverage-library` | limity pre `src/markdown/`, `src/preview/` a `i18n.rs`, merané samotným `cargo test` |

Jednotkové testy sú vedľa kódu, ktorý testujú. `tests/` obsahuje to, čo dosahuje knižnicu zvonka: náhodné testy v `robustness.rs`, ktoré zdieľajú generátor v `src/fuzz.rs` s testami v `sync.rs`, dokumenty vytvorené tak, aby škodili, v `hostile.rs`, a test dekodéra, ktorý spúšťa zostavený program. Rozhodnutia v `src/editor/`, ktoré nepotrebujú okno, sú vo voľných funkciách s vlastnými testami: všetko, čo ovládač vie, a ako sa to mení, v `state.rs`, pravidlá vykresľovania v `render_queue.rs`, kópia textu v `sync.rs`. Krok self-testu okna, ktorý čaká zbytočne, zlyhá celý beh. Self-test okna spúšťa po hlavnom prechode klikaním druhú časť v `src/editor/selftest/scenarios.rs`: každý príkaz formátovania a tabuľky, dialógy, kliknutia na odkazy, vkladanie, pretiahnutie, exporty, veľkú zmenu, obrázky meniace sa na disku, nedostatok pamäte, obrovský dokument a obnovu po páde.

Pokrytie má dva limity: každý modul aspoň 80 % riadkov, všetky moduly spolu aspoň 85 %. `make coverage` meria `cargo test` spolu so self-testami a zlyhá pod ktorýmkoľvek z nich; limity a niekoľko výnimiek, ako háčik na panic v `main.rs`, ktorý beží len pri páde, sú v [xtask](../../../xtask/src/coverage.rs) vedľa svojich dôvodov. Merané zostavenia idú do `target/coverage/`, takže meraný kód nikdy neskončí v `target/release/` ani v obraze disku. Pomocník dekodéra otvorí svoj súbor s meraním skôr, než sa zamkne, takže sa meria tiež.

CI ([.github/workflows/ci.yml](../../../.github/workflows/ci.yml)) pri každom pushi do `main` a každom pull requeste spúšťa `make check`, `make coverage-library` a univerzálne zostavenie na Apple Silicon a `cargo test` na Intele. Každú noc [.github/workflows/nightly.yml](../../../.github/workflows/nightly.yml) spúšťa `make test-long` s novými náhodnými vstupmi. Self-testy a obraz disku zostávajú na Macu vývojára: potrebujú obrazovku a podpisové certifikáty v kľúčenke.

Výstup self-testov ide do `target/selftest/`. Self-test zostaví samostatnú aplikáciu (`target/release/Splat Selftest.app`, ID balíka `local.splat.selftest`) a nastavenia, nedávne dokumenty a priradenia súborov bežnej aplikácie nechá nedotknuté.

### Štruktúra projektu

`src/lib.rs` robí z toho, čo nepotrebuje okno, samostatnú knižnicu `splat`: `markdown/`, `preview/`, `i18n.rs` a generátor testov `fuzz.rs`. Aplikácia v `src/main.rs` na nej stavia.

- `src/markdown/`: väzba na parser (comrak), farby syntaxe a všetky príkazy ako čisté úpravy textu (`Edit`). Tiež opätovné parsovanie po úprave (`incremental.rs`), čitateľ vloženého HTML (`markup.rs`), typografia, štítky, odkazy na zdroje a citácie v súvislom texte (`typography.rs`), ako text vyzerá a ktorý Markdown a HTML to spôsobí (`style.rs`), čo smie byť adresa (`url.rs`) a export do HTML a Wordu (`html.rs`, `docx.rs` s `docx/html.rs` a `docx/xml.rs`). Náhľad a oba exporty čítajú rovnaký strom, `Doc`. Bez AppKit.
- `src/preview/`: rozvrhnutie náhľadu, vložené HTML v ňom (`layout/html.rs`) a priradenie medzi náhľadom a zdrojovým textom (`Bridge`, `Index`). Bez AppKit.
- `src/editor/`: okno. Jeden objekt `EditorController` slúži AppKit ako delegát oboch zobrazení; jeho práca je rozdelená podľa zodpovednosti:

| Modul | Zodpovednosť |
|---|---|
| `mod.rs` | trieda, metódy delegáta, `invalidate` |
| `state.rs` | čo ovládač vie o dokumente a každý spôsob, ktorým sa to mení; jeho polia sú súkromné a nedotýka sa žiadneho zobrazenia |
| `actions.rs` | register príkazov: selektor, názov, skratka, symbol, kedy sa príkaz uplatňuje |
| `editing.rs` | spúšťanie a overovanie príkazov, písanie v náhľade |
| `update.rs` | parsovanie, farbenie, aktualizácia náhľadu; po úprave len dotknuté odseky |
| `sync.rs` | kópia textu a evidencia blokov, bez AppKit a s náhodnými testami |
| `progressive.rs` | vykresľovanie veľkých náhľadov po krokoch |
| `selection.rs`, `scroll.rs` | výber a poloha posunu oboch panelov v súlade |
| `atom.rs`, `atom_edit.rs` | editor vzorcov a diagramov |
| `navigation.rs`, `links.rs` | odkazy, wiki odkazy, štítky, zaškrtávacie políčka úloh, zbaľovanie, poznámky pod čiarou |
| `env.rs` | čo náhľad číta zvonka textu: typografická interpunkcia, repozitár pre `@name` a `#12`, súbor BibTeX, vložené poznámky |
| `render.rs`, `images.rs`, `remote.rs`, `safe_image.rs` | text náhľadu, lokálne a vzdialené obrázky |
| `attributes.rs` | atribúty textu; každý kľúč dostane len hodnotu očakávaného typu |
| `shadow.rs` | kópie neuloženého textu na obnovu po páde |
| `decoder.rs` | pomocné procesy v sandboxe, ktoré dekódujú obrázky a grafiku |
| `export.rs`, `outline.rs` | export do HTML a Wordu; bočný panel s osnovou |
| `js.rs`, `mathfield.rs`, `render_queue.rs` | WebKit pre MathJax, Mermaid, Graphviz, PlantUML a vizuálny editor vzorcov; pravidlá, čo sa kedy vykresľuje, nezávisle od WebKitu |
| `webkit.rs` | volania do WebKitu; s `js.rs` a `mathfield.rs` zostáva len životnosť delegáta navigácie |
| `window.rs`, `toolbar.rs`, `views.rs`, `dialogs.rs`, `style.rs` | okno, panel s nástrojmi, textové zobrazenia, dialógy, farby |

- `src/menu.rs`, `src/app.rs`, `src/document.rs`: lišta menu, delegát aplikácie, dokument (`MDDocument`, podtrieda `NSDocument`).
- `src/i18n.rs` s `resources/*.lproj`: 35 jazykov; kľúčmi sú nemecké texty.
- `src/large_alloc.rs`: veľké alokácie idú priamo z jadra a po uvoľnení sa mu vrátia, takže sa pamäť po veľkých úpravách vráti.
- `resources/js/`: webové stránky a knižnice; verzie a kontrolné súčty vo [vendor.lock](../../../vendor.lock).
- `xtask/`: zostavovanie, podpisovanie, obraz disku, ikona, vendorovanie, spúšťač self-testov a limity pokrytia.

### Tok dát

Zdrojový text v ľavom textovom zobrazení je jediným zdrojom pravdy.

1. Každá zmena znaku tam aktualizuje kópiu textu (`Snapshot`) na mieste, bez opätovného čítania textu.
2. Aktualizácia parsuje kópiu, prefarbí zdrojový text len tam, kde sa farby zmenili, rozvrhne náhľad a nahradí len bloky, ktorých obsah sa zmenil.
3. Po úprave sa znova parsujú, farbia a rozvrhujú len dotknuté odseky, pokiaľ ich susedia zostávajú rovnakí; `reparse_part_and_color` povie, kedy nie. Dokumenty s blokmi komentárov `%%`, zmeny front matter a zmeny blízko definícií referenčných odkazov sa spracúvajú celé. comrak rozpozná odkaz na poznámku pod čiarou len s jej definíciou v tom istom parsovaní a presúva definície za text, preto sa časť parsuje so zástupnou definíciou pre každý štítok a čísla preberá zo starého stromu; zmena odkazov alebo definícií v časti, alebo vedľa poznámky pod čiarou v texte, sa spracúva celá.
4. Bloky HTML, ktoré otvoria `<details>` alebo `<div align>` a zatvoria ho neskôr, ovplyvňujú bloky Markdownu medzi nimi; opätovne použitý blok musí mať rovnaké okolie.
5. Počas písania v zdrojovom texte dokumentu do 2 MB beží čiastočná aktualizácia okamžite; keď sa neuplatní, alebo vo väčšom dokumente, parsovanie a rozvrhnutie bežia na pozadí. Počas písania v náhľade bežia okamžite, pretože kurzor náhľadu závisí od výsledku.
6. Vstup v náhľade `Bridge` preloží na úpravu zdrojového textu a odtiaľ ide rovnakou cestou. Markdown má príliš veľa spôsobov, ako zmeniť text na syntax, aby stačili samotné pravidlá, preto `preview/verify.rs` zmenenú časť parsuje znova a porovná, čo zobrazuje, s tým, čo sa napísalo; úprava, ktorá nesedí, sa ošetrí ďalej alebo odmietne.
7. Pozície v texte z comraku sú na niekoľkých miestach chybné, napríklad za definíciou referenčného odkazu. `markdown/positions.rs` ich pri čítaní bloku overí voči zdrojovému textu a tam, kde nesedia, ich prečíta znova zo zdrojového textu.

### Pravidlá v ovládači

1. Vlastné zmeny zobrazení v ovládači bežia vnútri `silently()`; spätné volania delegáta ich ignorujú.
2. `state` sa vypožičiava len na čítanie a výpočet, nikdy cez volanie, ktoré zobrazenie mení, rozvrhuje alebo posúva. Mení sa len vlastnými metódami, ktorých testy nepotrebujú okno.
3. Správy zvonka (načítaný obrázok, vykreslený vzorec) prichádzajú oneskorene cez `renders_arrived` a `image_arrived`, nikdy uprostred aktualizácie.
4. Prekreslenie bez zmeny textu ide len cez `invalidate(reason)`.
5. `Rc<Snapshot>` sa nedrží cez zmenu zdrojového textu, inak táto zmena skopíruje celý text.
6. Nový príkaz potrebuje záznam v `actions.rs`, metódu v `mod.rs` a vetvu v `editing::perform`.
7. Každý blok `unsafe` nesie svoje zdôvodnenie ako komentár `// SAFETY:`. Bajty sa k systémovým dekodérom obrázkov v aplikácii dostanú len cez `decoder::system_image`.
8. Nič na hlavnom vlákne nečaká na priečinky používateľa, pretože sieťový zväzok môže prestať odpovedať: súbory sa preverujú na vlákne `DISK` a náhľad číta odpovede cez `Env::known`. Priamo na disk smie siahnuť len akcia, na ktorú používateľ čaká, ako export alebo ukladanie.

### Pridanie príkazu

Lišta menu, panel s nástrojmi aj kontextové menu budujú svoje položky z registra v [src/editor/actions.rs](../../../src/editor/actions.rs) a validácia menu tam číta, kedy sa príkaz uplatňuje a či zobrazuje začiarknutie. Nový príkaz potrebuje:

1. variant `Action` s jeho `Spec`: selektor, názov, popis na paneli s nástrojmi, tooltip (začínajúci slovesom, najviac 75 znakov), skratka, SF Symbol a kedy sa uplatňuje
2. metódu v `src/editor/mod.rs`
3. vetvu v `editing::perform`
4. jeho texty v každom `Localizable.strings`

Test `every_command_has_a_method_and_every_method_a_command` kontroluje, či register a metódy zodpovedajú. Štandardné skratky Applu ako ⌘E, ⌘J, ⌘T, ⌘;, ⇧⌘C, ⌥⌘C, ⌥⌘V, ⇧⌘P, ⌥⌘T, ⌃⌘F a ⌘, zostávajú nedotknuté.

### Preklady

Kód používa nemecký text ako kľúč: `t("Fett")` ho vyhľadá za behu, `n("Fett")` označí konštantu na neskoršie vyhľadanie. Každý `resources/<language>.lproj/` obsahuje:

- `Localizable.strings`: každý text rozhrania, pojmy menu podľa glosárov Applu
- `InfoPlist.strings`: názvy, ktoré macOS zobrazuje pre aplikáciu a jej typ dokumentu
- `QuickReference.md`: stručný prehľad Markdownu

`cargo xtask bundle` uvedie každý priečinok `.lproj` ako lokalizáciu a `CFBundleDevelopmentRegion` je angličtina, takže systémové menu a dialóg ukladania sledujú ten istý jazyk. Test i18n kontroluje, či má každý jazyk každý text a či má každý stručný prehľad osnovu nemeckého.

`assets/readme/<language>/` obsahuje toto README v danom jazyku, jeho snímku obrazovky a ukážkový dokument, ktorý snímka zobrazuje. Zmena tohto README patrí aj do týchto kópií.

### Komponenty tretích strán

| Komponent | Verzia | Na čo sa používa | Licencia |
|---|---|---|---|
| [comrak](https://github.com/kivikakk/comrak) | 0.59 | parser Markdownu | BSD-2-Clause |
| [objc2](https://github.com/madsmtm/objc2), objc2-foundation, block2 | 0.6 / 0.3 | runtime Objective-C, Foundation, bloky | MIT |
| objc2-app-kit, objc2-web-kit, dispatch2 | 0.3 | väzby na AppKit, WebKit a Grand Central Dispatch | Zlib, Apache-2.0 alebo MIT |
| [svg2pdf](https://github.com/typst/svg2pdf) | 0.13 | vzorce a obrázky SVG ako vektorové PDF | MIT alebo Apache-2.0 |
| [image](https://github.com/image-rs/image) | 0.25 | dekódovanie PNG, JPEG, GIF, WebP a BMP | MIT alebo Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2 | zarovnávanie stĺpcov tabuliek | MIT alebo Apache-2.0 |
| [entities](https://github.com/p-jackson/entities) | 1 | názvy odkazov na znaky HTML | MIT |
| [MathJax](https://www.mathjax.org) | 3.2.2 | vykresľovanie vzorcov | Apache-2.0 |
| [MathLive](https://cortexjs.io/mathlive/) | 0.111.1 | vizuálny editor vzorcov, s písmami KaTeX | MIT |
| [Mermaid](https://mermaid.js.org) | 12.2.0 | diagramy Mermaid | MIT |
| [Viz.js](https://github.com/mdaines/viz-js) | 3.24.0 | Graphviz skompilovaný do WebAssembly, obsahuje Graphviz (EPL-1.0) a Expat (MIT) | MIT |
| [PlantUML](https://github.com/plantuml/plantuml) (`@plantuml/core`) | 1.2026.6 | diagramy PlantUML, skompilované do JavaScriptu | MIT |
| [Sparkle](https://sparkle-project.org) | 2.10.1 | aktualizácie verzie z obrazu disku | MIT, s časťami pod licenciami BSD a zlib |

Knižnice JavaScriptu sú uložené v `resources/js/` presne tak, ako boli publikované; [vendor.lock](../../../vendor.lock) zaznamenáva ich zdroje a kontrolné súčty SHA-256. Texty ich licencií, vrátane licencií Graphvizu a Expatu vo vnútri Viz.js, sú vedľa nich ako `resources/js/LICENSE-*` a dodávajú sa v aplikácii. Sparkle nie je uložený v repozitári: `cargo xtask dmg` stiahne jeho vydanie a overí ho voči kontrolnému súčtu v xtasku a jeho licencia sa dodáva v aplikácii ako `LICENSE-sparkle`.

## Licencia

Splat je licencovaný podľa jednej z týchto licencií

- Licencia Apache, verzia 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE))
- Licencia MIT ([LICENSE-MIT](../../../LICENSE-MIT))

podľa vášho výberu.

Ak výslovne neuvediete inak, každý príspevok, ktorý ste zámerne odovzdali na zahrnutie do Splatu, ako je definované v licencii Apache-2.0, bude duálne licencovaný ako vyššie, bez ďalších podmienok.
