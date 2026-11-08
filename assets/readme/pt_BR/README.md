<p><img src="../../readme-icon.png" alt="Ícone do app Splat" width="168" height="168">&nbsp;&nbsp;&nbsp;<img src="screenshot.png" alt="Splat com Demo.md: o código-fonte Markdown à esquerda, a pré-visualização editável à direita" width="274" height="168"></p>

# Splat

**O editor de Markdown para macOS que deixa você escrever no código-fonte ou direto na pré-visualização.**

O Splat mostra à esquerda o código-fonte Markdown com cores de sintaxe e, à direita, uma pré-visualização ao vivo e totalmente editável. Digite em qualquer uma: tudo o que você escreve na pré-visualização vira uma alteração mínima no código-fonte, de modo que as linhas que você não tocou continuam idênticas byte a byte e seus diffs do Git ficam limpos. O Splat entende o Markdown que você já usa: CommonMark e GitHub Flavored Markdown, alertas do GitHub, wiki links, embeds e caixas de informação do Obsidian, extensões do Pandoc e do Typora, HTML embutido, fórmulas LaTeX e diagramas Mermaid, Graphviz e PlantUML, tudo renderizado offline. Navegue e busque em uma pasta inteira de notas, veja o que mudou desde o último commit do Git, cole do Word ou de uma página da web como Markdown e exporte para PDF, HTML ou Word com um clique. Ele é escrito em Rust sobre controles AppKit genuínos, fala 35 idiomas e não envia nada pela rede, a menos que você peça.
 


