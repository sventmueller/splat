# Glossary

The words the code uses for its own things, with the usual term where one exists. Names in `code` are types or
functions; the crate is in brackets where it is not obvious.

## Positions and text

| In the code | Meaning |
|---|---|
| `Byte` | A byte offset into the Markdown source; the model counts in these. |
| `U16` | An offset in UTF-16 code units into a source view, as AppKit counts. |
| `Shown` (`splat-preview`) | An offset in UTF-16 code units into the preview's text, which has nothing in common with the source but what `preview_offset` and `insert_point` map. |
| `TextAt::at` / `from` / `upto` | Slicing a source text by `Byte`: a range, from an offset to the end, from the start up to an offset. |
| `fit`, `fit_at` | Bringing a selection or offset from outside, which may belong to an older text, into the text and onto the start of a character; every command and query does this first. |
| `line_marks`, `LineMark` | The marks that open containers at the start of a source line: quote marks, list markers and a task's checkbox, read the one way CommonMark reads them. |
| `TextPart` (`splat-markdown`) | A slice of the source together with the offset it starts at, so that offsets stay those of the whole text. |
| `Utf16Map` | A lookup between byte offsets and UTF-16 offsets of one text. |
| `Snapshot` (`splat-editor-state`) | The app's copy of the source text, patched in place on every change of the source view. |

## Parsing

| In the code | Meaning |
|---|---|
| `Doc`, `Block`, `Inline` | The document tree, Splat's own; comrak is named only in `reader/`. |
| partial parse, `reparse_or_why_not` | Parsing only the paragraphs an edit touched and keeping the rest of the tree (incremental parsing); see [partial-parse.md](partial-parse.md). |
| `Splice` | What a partial parse replaced: block indexes before and after, and the shift of everything behind. |
| `WholeParse` | Why an edit needs a full parse instead. |
| `TextFacts` | What is known about all of a text (link definitions, footnotes, `SyntaxCounts`), carried along with an edit instead of scanning the text again. |
| `SyntaxCounts` | How often a text holds `^[`, `\(`, `%%`, `]:` and the like, whose meaning depends on text far away, and its estimated memory. |
| stand-in definition | A dummy footnote definition parsed along with a part, as comrak sees a reference only with its definition. |
| `Running`, `Within`, `Segment` | Running text as the preview and the exports show it: typographic punctuation, and the tags, references and citations in it. |
| `Shorthand`, `Typesetting` | A tag, a reference into a code host or a citation in running text; what decides for a whole document how its punctuation and shorthands show. |
| `Aligner`, `Spelling` | Where each character of what the parser read stands in the source, past escapes, character references and the prefixes of quotes; `Spelling` says which of these the source may use. |
| `Role`, `Destination` (`splat-markdown`) | What an element of embedded HTML is to the preview, the exports and the word count, read once from its tag name; where an address in a link or a picture leads, which decides what each of them keeps of it. |

## The preview

| In the code | Meaning |
|---|---|
| `Layout`, `TopBlock`, `Para`, `Run` | The preview as values: one entry per top-level block, its paragraphs, and stretches of one style. |
| `Atom` | A run that stands for a whole range of the source (a picture, formula, checkbox), not character by character. |
| `CharMap` | Where each character of a run comes from in the source. |
| `ItemMarker` | The bullet, number, checkbox or footnote label in front of the first paragraph of a list item or footnote. |
| `Index` | Where each paragraph of a layout starts in the preview text. |
| `Bridge` | The translation of an edit in the preview into an edit of the source. |
| `verify` | Re-parsing what an edit in the preview produced, to check it shows what was typed. |
| `Resolver` | What the layout asks the outside world: whether a formula or diagram is drawn yet, quotation marks, citations. |
| `Drawn`, `Live` (app) | What rendering shows but does not make (pictures from the web, what WebKit drew); `Live` is the app's, a check passes its own. |

## The window

| In the code | Meaning |
|---|---|
| `EditorController` | The one object AppKit knows per window; it forwards to the components. |
| component, `ForPanes`, `ForPictures`, … | A part of the editor with the state it owns, reached through a handle that offers only what lies below it ([development.md](development.md#rules-in-the-controller)). |
| `State`, `Model` (`splat-editor-state`) | What the editor knows about the document; `Model` lends it to closures that cannot reach a view. |
| `Object` | The controller as AppKit knows it, as a component gets it: a target, a delegate, an object to schedule selectors on, none of the controller's own methods. |
| `Message`, `Messenger` | What reaches the controller for a component later or from one above it; a `Messenger` sends it from any thread, and it arrives on the main thread while the window is open. |
| `ViewReader` | A text view that can only be asked things; changes go through `Panes`. |
| `TextGeometry` | Where the text of a view lies, in whichever TextKit the view uses. |
| `Invalidate` | The reason for a redraw without a change of text. |
| `UnderWay`, `defers_background_results` | What goes on in the views that an update must not cut into, and whether a background result has to wait for it. |
| `Panes::silently` | Changes the controller makes to its own views, which the delegate callbacks ignore. |
| `contained`, `guarded` | Running code so that a panic costs that one result, not the app; `guarded` is for callbacks from the system. |
| `Lane`, `DISK` | A worker thread that runs jobs one after the other; `DISK` is the one for the user's files. |
| shadow copy, recovery | Unsaved text copied aside for the recovery after a crash. |
| decoder helper | A sandboxed process that decodes images and graphics, so a hostile file cannot reach the app. |
| `test_hooks` | What the self-test build does differently from the app that ships. |

## Files and imports

| In the code | Meaning |
|---|---|
| `FileState`, `Stamp` | Which text a file holds, apart from AppKit's change count; a file's date and size as last seen. |
| `Fingerprint` | The length and a hash of a text or a file, which tell texts apart without keeping a copy of them. |
| `SaveTicket` | A save that has begun; ending the save takes the ticket, so that each save ends once. |
| look, `look_ticket`, `LookTicket` | Checking a file on disk for a change by another program; the ticket tells whether a save touched the file since the look was asked for. |
| `Shared`, `OnMain` (app) | The part of a document that AppKit reaches on its own threads, and the part that only the main thread touches. |
| `Rich` (`splat-import`) | A foreign document (Word, RTF, HTML) in terms of what Markdown can say, before it is written as Markdown. |
| `RunContext`, `RunFormat`, `BlockBuilder` (`splat-import`) | While reading Word: what surrounds a run, how it looks, and the blocks built from paragraphs. |
| package (`splat-import`) | The Word file as a ZIP of XML parts with content types and relationships (Open Packaging Conventions). |
| OOXML (`splat-ooxml`) | Office Open XML, the format of Word files: a ZIP archive of XML parts. |
| `MOST_ENTRY_BYTES`, `MOST_TOTAL_BYTES` (`splat-ooxml`) | What one entry of a ZIP archive may inflate to, and what all entries read from one archive may inflate to together. A Word import goes without a picture beyond the first and ends beyond the second. |
| `WordError` (`splat-import`) | Why a Word file cannot be read, for an import or as one of the two files of a template merge. |
| `XmlBudget` (`splat-import`) | What all the XML parts of one import or merge may take together: bytes of text and parsed nodes. |
| `Placement` (`splat-import`) | Where pasted Markdown goes: into the line at the cursor or on lines of its own. |
| code page (`splat-import`) | What the bytes of RTF text stand for, from `\ansicpg` or from a font's `\fcharset` or `\cpg`. |
| `Tree`, `Node` (`splat-notes`) | The notes below a folder; only the scanner and `Tree::of_notes`, which builds the same tree without the disk, make one, so its indexes always name its nodes. |
| `folded_name` (`splat-git`) | A key for a file name: names a case-insensitive Mac volume takes for one file share it, as far as Latin letters go. |
| `LinkTarget` (`splat-notes`) | What a link in a note points to: a wiki page or an address. |
| `Oid` (`splat-git`) | A git object id, the SHA-1 of an object. |
| `MAX_TEXT_BYTES` (`splat-git`) | The longest text the app compares with the last commit; the largest object `splat-git` reads follows from it. |