[![Baixar o Splat para macOS](https://img.shields.io/badge/Download-Splat_for_macOS-0A84FF?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)

**[Baixar Splat.dmg](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)**: assinado e notarizado pela Apple, para macOS 14 ou posterior, em Apple Silicon e Intel.

**Encontrou um bug?** O Splat mantém idênticas byte a byte as linhas que você não tocou. Se mesmo assim alguma mudar, ou um arquivo for exibido errado, [abra uma issue](https://github.com/sventmueller/splat/issues/new/choose) e anexe o arquivo. Se o Splat for útil para você, uma [estrela no GitHub](https://github.com/sventmueller/splat) ajuda outras pessoas a encontrá-lo.
 
 

[![Rust edição 2024](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![macOS 14 ou posterior](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)
![Universal: Apple Silicon e Intel](https://img.shields.io/badge/Universal-Apple_Silicon_%7C_Intel-333333?logo=apple&logoColor=white)
[![AppKit via objc2](https://img.shields.io/badge/AppKit-objc2_0.6-147EFB?logo=apple&logoColor=white)](https://github.com/madsmtm/objc2)
[![WebKit](https://img.shields.io/badge/WebKit-offline-0A84FF?logo=safari&logoColor=white)](https://webkit.org)
[![Comrak 0.59](https://img.shields.io/badge/comrak-0.59-DEA584?logo=markdown&logoColor=white)](https://github.com/kivikakk/comrak)
[![CommonMark e GFM](https://img.shields.io/badge/CommonMark-GFM-24292F?logo=github&logoColor=white)](https://github.github.com/gfm/)
[![MathJax 4.1.3](https://img.shields.io/badge/MathJax-4.1.3-1E6E9E?logo=latex&logoColor=white)](https://www.mathjax.org)
[![MathLive 0.111.4](https://img.shields.io/badge/MathLive-0.111.4-5A3FC0)](https://cortexjs.io/mathlive/)
[![Mermaid 12.3.0](https://img.shields.io/badge/Mermaid-12.3.0-FF3670?logo=mermaid&logoColor=white)](https://mermaid.js.org)
[![Graphviz via Viz.js 3.24](https://img.shields.io/badge/Graphviz-Viz.js_3.24-2596BE)](https://graphviz.org)
[![PlantUML 1.2026.9](https://img.shields.io/badge/PlantUML-1.2026.9-A80036)](https://plantuml.com)
![35 idiomas de interface](https://img.shields.io/badge/languages-35-4C9A2A)
![Offline por padrão](https://img.shields.io/badge/network-offline_by_default-4C9A2A)
![Assinado com Hardened Runtime](https://img.shields.io/badge/Hardened_Runtime-signed-4C9A2A)
[![Licença: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-4C9A2A)](#licença)

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
**Português (Brasil)** ·
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

## Conteúdo

- [Recursos em resumo](#recursos-em-resumo)
- [Requisitos](#requisitos)
- [Instalação](#instalação)
- [Primeiros passos](#primeiros-passos)
- [Escrita](#escrita)
  - [No código-fonte](#no-código-fonte)
  - [Na pré-visualização](#na-pré-visualização)
  - [Comandos de formatação](#comandos-de-formatação)
  - [Listas e tarefas](#listas-e-tarefas)
  - [Tabelas](#tabelas)
  - [Colar texto formatado](#colar-texto-formatado)
  - [Metadados](#metadados)
- [Markdown suportado](#markdown-suportado)
  - [CommonMark e GitHub Flavored Markdown](#commonmark-e-github-flavored-markdown)
  - [Caixas de informação](#caixas-de-informação)
  - [Sintaxe estendida](#sintaxe-estendida)
  - [Obsidian](#obsidian)
  - [Citações e referências](#citações-e-referências)
  - [HTML embutido](#html-embutido)
  - [Pontuação tipográfica](#pontuação-tipográfica)
  - [Resumo da sintaxe](#resumo-da-sintaxe)
- [Fórmulas](#fórmulas)
- [Diagramas](#diagramas)
- [Imagens](#imagens)
- [Links e navegação](#links-e-navegação)
- [Notas em uma pasta](#notas-em-uma-pasta)
- [Alterações desde o último commit](#alterações-desde-o-último-commit)
- [Exportar, imprimir e compartilhar](#exportar-imprimir-e-compartilhar)
- [Importar documentos do Word](#importar-documentos-do-word)
- [Aparência e acessibilidade](#aparência-e-acessibilidade)
- [Idiomas](#idiomas)
- [Atalhos de teclado](#atalhos-de-teclado)
- [Privacidade e segurança](#privacidade-e-segurança)
- [Documentos grandes](#documentos-grandes)
- [Limitações conhecidas](#limitações-conhecidas)
- [Solução de problemas](#solução-de-problemas)
- [Para desenvolvedores](#para-desenvolvedores)
  - [Componentes de terceiros](#componentes-de-terceiros)
- [Licença](#licença)

## Recursos em resumo

- **Duas formas de escrever, um só texto.** O código-fonte e a pré-visualização são editáveis. O arquivo Markdown é a única verdade: há um histórico de desfazer por documento, e cada edição na pré-visualização é traduzida na menor alteração possível no código-fonte.
- **O Markdown que você já usa.** CommonMark, GitHub Flavored Markdown, alertas do GitHub, extensões do Obsidian, do Pandoc e do Typora, shortcodes de emoji, spoilers, listas de definição, âncoras de título e HTML como nos README do GitHub.
- **Fórmulas e diagramas integrados.** Fórmulas LaTeX via MathJax, com um editor visual de fórmulas. Diagramas em Mermaid, Graphviz e PlantUML, renderizados no seu Mac, sem Java, servidor ou conexão de rede.
- **Um botão para tudo.** Todo elemento do Markdown tem um botão na barra de ferramentas e um item de menu, e os mais comuns também um atalho de teclado. As tabelas têm um menu próprio para linhas, colunas e alinhamento.
- **Uma pasta, não só um arquivo.** A barra lateral lista as notas em torno de um documento e busca em todas elas. Renomear uma nota altera os links para ela, e todos os bytes ao redor deles continuam iguais.
- **Alterações em um relance.** Em um repositório Git, barras ao lado do texto marcam o que difere do último commit, nos dois painéis.
- **Do Word e da web para o Markdown.** Cole texto formatado como Markdown, células de uma planilha como tabela, e importe documentos do Word inteiros com suas imagens.
- **Exporte sem perder nada.** PDF, um único arquivo HTML autossuficiente ou um documento do Word com títulos, listas, tabelas, notas de rodapé e imagens de verdade. Adicione números de página, cabeçalhos e uma página de rosto, ou use o modelo do Word da sua empresa.
- **Nativo até a raiz.** Janelas, abas, menus, popovers, Versões, Salvamento Automático, menu de compartilhamento, modo escuro, verificação ortográfica, ditado e VoiceOver autênticos do AppKit. Sem Electron, sem web view para o texto em si.
- **Rápido com arquivos grandes.** Renderizar uma tecla pressionada em um documento de 10 MB leva cerca de 10 ms, antes do trabalho da própria view.
- **Privado e cuidadoso.** O Splat roda no App Sandbox, as imagens da web só carregam depois que você permite, links para outros apps perguntam antes, e tudo o que decodifica dados de imagem não confiáveis roda em um serviço próprio isolado em sandbox.
- **No seu idioma.** A interface e a referência rápida de Markdown integrada estão disponíveis em 35 idiomas: alemão, árabe, catalão, checo, chinês (simplificado), chinês (tradicional), coreano, croata, dinamarquês, eslovaco, esloveno, espanhol, finlandês, francês, grego, hebraico, hindi, holandês, húngaro, indonésio, inglês, italiano, japonês, malaio, norueguês, polonês, português (Brasil), português (Portugal), romeno, russo, sueco, tailandês, turco, ucraniano e vietnamita.

## Requisitos

- macOS 14 Sonoma ou posterior. O Splat é desenvolvido e testado no macOS 27, onde adota o design Liquid Glass automaticamente.
- Um Mac com Apple Silicon ou processador Intel.
- Cerca de 45 MB de espaço em disco.

## Instalação

**Pela imagem de disco.** Baixe [`Splat.dmg`](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg), abra-o e arraste o Splat para a pasta Aplicativos. A imagem é assinada com um Developer ID e notarizada pela Apple, então abre sem avisos.

**Com o Homebrew.** O cask fica em um tap próprio:

```sh
brew install --cask sventmueller/tap/splat
```

`brew upgrade --cask splat` instala novas versões, e `brew uninstall --zap --cask splat` também remove os ajustes e os caches do Splat.

**Atualizações.** O Splat instalado pela imagem de disco ou pelo Homebrew se mantém atualizado com o [Sparkle](https://sparkle-project.org). Na segunda abertura, ele pergunta se pode buscar atualizações por conta própria, uma vez por dia. **Splat › Buscar Atualizações…** busca na hora; **Buscar Atualizações Automaticamente** e **Baixar e Instalar Atualizações Automaticamente** no mesmo menu mudam sua escolha depois. O Splat só instala uma atualização se ela tiver a assinatura do desenvolvedor. Uma versão compilada por você não tem atualizador.

**Pelo código-fonte.** Veja [Compilação](../../../docs/development.md#building) para os pré-requisitos e depois:

```sh
make build
ditto target/release/Splat.app /Applications/Splat.app
open /Applications/Splat.app
```

Para abrir arquivos Markdown com o Splat por duplo clique, selecione qualquer arquivo `.md` no Finder, escolha **Arquivo › Obter Informações**, selecione o Splat em **Abrir com** e clique em **Alterar Todos…**. O Splat se registra para arquivos `.md`, `.markdown`, `.mdown` e `.mkd`.

## Primeiros passos

### A janela

Cada documento abre em sua própria janela, ou como aba, se você preferir abas:

- **À esquerda: o código-fonte.** Seu Markdown em SF Mono com cores de sintaxe.
- **À direita: a pré-visualização.** O documento renderizado, pronto para editar.
- **Barra lateral (opcional).** Todos os títulos em forma de árvore, um clique em um deles o mostra nos dois painéis: **Visualizar › Mostrar Estrutura** (⌃⌘S). Outras duas abas contêm as [notas da pasta](#notas-em-uma-pasta) e uma busca nelas. O botão ao lado do título da janela mostra e oculta a barra, e as novas janelas a abrem como a última janela a deixou.
- **Barra de ferramentas.** Botões agrupados pelo que alteram: caracteres, parágrafos e elementos inseridos, depois pré-visualização ligada/desligada, Compartilhar e Novo Documento. Personalize-a com **Visualizar › Personalizar Barra de Ferramentas…**.

Arraste o divisor para dar mais espaço a qualquer um dos lados. **Visualizar › Ocultar Pré-visualização** (⌥⌘P) deixa só o código-fonte; o Splat lembra disso por janela. Os dois painéis rolam juntos, e uma seleção em um painel é destacada no outro.

O canto da janela mostra as palavras e os caracteres do texto e quanto tempo leva para lê-lo, e os da seleção enquanto houver uma. Endereços de links, tags HTML, metadados, comentários, fórmulas e diagramas não contam. Essa informação fica oculta até que **Visualizar › Mostrar Contagem de Palavras** a ative.

### Documentos, abas e versões

O Splat é um app de documentos comum do macOS:

- **Salvamento Automático e Versões.** As alterações são salvas no próprio arquivo. **Arquivo › Reverter Para › Explorar Todas as Versões…** leva você de volta no tempo.
- **Alterações de outros programas.** Quando o Git, um script ou outro editor altera um documento aberto, o Splat o recarrega e mantém o cursor e a posição de rolagem. Se você tiver alterações próprias não salvas, o Splat as mantém, e o macOS pergunta antes de sobrescrever a outra versão.
- **Os comandos de arquivo de sempre.** Duplicar (⇧⌘S), Renomear…, Mover Para… e **Arquivo › Abrir Recente**.
- **Abas.** **Arquivo › Nova Aba** ou o botão **+** na barra de ferramentas abre um novo documento como aba; o menu Janela padrão combina, move e alterna as abas.
- **Posição das janelas.** O tamanho e a posição da sua última janela são lembrados, inclusive entre reinicializações. Se já houver uma janela ali, a nova abre um pouco deslocada.
- **O ícone do Dock.** Um clique nele depois de você fechar a última janela abre um novo documento.
- **Abrir arquivos Markdown no Splat.** **Splat › Definir como App Padrão para Markdown** faz um duplo clique em um arquivo `.md` ou `.markdown` abrir o Splat; o macOS pede uma confirmação, uma única vez. O item mostra uma marca de seleção enquanto o Splat for o padrão.

Os arquivos precisam ser texto UTF-8 ou UTF-16 com marca de ordem de bytes. O Splat recusa outras codificações em vez de adivinhar e corromper o texto. A marca de ordem de bytes permanece, UTF-16 continua UTF-16, e em um arquivo cujas linhas terminam todas em CRLF, as linhas que você adiciona também recebem CRLF.

**Depois de uma falha**, no máximo os últimos segundos de digitação se perdem: a cada cinco segundos, o Splat copia o texto não salvo para `~/Library/Application Support/Splat/Unsaved` e remove as cópias quando o documento é salvo ou o Splat é encerrado normalmente. Na próxima abertura, o que uma falha deixou ali é movido para `~/Library/Application Support/Splat/Recovery`, e o Splat avisa uma única vez, com um botão que abre a pasta. Uma falha à qual o Splat ainda consegue reagir também grava ali o texto de cada documento aberto.

### Referência rápida

**Ajuda › Referência Rápida de Markdown** abre uma folha de consulta no seu idioma como um documento novo e sem título. É um documento comum, então experimente tudo ali mesmo, no código-fonte ou na pré-visualização.

## Escrita

### No código-fonte

O código-fonte é um editor de texto simples ajustado para Markdown:

- **Cores de sintaxe** como no VS Code: títulos, ênfase, links, código, tabelas, HTML, front matter, e LaTeX e Mermaid dentro de fórmulas e diagramas. Há paletas para os modos claro e escuro e para Aumentar Contraste.
- **Blocos de código delimitados** são destacados para Bash, C, C++, C#, CSS (também SCSS e Less), Go, HTML, XML, Java, JavaScript, JSON, Kotlin, PHP, Python, Ruby, Rust, SQL, Swift, TypeScript, YAML, TeX e Mermaid, inclusive com os nomes curtos usuais, como `js`, `py` ou `sh`.
- **Sem substituições surpresa.** Aspas inteligentes, hifens inteligentes e substituição de texto vêm desligados, porque quebram `--`, `---` e `"`. Ligue-os em **Editar › Substituições**.
- **As ferramentas de escrita do sistema.** Verificação de ortografia e gramática, ditado, Buscar e Substituir (⌥⌘F) e Fala.
- **Arrastar e soltar.** Solte um arquivo de imagem para inserir `![nome](relative/path.png)`, qualquer outro arquivo para inserir um link para ele e uma URL para inserir `[url](url)`. Cada soltura é um passo de desfazer.
- **Colar imagens.** Uma imagem na área de transferência, como uma captura de tela, é salva como arquivo PNG ao lado do documento (`Imagem 2026-10-29 13.09.07.png`) e inserida como imagem. Para isso, o documento precisa ter sido salvo uma vez.

### Na pré-visualização

A pré-visualização se comporta como um processador de texto, mas tudo o que você faz nela acaba como Markdown:

- **Digitação.** Caracteres que significam algo no Markdown, como `*` ou `#`, recebem escape para aparecer como foram digitados.
- **Return** inicia um novo parágrafo ou item de lista. **⌥Return** ou **⌃Return** insere uma quebra de linha.
- **Formatação no cursor.** Com o cursor no fim de uma palavra, Negrito, Itálico e os outros formatos de caractere valem para o que você digitar em seguida. Escolha o formato de novo para parar. Dentro de uma palavra, a palavra inteira é formatada.
- **Linhas vazias.** Return em uma linha vazia adiciona outra linha vazia, e Delete a remove. No código-fonte, cada linha vazia extra equivale a mais duas linhas em branco; outros programas de Markdown as mostram como espaçamento normal entre parágrafos.
- **Desfazer** vale para os dois painéis. Os itens de desfazer têm nome (“Desfazer Negrito”), a digitação é um passo por sequência de teclas, e depois de Desfazer e Refazer os dois painéis rolam até a alteração.
- **Fórmulas e diagramas** aparecem como gráficos prontos. Clique em um, leve o cursor até ele ou pressione Return sobre ele para abrir seu editor (veja [Fórmulas](#fórmulas) e [Diagramas](#diagramas)).
- **HTML embutido, notas embutidas e front matter** são exibidos, mas você os edita no código-fonte. Um clique neles coloca o cursor no lugar correspondente lá.

Tudo o que o Markdown não consegue expressar é bloqueado na pré-visualização: os painéis Fontes e Cores, a régua e a colagem ou soltura de texto rico e imagens. HTML e RTF colados chegam como texto simples, a menos que você escolha [Colar como Markdown](#colar-texto-formatado).

### Comandos de formatação

Todos os comandos estão disponíveis na barra de ferramentas, na barra de menus e no menu de contexto dos dois painéis. Os itens de menu mostram uma marca de seleção para a formatação no cursor.

| Menu | Comandos |
|---|---|
| **Formatar › Fonte** | Negrito, Itálico, Sublinhado, Tachado, Destaque, Código, Sobrescrito, Subscrito |
| **Formatar › Título** | Corpo de Texto, Título 1 a 6 |
| **Formatar › Lista** | Lista com Marcadores, Lista Numerada, Lista de Tarefas |
| **Formatar** | Citação, Bloco de Código (Sem Linguagem ou uma de 20 linguagens; também altera a linguagem de um bloco existente), Caixa de Informação (Nota, Dica, Importante, Aviso, Cuidado; o mesmo tipo de novo a remove), Editar Fórmula ou Diagrama…, Limpar Formatação |
| **Inserir** | Link…, Imagem…, Tabela…, Fórmula, Diagrama (Fluxograma, Diagrama de Sequência, Gráfico de Gantt), Índice, Nota de Rodapé, Separador |
| **Tabela** | Inserir Linha Acima/Abaixo, Inserir Coluna à Esquerda/à Direita, Apagar Linha, Apagar Coluna, Alinhamento (À Esquerda, Centralizar, À Direita, Padrão), Alinhar Colunas da Tabela |
| **Visualizar** | Ocultar/Mostrar Pré-visualização, Mostrar/Ocultar Estrutura, Pontuação tipográfica, Ampliar/Reduzir, Tamanho Real, barra de ferramentas, barra de abas, tela cheia |

Sublinhado, destaque, sobrescrito e subscrito são escritos como `<ins>`, `<mark>`, `<sup>` e `<sub>`, que o GitHub também exibe. O Splat também lê `<u>`, que o GitHub remove, e as formas mais curtas `==destaque==`, `^sup^` e `~sub~`. O GitHub mostra `==` e `^` como digitados e `~sub~` como tachado.

### Listas e tarefas

- Return continua uma lista; Return em um item vazio a encerra.
- **Tab** e **⇧Tab** aumentam e diminuem o recuo dos itens de lista na pré-visualização.
- Um clique na caixa de seleção de uma tarefa na pré-visualização alterna `- [ ]` e `- [x]`.
- **Formatar › Lista** transforma os parágrafos selecionados em uma lista ou muda o tipo dela.

### Tabelas

- **Inserir › Tabela…** pergunta o número de linhas (incluindo o cabeçalho) e de colunas.
- Na pré-visualização, **Tab** vai para a próxima célula, **⇧Tab** para a anterior e **Return** para a célula abaixo. Tab na última célula adiciona uma linha.
- O menu **Tabela** insere e apaga linhas e colunas e define o alinhamento de uma coluna (⌘{, ⌘|, ⌘}).
- **Tabela › Alinhar Colunas da Tabela** preenche o código-fonte com espaços para que as colunas fiquem alinhadas.

### Colar texto formatado

**Editar › Colar como Markdown** (⌃⌘V), também no menu de contexto dos dois painéis, transforma o que está na área de transferência em Markdown na seleção, como um único passo de desfazer. ⌘V continua inserindo texto simples.

- **De um navegador, Word, Google Docs, Confluence, Notas ou Pages:** títulos, negrito, itálico, sublinhado, tachado, sobrescrito e subscrito, links, listas com seu aninhamento, citações, código e tabelas vêm junto.
- **De uma planilha:** as células viram uma tabela com linha de cabeçalho e colunas de números alinhadas à direita. O mesmo vale para texto separado por tabulações, ponto e vírgula ou vírgulas, se todas as linhas tiverem o mesmo número de colunas.
- **Imagens** com endereço da web continuam sendo links. Uma imagem dentro do HTML colado vira um arquivo ao lado do documento, que para isso precisa ter sido salvo uma vez. Os arquivos de imagem temporários do Word não são trazidos, e o texto alternativo deles permanece.
- **Links** mantêm apenas endereços `http`, `https`, `mailto`, relativos e `#`; de qualquer outro link fica o texto.

### Metadados

**Formatar › Editar Metadados…**, ou um clique na linha de metadados no topo da pré-visualização, abre um formulário com os campos do front matter YAML. Os campos podem ser alterados, adicionados e removidos. Um campo com uma lista a mantém como lista: seus valores ficam um depois do outro, separados por vírgulas, e um valor que contém uma vírgula fica entre aspas. Um campo novo chamado `tags`, `aliases`, `keywords`, `categories`, `authors` ou `cssclasses` também vira uma lista. Campos aninhados e texto de várias linhas podem ser vistos ali e são editados no código-fonte.

O que o formulário altera é um único passo de desfazer e atinge apenas as linhas dos campos que você alterou: comentários, a ordem, espaçamentos incomuns, aspas e finais de linha de todos os outros continuam idênticos byte a byte. Um documento sem metadados ganha um bloco no início.

## Markdown suportado

O Splat faz a análise com o [comrak](https://github.com/kivikakk/comrak), um parser compatível com CommonMark e GFM, e ativa a maioria das extensões dele. Tudo o que está abaixo é renderizado na pré-visualização e em todas as exportações.

### CommonMark e GitHub Flavored Markdown

- Títulos (`#` e sublinhados), parágrafos, quebras de linha, ênfase, ênfase forte, código em linha, citações em bloco, separadores.
- Listas com marcadores, numeradas e aninhadas; listas de tarefas.
- Links (em linha, de referência e automáticos), imagens, definições de referência de link.
- Blocos de código delimitados e recuados com cores de sintaxe.
- Tabelas com alinhamento de colunas.
- Tachado com `~~dois til~~`.
- Notas de rodapé: `texto[^1]` com `[^1]: A nota.` em qualquer lugar do documento. Um clique no marcador leva até a nota.
- Front matter YAML entre linhas `---` no início. A pré-visualização o resume em uma linha, como “Metadados: title, date, tags”.

### Caixas de informação

Os alertas do GitHub são renderizados como caixas coloridas com ícone e título:

```markdown
> [!NOTE]
> Informação útil que os usuários devem conhecer.

> [!WARNING]
> Conteúdo crítico que exige atenção imediata.
```

Os cinco tipos são Nota, Dica, Importante, Aviso e Cuidado. **Formatar › Caixa de Informação** transforma o parágrafo no cursor em uma delas.

As caixas de informação do Obsidian também funcionam, com qualquer título e um estado de recolhimento:

```markdown
> [!tip]- Clique em ▸ para expandir
> Oculto até ser aberto.

> [!question]+ Aberto por padrão
> Um `+` começa expandido, um `-` recolhido.
```

Os outros tipos do Obsidian são exibidos na cor mais próxima entre as cinco: `success`, `check`, `done` e `hint` como Dica; `question`, `help` e `faq` como Importante; `attention` como Aviso; `failure`, `fail`, `missing`, `danger`, `error` e `bug` como Cuidado; todo o resto como Nota.

### Sintaxe estendida

| Recurso | Sintaxe | Origem |
|---|---|---|
| Destaque | `==texto marcado==` | Obsidian, Typora |
| Sobrescrito | `x^2^` | Pandoc, Typora |
| Subscrito | `H~2~O` | Pandoc, Typora |
| Shortcode de emoji | `:rocket:` vira 🚀 | GitHub |
| Spoiler | `\|\|texto oculto\|\|`, selecione-o para ler | Discord |
| Nota de rodapé em linha | `texto^[a própria nota]` | Pandoc, Obsidian |
| Lista de definição | `Termo`, linha em branco, `: Explicação` | PHP Markdown Extra, Pandoc |
| Âncora de título | `## Instalar {#setup}`, link com `[veja](#setup)` | Pandoc, kramdown |

As listas de definição ficam assim no código-fonte:

```markdown
Termo

: Explicação do termo.
```

Sem uma âncora personalizada, cada título recebe uma derivada do próprio texto, como no GitHub: `## Primeiros passos` pode ser acessado como `#primeiros-passos`.

### Obsidian

- **Wiki links.** `[[Nota]]` aponta para `Nota.md` na mesma pasta ou em uma pasta abaixo. `[[Nota|Texto exibido]]` mostra o seu próprio texto, e `[[Nota#Título]]` abre a nota. ⌘-clique a abre no Splat.
- **Embeds.** `![[Nota]]` mostra a outra nota dentro desta; as notas embutidas nela aparecem como links. `![[diagram.png]]` mostra uma imagem. Como no Obsidian, os embeds nomeiam notas e imagens na pasta do documento ou abaixo dela, nunca por um caminho absoluto, e só arquivos Markdown aparecem como notas.
- **Comentários.** `%%Só no código-fonte%%` permanece visível no código-fonte e fica de fora da pré-visualização e de todas as exportações, tanto em linha quanto em linhas próprias.
- **Tags.** `#ideia` ou `#projeto/splat` é destacado. ⌘-clique procura a tag no documento e, na barra lateral, em toda a pasta.
- **Caixas de informação recolhíveis.** Veja [Caixas de informação](#caixas-de-informação).

### Citações e referências

- **Citações do Pandoc.** `[@doe2020]` ou `[@doe2020; @roe2021]` aparece como “(Doe 2020; Roe 2021)”. As entradas vêm do arquivo BibTeX que o front matter indica com `bibliography: refs.bib`. Sem isso, o Splat usa `references.bib`, `literatur.bib` ou `bibliography.bib` ao lado do documento.
- **Referências do GitHub e do GitLab.** Se o documento estiver em um repositório Git cujo remote `origin` está no github.com ou em um host GitLab, `@nome`, `#12` (issue) e `!7` (merge request, só no GitLab) são destacados. ⌘-clique os abre no navegador.

### HTML embutido

O HTML que se encontra nos README do GitHub é renderizado, com um leitor próprio em vez de um motor de navegador:

- parágrafos e blocos centralizados (`<p align="center">`, `<div align="center">`)
- imagens com tamanho (`<img src="logo.png" width="120">`)
- links, `<kbd>⌘</kbd>`, `<br>`, `<u>`, `<ins>`, `<mark>`, `<sup>`, `<sub>`
- listas e tabelas, inclusive células que abrangem colunas (`colspan`)
- seções recolhíveis: `<details><summary>Mais</summary>…</details>`, abertas com um clique em ▸

O Markdown entre tags HTML é renderizado como Markdown. Scripts, estilos, frames e formulários são omitidos por completo, e links e imagens mantêm apenas endereços `http`, `https`, `mailto` e relativos.

### Pontuação tipográfica

**Visualizar › Pontuação tipográfica** mostra aspas retas como as aspas do idioma da sua interface, `--` como meia-risca, `---` como travessão e `...` como reticências. Em alemão, `"Ja"` vira „Ja“, em francês «Oui», em japonês 「はい」, também onde chinês e japonês compõem aspas sem espaço; abreviações hebraicas como `צה"ל` recebem seus gershayim, צה״ל. O código-fonte mantém os caracteres simples; a pré-visualização e todas as exportações mostram os tipográficos. A opção vem desligada até você ligá-la.

### Resumo da sintaxe

| Elemento | Markdown |
|---|---|
| **Negrito** | `**Negrito**` |
| *Itálico* | `*Itálico*` |
| Sublinhado | `<ins>Sublinhado</ins>` ou `<u>Sublinhado</u>` |
| ~~Tachado~~ | `~~Tachado~~` |
| Destaque | `<mark>Destaque</mark>` ou `==Destaque==` |
| `Código` | `` `Código` `` |
| x² | `x<sup>2</sup>` ou `x^2^` |
| H₂O | `H<sub>2</sub>O` ou `H~2~O` |
| [Link](https://example.com) | `[Link](https://example.com)` |
| Título | `# Título` a `###### Título` |
| Citação | `> Citação` |
| Lista com marcadores | `- Item` |
| Lista numerada | `1. Item` |
| Tarefa | `- [ ] Pendente` e `- [x] Concluída` |
| Bloco de código | ```` ```rust ```` … ```` ``` ```` |
| Tabela | `\| A \| B \|` com `\|---\|---\|` abaixo do cabeçalho |
| Nota de rodapé | `Texto[^1]` e `[^1]: Nota` |
| Separador | `---` |
| Imagem | `![Texto alternativo](picture.png)` |
| Fórmula | `$E = mc^2$`, `$$ … $$` |
| Diagrama | ```` ```mermaid ````, ```` ```dot ````, ```` ```plantuml ```` |
| Caixa de informação | `> [!TIP]` |
| Wiki link | `[[Nota]]`, `[[Nota\|Texto]]` |
| Embed | `![[Nota]]`, `![[image.png]]` |
| Comentário | `%%Comentário%%` |
| Tag | `#tag` |
| Citação bibliográfica | `[@chave]` |
| Spoiler | `\|\|Spoiler\|\|` |
| Emoji | `:tada:` |

## Fórmulas

As fórmulas são renderizadas pelo [MathJax](https://www.mathjax.org) com seu TeX e matemática AMS, como gráficos vetoriais nítidos. O texto em `\text{…}` pode estar em qualquer escrita: o que faltar nas fontes do MathJax vem das fontes do seu Mac, com árabe e hebraico correndo da direita para a esquerda, e emoji em cores.

| Tipo | Sintaxe |
|---|---|
| Em linha | `$E = mc^2$`, `` $`E = mc^2`$ `` (GitHub, GitLab), `\(E = mc^2\)` |
| Em bloco | `$$` … `$$`, `\[` … `\]` ou um bloco de código marcado como `math` |

**Editar uma fórmula.** Clique em uma fórmula na pré-visualização, dê um duplo clique nela, pressione Return sobre ela ou leve o cursor até ela para abrir o editor em um popover. **Inserir › Fórmula** insere uma nova e abre o editor na hora.

- **Visual ou LaTeX.** A chave no topo alterna entre um editor visual ([MathLive](https://cortexjs.io/mathlive/)) e o código LaTeX. O Splat lembra da sua escolha.
- **Blocos de construção** para fração, raiz quadrada, sobrescrito e subscrito, somatório e integral com limites, limite, parênteses que crescem com o conteúdo, matriz 2×2, letras gregas, e relações, operadores e setas. Eles deixam marcadores □, e **Tab** salta para o próximo.
- **Pré-visualização ao vivo.** As alterações aparecem no documento enquanto você digita. Os erros do MathJax aparecem em uma linha abaixo do código.
- **Confirmar ou descartar.** Clicar fora mantém a alteração como um único passo de desfazer; **Esc** ou **Cancelar** a descarta. Pressionar Esc em uma fórmula nova e vazia a remove de novo.
- O menu de contexto oferece **Editar Fórmula…** para a fórmula sob o ponteiro.

O VoiceOver lê uma fórmula como o seu código LaTeX. O Splat renderiza fórmulas, não documentos LaTeX completos: não há pacotes, nem TikZ, nem `\documentclass`.

## Diagramas

Os diagramas são renderizados dentro do Splat, offline e em uma variante clara e uma escura.

| Motor | Bloco de código | O que desenha |
|---|---|---|
| [Mermaid](https://mermaid.js.org) | ```` ```mermaid ```` | fluxogramas, diagramas de sequência, de classes, de estados, entidade-relacionamento, Gantt, pizza, mapas mentais, linhas do tempo e todos os outros tipos do Mermaid |
| [Graphviz](https://graphviz.org) | ```` ```dot ```` ou ```` ```graphviz ```` | grafos na linguagem DOT |
| [PlantUML](https://plantuml.com) | ```` ```plantuml ```` ou ```` ```puml ```` | diagramas UML; roda sem Java |

```mermaid
flowchart LR
    A[Markdown] --> B[Pré-visualização]
    B --> C[PDF]
```

**Editar um diagrama** funciona como editar uma fórmula: clique nele para abrir o código em um popover, com cores de sintaxe e mensagens de erro em uma linha abaixo do código. **Inserir › Diagrama** parte de um modelo de fluxograma, de diagrama de sequência ou de gráfico de Gantt. Blocos de construção ajudam com os três tipos mais comuns do Mermaid:

- **Fluxograma:** Nó ▾ (retângulo, arredondado, círculo, losango, banco de dados), Conexão, Conexão com rótulo, Grupo, Direção ▾
- **Diagrama de sequência:** Participante, Mensagem, Resposta, Nota, Laço, Caminhos alternativos
- **Gráfico de Gantt:** Seção, Tarefa, Marco

Os outros tipos de diagrama são editados como código. O VoiceOver lê o título ou o texto alternativo de um diagrama.

O primeiro diagrama depois da abertura leva cerca de um segundo, porque o motor de renderização inicia em segundo plano. Um diagrama que leva mais de 10 segundos mostra um erro, e o motor de renderização reinicia do zero para o próximo. O motor do PlantUML (3 MB) só é carregado com o primeiro diagrama PlantUML. Um `!include` do PlantUML vindo da web é bloqueado.

## Imagens

- **Imagens locais** usam caminhos relativos ao documento: `![Logo](images/logo.png)`. O Splat exibe PNG, JPEG, GIF, WebP, BMP, SVG, HEIC, TIFF, AVIF e PDF.
- **Imagens da web** ficam bloqueadas até você permitir: uma barra acima da pré-visualização explica que carregá-las informa aos servidores delas que e quando você abre o documento. **Carregar Imagens** as permite para este documento; o Splat lembra da escolha por arquivo.
- **Tamanho.** `<img width="200">` define o tamanho de uma imagem; caso contrário, as imagens se ajustam à largura da pré-visualização.
- **Alterações no disco.** Uma imagem que você substitui, ou uma que aparece depois que o documento a citou, é exibida sem abrir o documento de novo. Imagens da web que falharam por causa da rede são tentadas de novo enquanto um documento as exibe.
- **Memória.** As imagens são exibidas com até 2560 pixels no lado mais longo, o que também preenche uma página A4. Elas são decodificadas menores quando ocupam um oitavo da memória do Mac e enquanto o macOS informa pressão de memória.
- **Modo escuro.** Imagens com fundo branco ficam levemente escurecidas na pré-visualização escura.
- **Colar e soltar.** Veja [No código-fonte](#no-código-fonte).

## Links e navegação

Na pré-visualização, um clique posiciona o cursor para você editar o texto do link; **⌘-clique** segue o link:

- `http`, `https` e `mailto` abrem no seu navegador ou app de e-mail.
- Arquivos Markdown abrem no Splat; outros arquivos e pastas são mostrados no Finder.
- `#anchor` salta para o título no documento.
- Endereços que abrem outro app, como `slack://` ou `vscode://`, perguntam antes e mostram o endereço completo e o app. **Cancelar** vem pré-selecionado.

**Inserir › Índice** insere uma lista com links para todos os títulos. Com o cursor dentro dela, o mesmo comando a atualiza. A barra lateral de estrutura (⌃⌘S) salta para qualquer título nos dois painéis.

## Notas em uma pasta

Os botões no topo da barra lateral alternam entre a estrutura, os arquivos e uma busca.

- **Arquivos** (**Visualizar › Mostrar Arquivos**, ⌃⌘E). Todos os arquivos Markdown da pasta do documento ou, se ele estiver em um repositório Git, de todo o repositório. Um clique ou Return abre uma nota como aba desta janela; se já estiver aberta, ela vem para a frente. O botão ao lado do nome da pasta mostra outra pasta nesta janela. Arquivos ocultos, `node_modules`, `target`, `Pods`, `DerivedData`, `__pycache__` e pastas sem notas ficam de fora. Um documento diretamente na sua pasta pessoal mostra apenas as notas ao lado dele, para que o Splat não percorra todas as suas pastas pedindo permissão para cada uma.
- **Busca** (**Editar › Buscar › Buscar em Todas as Notas…**, ⇧⌘F). Encontra texto em todas as notas, não importa se em maiúsculas ou minúsculas nem como seus acentos estão codificados. `#tag` encontra uma tag, `#project` também `#project/splat`, e nada dentro de código. Um clique em um resultado abre a nota naquele ponto.
- **Links para uma nota.** O menu de contexto de uma nota lista as notas que apontam para ela, por wiki link, embed, link relativo ou definição de referência de link.
- **Renomear uma nota.** O mesmo menu a renomeia e altera esses links. O Splat diz primeiro de quantos links em quantos arquivos se trata. Em cada link muda apenas o nome, e todos os outros bytes do arquivo continuam iguais. Documentos abertos recebem a alteração como um passo de desfazer; em outros arquivos, só o Git ou as Versões podem revertê-la. Um arquivo que outro programa alterou nesse meio-tempo interrompe a renomeação de todos os links, e o Splat o indica.

A lista acompanha a pasta quando a janela vem para a frente. A busca vê os arquivos como estão no disco, e o texto que você ainda não salvou conta a partir do próximo Salvamento Automático.

## Alterações desde o último commit

Em um documento em um repositório Git, barras ao lado do texto marcam o que difere do último commit, no código-fonte e na pré-visualização: verde para linhas adicionadas, azul para linhas alteradas e uma marca vermelha curta onde algo foi removido. Elas acompanham enquanto você digita e quando o branch se move. **Visualizar › Alterações desde o Último Commit** as desativa, em todas as janelas.

Linhas que diferem apenas no final de linha não contam, então um arquivo com CRLF diante de um commit com LF não mostra alteração. Um arquivo novo não tem marcas até o primeiro commit, nem um documento acima de 32 MB.

## Exportar, imprimir e compartilhar

| Formato | Como | O que você obtém |
|---|---|---|
| **PDF** | **Arquivo › Exportar como PDF…** ou o menu Compartilhar | Páginas no tamanho do papel, sempre na aparência clara, com fórmulas e diagramas como gráficos vetoriais. **Arquivo › Configurar Página…** define o papel. |
| **Impressão** | **Arquivo › Imprimir…** (⌘P) | O mesmo layout do PDF. |
| **HTML** | **Arquivo › Exportar como HTML…** ou o menu Compartilhar | Um único arquivo autossuficiente: imagens locais embutidas, fórmulas e diagramas como imagens nítidas, âncoras de título para o índice funcionar. Sem scripts. |
| **Word** | **Arquivo › Exportar como Documento do Word…** ou o menu Compartilhar | Um `.docx` com os estilos de título do Word, listas e tabelas de verdade (inclusive células mescladas), notas de rodapé como notas de rodapé do Word, imagens embutidas, fórmulas e diagramas. |
| **Markdown** | O botão **Compartilhar** na barra de ferramentas | O documento como arquivo `.md` para o Mail, o Mensagens, o AirDrop e qualquer outro serviço de compartilhamento. |

O menu **Arquivo › Compartilhar** oferece tudo isso em um só lugar. As exportações usam a opção de pontuação tipográfica e as imagens da web que você permitiu; no HTML, as imagens da web continuam sendo links.

**Arquivo › Opções de Exportação…** define o que PDF, impressão e Word adicionam ao texto, para todos os documentos:

- **Números de página** no rodapé do PDF e da impressão, no formato `2 / 9`.
- **Cabeçalho e rodapé** para PDF e impressão. `{title}`, `{author}`, `{date}`, `{page}` e `{pages}` representam seus valores, os três primeiros vindos dos metadados do documento.
- **Uma página de rosto** para PDF, impressão e Word, com `title`, `subtitle`, `author` e `date` dos metadados, ou o nome do documento como título. Ela não tem cabeçalho nem rodapé.
- **Um modelo do Word**, um `.docx` ou `.dotx` cuja aparência a exportação assume: seus estilos, inclusive com nomes localizados, seu tema, tamanho de página, margens, cabeçalhos e rodapés. Um modelo que não pode ser lido interrompe a exportação e avisa, em vez de entregar a você um documento sem ele.

## Importar documentos do Word

**Arquivo › Importar Documento do Word…** pede um arquivo `.docx` ou `.dotx` e onde colocar seu Markdown, e depois abre o resultado. As imagens vão para uma pasta ao lado, com o nome do documento seguido de `-images`.

Títulos e listas chegam como os estilos e a numeração do Word os definem, inclusive em documentos cujos estilos têm nomes localizados. Negrito, itálico, sublinhado, tachado, destaque, sobrescrito e subscrito, links, citações, código, notas de rodapé, notas de fim e tabelas vêm junto, com células que ocupam várias colunas como tabela HTML. Fórmulas viram LaTeX. Alterações controladas contam como aceitas, e um índice fica de fora, já que **Inserir › Índice** cria um novo. Arquivos `.doc` antigos e documentos protegidos por senha são recusados.

## Aparência e acessibilidade

- **O modo escuro** segue o sistema. Fórmulas e diagramas são renderizados para cada aparência.
- **Aumentar contraste** nos Ajustes troca as cores de sintaxe por paletas de alto contraste.
- **Zoom.** ⌘+, ⌘− e ⌘0 escalam os dois painéis.
- **Tela cheia, abas e uma barra de ferramentas personalizável**, como em todo app do Mac.
- **VoiceOver.** Os painéis se chamam “Código-fonte Markdown” e “Pré-Visualização”, as fórmulas são lidas como o código delas e os diagramas como o título. Todo comando é acessível pelo teclado.

O Splat não tem janela de ajustes: as poucas opções ficam onde se aplicam, em **Visualizar**, **Editar › Substituições** e **Arquivo › Opções de Exportação…**.

## Idiomas

A interface, os menus e a referência rápida de Markdown estão traduzidos para 35 idiomas:

alemão, árabe, catalão, checo, chinês (simplificado), chinês (tradicional), coreano, croata, dinamarquês, eslovaco, esloveno, espanhol, finlandês, francês, grego, hebraico, hindi, holandês, húngaro, indonésio, inglês, italiano, japonês, malaio, norueguês, polonês, português (Brasil), português (Portugal), romeno, russo, sueco, tailandês, turco, ucraniano e vietnamita.

O Splat segue a ordem de idiomas em **Ajustes › Geral › Idioma e Região**; ali você também pode escolher um idioma só para o Splat em **Aplicativos**. Sem tradução para o seu idioma, ele usa inglês. Os termos de menu seguem as traduções da própria Apple, de modo que o Splat se lê como o resto do seu Mac.

O texto em hebraico e árabe é lido da direita para a esquerda onde quer que apareça, qualquer que seja o idioma da interface: cada parágrafo, título e item de lista assume a direção da sua primeira letra, citações e caixas de informação desenham a barra à direita, e uma tabela cujo texto começa da direita para a esquerda lista suas colunas a partir da direita. Os blocos de código continuam da esquerda para a direita. As exportações em PDF, HTML e Word mantêm essas direções. Com hebraico ou árabe como idioma do sistema, o macOS também espelha a janela, de modo que a pré-visualização fica à esquerda.

## Atalhos de teclado

| Ação | Atalho |
|---|---|
| Negrito, Itálico, Sublinhado | ⌘B, ⌘I, ⌘U |
| Tachado | ⇧⌘X |
| Código | ⌥⇧⌘C |
| Link… | ⌘K |
| Corpo de texto, Título 1 a 6 | ⌥⌘0, ⌥⌘1 a ⌥⌘6 |
| Alinhar a coluna da tabela à esquerda, ao centro, à direita | ⌘{, ⌘\|, ⌘} |
| Ocultar ou mostrar a pré-visualização | ⌥⌘P |
| Mostrar ou ocultar a estrutura | ⌃⌘S |
| Mostrar ou ocultar os arquivos | ⌃⌘E |
| Buscar em todas as notas | ⇧⌘F |
| Colar como Markdown | ⌃⌘V |
| Ampliar, reduzir, tamanho real | ⌘+, ⌘−, ⌘0 |
| Novo documento, abrir, salvar, duplicar | ⌘N, ⌘O, ⌘S, ⇧⌘S |
| Configurar página, imprimir | ⇧⌘P, ⌘P |
| Buscar, buscar e substituir | ⌘F, ⌥⌘F |
| Buscar seguinte, anterior; usar seleção; saltar para a seleção | ⌘G, ⇧⌘G; ⌘E; ⌘J |
| Mostrar ortografia e gramática; verificar documento | ⌘:, ⌘; |
| Colar com o mesmo estilo | ⌥⇧⌘V |
| Ocultar a barra de ferramentas, mostrar todas as abas, tela cheia | ⌥⌘T, ⇧⌘\\, ⌃⌘F |
| Quebra de linha (pré-visualização) | ⌥Return ou ⌃Return |
| Próxima célula / célula anterior em uma tabela (pré-visualização) | Tab / ⇧Tab |
| Aumentar / diminuir o recuo de um item de lista (pré-visualização) | Tab / ⇧Tab |
| Próximo marcador no editor de fórmulas | Tab |
| Fechar o editor de fórmulas ou de diagramas sem alterações | Esc |
| Seguir um link, uma tag ou uma referência (pré-visualização) | ⌘-clique |

O Splat deixa os atalhos padrão da Apple onde estão.

## Privacidade e segurança

Um arquivo Markdown pode vir de qualquer pessoa, por isso o Splat trata todo documento como não confiável.

- **Sem rede por padrão.** O Splat não tem telemetria. Ele só busca atualizações depois que você concorda ou escolhe **Buscar Atualizações…**: nesse caso, carrega um pequeno arquivo de feed do GitHub, e a requisição informa apenas o app e a versão dele. Fórmulas e diagramas são renderizados localmente; as web views que fazem isso não alcançam a rede. **Ajuda › Relatar um Problema…** abre um formulário do GitHub no navegador; o endereço leva a versão do Splat, a do macOS e o tipo de Mac, nada dos seus documentos.
- **As imagens da web** só carregam com a sua permissão, só por `https`, nunca de hosts da sua rede local, com no máximo 10 MB cada e em uma sessão privada, sem cookies.
- **Decodificação isolada.** Imagens e os gráficos de fórmulas e diagramas são decodificados em um serviço próprio, que o macOS inicia em uma sandbox própria: nenhum dos seus arquivos, sem rede. Ele devolve apenas pixels, cujo tamanho é verificado, e se encerra sozinho quando uma decodificação leva mais de 10 segundos ou usa memória demais. Os decodificadores de imagem da própria Apple só veem HEIC, TIFF, AVIF e PDF, e apenas quando os dados realmente têm esse formato. SVGs, fórmulas e diagramas também passam por esse serviço, então no Splat são imagens com resolução fixa: quatro pixels por ponto, até 2560 no lado mais longo.
- **Arquivos locais** chegam a um documento apenas pelo caminho que ele cita, e apenas de uma pasta à qual você concedeu acesso: o Splat pode ler um documento que você abre, mas as imagens, notas, bibliografia e repositório ao redor dele só depois que você concede acesso à pasta em que estão com **Conceder Acesso à Pasta…** na barra acima da pré-visualização. Um acesso vale para todas as subpastas e fica lembrado. Os embeds mostram notas Markdown da pasta do documento e abaixo, e uma imagem ausente só é procurada ali. Um caminho de imagem em outro lugar, absoluto ou começando com `~`, carrega na pré-visualização a partir de uma pasta com acesso concedido; as exportações HTML e Word, a impressão e os PDFs levam apenas o que está na pasta do documento ou no repositório Git dele, a menos que esse repositório também contenha a sua pasta pessoal, para que um documento de outra pessoa não puxe seus arquivos para uma exportação que você repassa.
- **Pastas de notas** são lidas apenas enquanto a barra lateral as mostra, e renomear uma nota altera outros arquivos só depois que você confirma quantos.
- **Repositórios Git** são lidos pelo próprio Splat. Ele nunca executa o programa `git`, cuja configuração em um repositório de outra pessoa pode iniciar outros programas, e não lê mais do que a versão do documento aberto no último commit. Ele não grava nada ali e não acessa nenhuma rede.
- **Documentos do Word e texto colado** são lidos pelos parsers próprios do Splat, com limites de tamanho, de aninhamento e de quanto um arquivo pode se descompactar. Os links neles mantêm apenas os endereços que o Splat permite em outros lugares, e os nomes de imagens em um arquivo do Word nunca decidem onde elas são gravadas.
- **Cópias de texto não salvo** ficam neste Mac, na sua pasta Biblioteca, para a recuperação após uma falha. Elas saem com o salvamento ou com o encerramento normal do Splat; as cópias que uma falha deixou ficam na pasta de recuperação até você apagá-las.
- **Links** nunca iniciam um programa com um clique. Links da web e de e-mail abrem como de costume; links para outros apps perguntam antes.
- **O HTML nos documentos** é lido pelo parser próprio do Splat. Scripts, estilos, frames e formulários nunca são executados.
- **As exportações** não levam scripts. As páginas HTML mantêm apenas uma lista fixa de tags e atributos inofensivos, como o GitHub faz, além de uma política de segurança de conteúdo. Os arquivos Word só vinculam endereços `http`, `https` e `mailto`, exceto os links que vêm com um modelo do Word que você escolheu.
- **Assinatura de código** com o Hardened Runtime e o App Sandbox. O Splat pode ler e gravar os documentos que você abre e as pastas às quais você concede acesso, acessar a rede para as imagens da web que você permitiu e para atualizações, e imprimir; nada mais.

## Documentos grandes

O Splat continua responsivo com documentos de vários megabytes:

- O código-fonte usa o TextKit 2, que faz o layout apenas da parte visível.
- Enquanto você digita na pré-visualização, e no código-fonte de documentos de até 2 MB, só os parágrafos afetados são analisados, coloridos e diagramados de novo. Em um documento de 10 MB, analisar e colorir leva cerca de 3 ms, renderizar cerca de 10 ms, e uma tecla inteira na pré-visualização dele cerca de 30 ms, com o trabalho da própria view.
- Pré-visualizações grandes renderizam primeiro a parte visível e o resto enquanto o app está ocioso. Os blocos que entram na área visível pela rolagem são renderizados antes de serem desenhados.
- No código-fonte de documentos maiores, a análise para a pré-visualização roda em segundo plano enquanto você digita.
- Um documento continua sendo texto simples quando precisaria de cerca de 4 GB de memória, ou seja, cerca de 50 a 120 MB de Markdown comum, mas apenas de 10 a 20 MB se tiver só tabelas ou títulos: o código-fonte fica sem cores e a pré-visualização diz por que continua vazia. As exportações ainda funcionam.
- A partir de cerca de 200 KB, um documento abre a janela antes de ser analisado; com 5 MB, as cores e a pré-visualização vêm cerca de um quarto de segundo depois.
- Todas as imagens juntas, incluindo fórmulas e diagramas, ocupam no máximo um quarto da memória do Mac, e metade disso enquanto a memória está escassa. Além disso, as demais imagens aparecem como marcadores de posição até que haja espaço de novo.

Algumas alterações são processadas por inteiro, com cerca de 300 ms por tecla em 10 MB: as que afetam a definição ou as referências de uma nota de rodapé, o que fica junto a uma nota de rodapé em linha, o fim do documento junto a uma nota de rodapé, o front matter ou definições de referência de link, e toda alteração em um documento com blocos de comentário `%%`, com uma nota de rodapé definida duas vezes ou dentro de uma citação ou lista, ou com mais de 1000 notas de rodapé em linha ou marcas de fórmula não fechadas. A memória cresce com o arquivo, para aproximadamente 115–160 MB com 1 MB de Markdown e 340–510 MB com 10 MB.

## Limitações conhecidas

- **Um único til significa subscrito.** `~texto~` é subscrito, como no Pandoc; o tachado exige `~~dois~~`. O GitHub também tacha com um único til.
- **Somente leitura na pré-visualização:** HTML embutido e notas embutidas são editados no código-fonte, e o front matter em seu formulário ou no código-fonte.
- **Aninhamento muito profundo:** HTML aninhado com mais de 64 níveis, e HTML em linha com mais de 16, cede lugar ao seu conteúdo.
- **Células de tabela mescladas** existem apenas em tabelas HTML, e só entre colunas (`colspan`, sem `rowspan`). A sintaxe de tabela do MultiMarkdown para células mescladas não é suportada.
- **Bibliografias** são lidas apenas de BibTeX, não de CSL JSON nem de YAML.
- **Referências** precisam de um repositório no github.com ou em um host GitLab; `!7` existe apenas no GitLab.
- **Pontuação tipográfica:** uma aspa simples de fechamento alemã não se distingue de um apóstrofo e é composta como ’.
- **Fórmulas, não documentos:** sem pacotes LaTeX, TikZ nem documentos completos.
- **O editor visual de fórmulas** escreve LaTeX na notação do MathLive: os espaços desaparecem (`E = mc^2` vira `E=mc^2`), e a renderização continua a mesma.
- **Contagem de palavras:** tailandês, laosiano, khmer e birmanês são escritos sem espaços e são contados por trecho de texto, não por palavra. `3.14` e `snake_case` são duas palavras cada.
- **Renomear uma nota** não altera links em HTML embutido, e um wiki link fica como está se o novo nome contiver `#`, `|`, `^`, `[`, `]` ou `\`.
- **As marcas de alteração** leem repositórios com objetos SHA-1 e refs simples ou compactadas; um com SHA-256 ou reftable não mostra nenhuma. Um arquivo sob um filtro do Git como o LFS é comparado com o que o filtro armazenou.
- **A importação do Word** não traz o texto em cabeçalhos, rodapés e comentários, e imagens em `.emz` ou SVG mantêm apenas o texto alternativo.
- **Colar como Markdown** lê RTF como texto ocidental e, nele, só obtém títulos a partir de tamanhos de fonte. A maioria dos programas também oferece HTML, que tem preferência.
- **Por escolha de projeto:** sem números de linha, sem janela de ajustes, sem Help Book, fora da App Store.
- **Menos testado no Intel.** Os autotestes de janela rodam apenas em Apple Silicon; no Intel, o CI executa `cargo test`.

## Solução de problemas

**A lista de documentos recentes está vazia depois que recompilei o Splat.** O macOS associa os documentos recentes e as permissões de pastas à assinatura de código do app. As compilações locais são assinadas com o certificado “Splat Local Signing” no seu chaveiro de login, então as recompilações mantêm os dois. Se o certificado foi apagado, a próxima compilação cria um novo e as listas recomeçam uma vez.

**O app da imagem de disco mostra documentos recentes diferentes da minha própria compilação.** Os dois têm assinaturas diferentes, então o macOS mantém uma lista separada e permissões de pastas separadas para cada um.

**HTML ou outra sintaxe aparece como código simples.** Pode ter sido uma cópia desatualizada do Splat que abriu o documento. Encerre-a, escolha **Arquivo › Obter Informações** no documento, no Finder, e confira se **Abrir com** indica o Splat da sua pasta Aplicativos.

**Não há Buscar Atualizações… no menu Splat.** Apenas a versão da imagem de disco ou do Homebrew se atualiza sozinha; uma compilação própria não tem atualizador.

**As imagens da web não aparecem.** Clique em **Carregar Imagens** na barra acima da pré-visualização. Só carregam imagens `https` de hosts públicos.

**Ao colar uma captura de tela, o Splat pede para salvar antes.** As imagens coladas são guardadas como arquivos ao lado do documento, o que exige um lugar no disco. Salve o documento e cole de novo.

**“O arquivo não é um texto UTF-8.”** O arquivo está em outra codificação e não tem uma marca de ordem de bytes que a indique. Converta-o antes, por exemplo com `iconv -f ISO-8859-1 -t UTF-8 old.md > new.md`.

**Uma fórmula ou um diagrama mostra um erro.** Clique nele: o editor mostra a mensagem do MathJax, do Mermaid, do Graphviz ou do PlantUML abaixo do código.

## Para desenvolvedores

A compilação, os testes, a estrutura do código e as regras que ele segue estão descritos em [docs/development.md](../../../docs/development.md) (em inglês).

### Componentes de terceiros

| Componente | Versão | Usado para | Licença |
|---|---|---|---|
| [comrak](https://github.com/kivikakk/comrak) | 0.59 com [uma linha alterada](../../../vendor/comrak/PATCH.md) | parser de Markdown | BSD-2-Clause |
| [objc2](https://github.com/madsmtm/objc2), objc2-foundation, block2 | 0.6 / 0.3 | runtime do Objective-C, Foundation, blocos | MIT |
| objc2-app-kit, objc2-web-kit, dispatch2 | 0.3 | bindings de AppKit, WebKit e Grand Central Dispatch | Zlib, Apache-2.0 ou MIT |
| [krilla](https://github.com/LaurenzV/krilla), krilla-svg, [usvg](https://github.com/linebender/resvg) | 0.8 / 0.47 | fórmulas e imagens SVG como PDF vetorial | MIT ou Apache-2.0 |
| [image](https://github.com/image-rs/image) | 0.25 | decodificação de PNG, JPEG, GIF, WebP e BMP | MIT ou Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2 | alinhamento das colunas de tabelas | MIT ou Apache-2.0 |
| [entities](https://github.com/p-jackson/entities) | 1 | os nomes das referências de caracteres HTML | MIT |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide) | 0.9 | descompactar arquivos do Word e objetos do Git | MIT, Zlib ou Apache-2.0 |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.21 | ler o XML de arquivos do Word | MIT ou Apache-2.0 |
| [encoding_rs](https://github.com/hsivonen/encoding_rs) | 0.8 | leitura de texto RTF escrito em páginas de código antigas | MIT ou Apache-2.0, BSD-3-Clause |
| [unicode-normalization](https://github.com/unicode-rs/unicode-normalization) | 0.1 | nomes e busca entre letras compostas e decompostas | MIT ou Apache-2.0, Unicode-3.0 |
| [emojis](https://github.com/rossmacarthur/emojis) | 0.9 | manter literal o texto parecido com `:shortcode:` em texto importado | MIT ou Apache-2.0, Unicode-3.0 |
| [MathJax](https://www.mathjax.org) | 4.1.3 | renderização de fórmulas | Apache-2.0 |
| [MathLive](https://cortexjs.io/mathlive/) | 0.111.4 | editor visual de fórmulas, com fontes KaTeX | MIT |
| [Mermaid](https://mermaid.js.org) | 12.3.0 | diagramas Mermaid | MIT |
| [Viz.js](https://github.com/mdaines/viz-js) | 3.24.0 | Graphviz compilado para WebAssembly, contém Graphviz (EPL-1.0) e Expat (MIT) | MIT |
| [PlantUML](https://github.com/plantuml/plantuml) (`@plantuml/core`) | 1.2026.9 | diagramas PlantUML, compilados para JavaScript | MIT |
| [Sparkle](https://sparkle-project.org) | 2.10.1 | atualizações da versão da imagem de disco | MIT, com partes sob licenças BSD e zlib |

As bibliotecas JavaScript ficam em `resources/js/` exatamente como foram publicadas; o [vendor.lock](../../../vendor.lock) registra suas fontes e checksums SHA-256. Os textos de licença delas, inclusive os do Graphviz e do Expat dentro do Viz.js, ficam ao lado delas como `resources/js/LICENSE-*` e vão dentro do app. O Sparkle não fica no repositório: o `cargo xtask dmg` baixa o release dele e o confere com o checksum no xtask, e a licença dele vai dentro do app como `LICENSE-sparkle`.

## Licença

O Splat é licenciado sob qualquer uma das duas

- Licença Apache, Versão 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE))
- Licença MIT ([LICENSE-MIT](../../../LICENSE-MIT))

à sua escolha.

Salvo declaração explícita em contrário, qualquer contribuição enviada intencionalmente por você para inclusão no Splat, conforme definida na licença Apache-2.0, terá licença dupla como acima, sem termos ou condições adicionais.
