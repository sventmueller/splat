# Development

The words the code uses for its own things are explained in [glossary.md](glossary.md).

Splat is a Rust application that talks to AppKit and WebKit directly through
[objc2](https://github.com/madsmtm/objc2). There is no Swift, no Objective-C, no Xcode project and no Interface
Builder file.

## Building

Prerequisites:

- macOS with the Command Line Tools (`xcode-select --install`). Xcode is not needed; Splat builds without `actool`,
  `ibtool` and `metal`.
- Rust 1.99 (`brew install rust`). [rust-toolchain.toml](../rust-toolchain.toml) names the exact version that CI uses;
  rustup installs it on its own, Homebrew's Rust ignores the file. Raising it there is how new compiler and clippy
  warnings reach the project.
- rustup, for `make lint`, `make check` and `make universal`: `cargo xtask check-library` builds the library for Linux
  to show that it needs no macOS, and Homebrew's Rust has no other targets.

The [Makefile](../Makefile) holds short entry points; `make help` lists them all:

```sh
make build                  # target/release/Splat.app, same as cargo xtask bundle --release
make universal              # target/splat-universal for Apple Silicon and Intel, as the disk image carries it
make check                  # formatting, clippy without warnings, all tests, vendored libraries; CI runs it before coverage-library
make test                   # make check, then the self-test without a window
make test-gui               # self-tests with a window; they occupy the Mac for about four minutes
make test-long              # longer randomised search and the 10 MB partial-update check
make fuzz                   # random input for the partial parse and every output
make coverage               # line coverage of all tests and self-tests, with a floor per module
make coverage-library       # the same floors for the modules without a window, as in CI
make bench FILE=<file.md>   # timings for opening, typing and scrolling a large file
make release                # make test and coverage-library first, then the notarized disk image
```

The build logic itself lives in the Rust crate [xtask](../xtask/src/main.rs):

```sh
cargo xtask bundle --release   # target/release/Splat.app, Hardened Runtime, signed with "Splat Local Signing"
cargo xtask bundle             # target/debug/Splat Debug.app, a separate app that never opens Markdown files
cargo xtask dmg                # dist/Splat-<version>.dmg for distribution, Developer ID, notarized
cargo xtask icon               # rebuild assets/AppIcon.icns and the README icon from assets/icon-1024.png
cargo xtask vendor check       # verify the bundled JavaScript against vendor.lock
cargo xtask vendor fetch       # download it again after changing a version in vendor.lock
```

`bundle` writes the `Info.plist`, copies the resources and localizations, and signs the app with the certificate
"Splat Local Signing". The first build creates that certificate in your login keychain, valid for 20 years. As long
as it stays, Splat keeps its recent documents and folder permissions across rebuilds; with an ad hoc signature, every
build would be a new app to macOS. Debug and self-test builds carry bundle IDs of their own (`local.splat.debug`,
`local.splat.selftest`) and do not register for Markdown files, so the Finder never opens a document in an outdated
build and they leave the settings of the normal app alone.

`bundle` also writes `Acknowledgements.txt` into the resources, since the licenses of what Splat contains ask for their
texts to go along with the app; the About window links only Splat's own. The file holds Splat's license and the
license text of every crate that `cargo tree` finds in the program for either processor, of every
`resources/js/LICENSE-<name>` and of Sparkle where the bundle carries it, each text once under the components it
covers. Of alternatives a crate offers, MIT is taken where it can be; a crate that names MIT without shipping its text
gets Splat's MIT terms with the authors from its manifest. A dependency whose license text xtask does not recognize
stops the build with its name, and the test `every_component_of_the_program_comes_with_its_license_text` finds it in
`make check` before that. A new script in `resources/js` needs its `LICENSE-<name>` next to it.

The version lives in [VERSION](../VERSION) as `major.minor.build` and is the one the next build gets. Every release
build that succeeds (`make build`, `cargo xtask bundle --release`, `make release`) raises the build number by one;
debug and self-test builds leave it alone. Major and minor are raised by hand: set VERSION to, say, `1.1.0`, and the
next build gets exactly that. Since the version ends in the build number, the About window shows no second one.

## Distribution

`cargo xtask dmg` (or `make release`, which runs `make test` and `make coverage-library` first) builds a release, signs it with your Developer ID,
notarizes the app with Apple and staples the ticket, packs it into a compressed, read-only disk image with a link to
Applications, notarizes and staples the image as well and checks the result with Gatekeeper. The app carries its own
ticket so that a copy installed without the Finder, as Homebrew does, opens offline on first launch too; the two
rounds with the notary service take a few minutes each. Only an image that passed every step becomes
`dist/Splat-<version>.dmg`, the copy that is committed, and the build number goes up. The previous image stays for one more release, since GitHub's raw server may hand out the old update
feed for a few minutes after a push; older ones are moved to the trash. A failed run leaves `dist/` and VERSION as they
were.
Each release adds the full image, about 16 MB, to the repository history. It runs only when called, never as part of a normal build, and needs:

- a "Developer ID Application" certificate in the keychain
- a saved login for Apple's notary service with an app-specific password, created once with
  `xcrun notarytool store-credentials notary --apple-id <Apple ID> --team-id <team ID>`
- the private key that signs updates, in the login keychain. Sparkle's `generate_keys` created it; its public half is
  `UPDATE_KEY` in [xtask/src/main.rs](../xtask/src/main.rs) and inside every installed copy, which accepts only updates
  signed with it. Keep a backup in a password manager: `target/sparkle-2.10.1/bin/generate_keys -x <file>` exports
  it, `-f <file>` imports it on another Mac. Without it, installed copies can never be updated again.
- rustup (`brew install rustup`, which leaves Homebrew's Rust in place)
- `RELEASE_NOTES.md`, with what this release brings. The update dialog shows it above a line on how to report a problem,
  so `dmg` stops if the file is empty or still holds the previous release's notes

If any of them is missing, `dmg` stops before building. On the first run, macOS asks whether Terminal may control the
Finder; without that permission, the disk image shows neither its background nor the icon arrangement.

Only the disk image carries the updater, [Sparkle](https://sparkle-project.org), which the build downloads once into
`target/` from its GitHub release and checks against the checksum in xtask. The hardened runtime loads the framework
only when it carries the app's Team ID, which the Developer ID signature has and the local certificate lacks; and an
update must not replace a build of your own. The self-test build carries Sparkle as well, to check the update menu
against it: with a feed that does not exist, without Sparkle's question on the second launch, and with an entitlement
that lets the locally signed app load the framework. `dmg` signs the new image with the update key and writes
`dist/appcast.xml`, the feed installed copies read from GitHub. Commit and push the image and the feed
together: from then on, installed copies offer the update. The download links in this README and its translations point
at `Splat.dmg` in the latest GitHub Release, so each release carries a copy under that name:
`cp dist/Splat-<version>.dmg target/Splat.dmg`, then
`gh release create v<version> dist/Splat-<version>.dmg target/Splat.dmg`.

The disk image is a universal binary for Apple Silicon and Intel; other builds are for the host processor alone.
Homebrew's Rust ships the standard library for the host processor only, so `dmg` builds through rustup, which installs
the toolchain and targets of [rust-toolchain.toml](../rust-toolchain.toml) on first use, and stops if the program or
Sparkle lacks the code for either processor. `make universal` builds only the program.
The app in it carries a different signature from a local build, so macOS keeps separate recent documents and folder
permissions for the two.

## Testing

| Command | Checks |
|---|---|
| `cargo test --release` | unit, randomised and translation tests |
| `SPLAT_FUZZ_ROUNDS=20000 cargo test --release` | a longer randomised search; `SPLAT_FUZZ_SEED` varies the inputs |
| `SPLAT_I18N_ONLY=fr cargo test --release i18n` | one language, while others are incomplete |
| `SPLAT_WITHOUT_GIT=1 cargo test --release` | on a machine without `git`: the tests of `splat-git` that compare with it pass without comparing; without the variable they fail there |
| `cargo clippy --release --all-targets [--features selftest]` | lints, including the justification of every `unsafe` block, the length of functions and, in the library, number conversions |
| `cargo xtask check-library` | the library builds for Linux, so nothing in it needs macOS |
| `cargo xtask vendor check` | the JavaScript libraries match `vendor.lock` byte for byte |
| `cargo xtask selftest probe` | without a window, each with its expected result: formulas, diagrams, web images against a server inside the process, blocked addresses, the text every command writes, each image format through both exports, a page that stops answering, and the defences of the web views against a page without them. Takes about half a minute. What needs the app in front is skipped on a locked screen |
| `cargo xtask selftest gui [--stop-after "<check>"] [--scenarios] [--only "<group>"] [--in-back]` | the running app, click by click. Opens a window and types into it; the Mac is busy for about four minutes. `--scenarios` runs only the second part, in about a minute and a half. `--only` runs one group of that part, in its own document. On a locked screen it all runs as well, and what needs an unlocked one reports as skipped. `--in-back` leaves out what needs the app in front while another app is there; without it such a check fails |
| `cargo xtask selftest resize` | widens the window after moving the divider: the source re-wraps while the window is resized. Then shows and hides the sidebar: source and preview keep their share, and a new window opens the sidebar as the last one left it. Takes about ten seconds |
| `cargo xtask selftest typing [--by-hand]` | types through saves, with keys through the app's event queue or, with `--by-hand`, from a person: what is typed stays and shows as unsaved. Takes about a minute. On a locked screen the keys go straight to the window, and what needs the app in front reports as skipped |
| `cargo xtask selftest bench <file.md> [--keystroke-ms <n>]` | timings for opening, typing and scrolling a large file; `--keystroke-ms` fails when the fastest keystroke takes longer |
| `cargo test --release -- --ignored partial_updates` | a keystroke in 10 MB: partial against full update, the same result in a quarter of the time at most |
| `cargo test --release --test hostile --test hostile_import` | documents built to overflow the stack or take quadratic time or memory, each at two sizes whose time and peak memory must grow as the text does, through parsing, layout, both exports, keystrokes and commands, and the same for repositories, notes and foreign formats, also a ZIP bomb and a Word file full in every part. Time counts only with `--release` on Apple Silicon, one measurement at a time; memory counts in every build |
| `make mutants` | faults built into `crates/splat-markdown`, `crates/splat-preview`, `crates/splat-file-state`, `crates/splat-git`, `crates/splat-import` and `crates/splat-notes` one by one, through [cargo-mutants](https://mutants.rs) (`cargo install cargo-mutants`); says which ones no test notices. Takes hours; `FILE=crates/splat-preview/src/bridge/mod.rs` narrows it |
| `make fuzz` | random input through [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), which needs rustup's nightly: keystrokes that must read as a full parse, every output, edits in the preview, every command, a file that saves as it opened, foreign formats, the objects and comparisons of Git, and word counts, metadata, searches and renames. `SECONDS=600` searches longer |
| `cargo test --release --test fuzz_seeds` | the seeds in `fuzz/seeds/` through the checks of those fuzz targets, without a fuzzer; part of every `cargo test` |
| `cargo test --profile checked --workspace` | the tests with what a debug build checks on top: after every partial parse the blocks fit their text, and what it carries on is what the text says. Part of `make test-long` |
| `cargo test --release --test entry_points --test structure` | from the sources: everything the system calls is guarded against panics, modules depend downwards only, and the names in the tables of `docs/` exist in the code |
| `cargo test --release --test decoder` | the decoder process decodes images and reaches neither files nor the network |
| `make coverage` | line coverage of `cargo test` and all self-tests, through [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (`brew install cargo-llvm-cov llvm`); fails below the floors. Occupies the Mac for about five minutes, also on a locked screen |
| `make coverage-library` | the crates `splat-markdown`, `splat-preview`, `splat-file-state`, `splat-git`, `splat-import`, `splat-ooxml`, `splat-notes`, `splat-editor-state` and `splat-i18n` against their higher floors, measured by `cargo test` alone |

Unit tests sit next to the code they test. `crates/splat-testkit/tests/` holds what reaches the model and the preview
from outside: the randomised tests in `robustness.rs`, which share the generator of `splat-testkit` with those of
`sync.rs` in `splat-editor-state`, the documents built to hurt in `hostile.rs`, and the seeds of the fuzz targets in
`fuzz_seeds.rs`. `tests/` holds what looks at all of the program: its sources, its translations and the decoder test,
which starts the built program. What the window decides without needing one lives in `crates/splat-editor-state/`, whose
tests `make coverage-library` holds to the higher floors; in `src/editor/` the rest are free functions with tests of
their own: paragraph styles, fonts and colours as values in `render.rs`, what toolbar identifiers stand for in
`toolbar.rs`.

A step of the window self-test waits for a named condition, not for a time; only what gives no signal, like the absence of something, keeps a pause with its reason. The self-test build runs panes and popovers without their animations, which never end without a display, and no check looks at an animation. Waiting in vain is a failed check that names what never came, a step that panics is a failed check as well, and the run goes on either way. A check that cannot say anything in this run reports `SELFTEST SKIP`: xtask lists it, and it neither passes nor fails. xtask holds each mode, and `--scenarios` of its own, to about nine tenths of its checks. The window
self-test runs a second part after its main click-through, in `src/editor/selftest/scenarios/`: every formatting and
table command, the dialogs, link clicks, pasting, dropping, exports, a file another program changed, a large change, images
changing on disk, memory pressure, a huge document, the word count, the marks for what changed, the notes in the sidebar, the
metadata form, the export options, importing Word and the recovery after a crash. Each group of it opens a document of its
own, so that a failure in one does not reach the next, and each can run alone with `--only`. The groups of the
click-through do the same: each opens a document holding the text and carets the groups before it leave, kept in
`xtask/checks/click-through/` and written anew by `--record`, which runs them one after the other on one document. What the
self-test build does differently from the app that ships stands in `src/test_hooks.rs`, and `make coverage` names every
check that was skipped. The probe also breaks saving on
purpose: a folder that takes nothing, a read-only file, a folder that has gone, a full disk, and a panic in every callback of
a save, none of which may cost text.

Coverage has two floors: every module at least 84 % of its lines, all modules together at least 96 %. `make coverage`
measures `cargo test` together with the self-tests and fails below either; `make coverage-library` holds what needs no window to 90 % and 97 %. The floors and the few exceptions, such
as the panic hook in `main.rs` that runs only in a crash, are in [xtask](../xtask/src/coverage.rs) next to their reasons.
Measured builds go to `target/coverage/`, so measuring code never reaches `target/release/` or a disk image. The
decoder helper opens its measurement file before it locks itself in, so it is measured as well.

CI ([.github/workflows/ci.yml](../.github/workflows/ci.yml)) runs `make check`, `make coverage-library` and the universal
build on Apple Silicon and `cargo test` on Intel for every push to `main` and every pull request. Every night,
[.github/workflows/nightly.yml](../.github/workflows/nightly.yml) runs `make test-long` with new random inputs. The self-tests and the disk image stay on the developer's Mac: they need a screen and the
signing certificates in the keychain.

Self-test output goes to `target/selftest/`. The self-test builds a separate app (`target/release/Splat Selftest.app`,
bundle ID `local.splat.selftest`) and leaves the settings, recent documents and file associations of the normal app
untouched.

## Project layout

Everything that needs no window is a crate of its own under `crates/`, which builds without macOS, so the compiler holds the
way they depend on each other. The app in `src/` (package `splat-app`, program `splat`) builds on them.

- `crates/splat-markdown/`: the tree every consumer reads, `Doc` in `doc/`, which owns its text, and the only binding to the
  parser in `reader/`: comrak is named nowhere else, and `reader/assumptions.rs` holds what the code assumes about it.
  Syntax colours, and all commands as pure text edits (`Edit`) in `commands/`. Also re-parsing after an edit
  (`incremental.rs`), the reader for embedded HTML (`markup.rs`), typography, tags, references and citations in running
  text (`typography.rs`), how text looks and which Markdown and HTML make it so (`style.rs`), what an address may be
  (`url.rs`), the word count (`counts.rs`), reading and editing front matter line by line (`front_matter.rs`), and the HTML and
  Word export (`html.rs`, `docx.rs` with `docx/html.rs` and `docx/xml.rs`). A position has
  its type: `Byte` in the source text, `U16` in the source view, `Shown` in the preview view (`text.rs`).
- `crates/splat-preview/`: the preview layout in `layout/`, with embedded HTML in `layout/html.rs`, and the mapping between
  preview and source (`Bridge`, `Index`). A `Bridge` refuses a layout of another version of the `Doc`.
- `crates/splat-file-state/`: which text a file holds, in which format, and what a look at it means.
- `crates/splat-import/`: foreign formats as Markdown through one model in `rich.rs`: HTML, RTF in the code pages its
  fonts name (`rtf/code_page.rs`) and separated text from the clipboard, Word files with their formulas in `docx/`, the
  Word package that import and templates share in `package.rs`, one budget for all the XML of an import or a merge in
  `xml.rs`, and the look of a Word template in `template/`.
- `crates/splat-ooxml/`: what reading and writing Word files share: ZIP archives in `zip.rs`, read from anywhere within
  limits and written as the export stores them, their CRC-32 checksum, and XML escaping in `xml.rs`.
- `crates/splat-git/`: what a file holds in the last commit, read from the repository's files without the `git` program, and
  where a text differs from it (`diff.rs`); also how a Mac volume compares file names (`folded_name`).
- `crates/splat-notes/`: the folder around a document, the search through it, what its links resolve to and what a rename
  changes in them.
- `crates/splat-editor-state/`: what the editor window knows and decides without a window. Everything the controller
  knows about the document and every way that changes in `state.rs`, whose fields are private; `model.rs`, which lends
  the state and the text copy to closures that cannot reach a view; the text copy and the block bookkeeping in
  `sync.rs`, with randomised tests; the rules of what WebKit renders when in `render_queue.rs`, and when an update runs
  and which blocks of a large preview render when in `progress.rs`. Also what a link click means and where opening a
  link leads (`navigation.rs`, `links.rs`), the BibTeX file (`bibtex.rs`) and the code host of the repository
  (`repository.rs`), the sidebar's rows, searches and renames (`notes.rs`), the outline (`outline.rs`), the marks for
  what changed since the last commit (`changes.rs`), what an edit in the preview changed and what a drop inserts
  (`editing.rs`), new files beside a document (`files.rs`), the metadata form (`metadata.rs`), the word count
  (`word_count.rs`), and the worker threads and `contained` (`lanes.rs`).
- `crates/splat-i18n/` with `resources/*.lproj`: 35 languages; the keys are the German texts, and the app installs the table
  of the interface language at its start.
- `crates/splat-testkit/`: the generator of the randomised tests and the checks of the fuzz targets.
- `src/editor/`: the window. One `EditorController` object serves AppKit as delegate of both views and forwards to
  components, small structs over the controller and the state they own, whose fields the other modules cannot reach:

| Module | Responsibility |
|---|---|
| `controller.rs` | class, delegate methods that forward, `invalidate`, and what the rest of the program tells an editor; only it reaches the components |
| `panes.rs` | `Panes`: the text views, the outline, showing and hiding panes, scrolling and insets of the panes, and what gives access to the views; `ViewReader` can only ask them |
| `controller/layers.rs` | who may reach whom: a component gets a handle of the controller that offers what lies below it, and `Object`, the controller as AppKit knows it; what it needs from above or what arrives for it later goes to the controller as a `Message`, through a `Messenger` from other threads and sheets. The compiler holds to that |
| `geometry.rs` | `TextGeometry`: where the text of a view lies and how it shows a highlight, in TextKit 2 for the source and TextKit 1 for the preview |
| `actions.rs` | command registry: one row per command gives its `Action`, its `Spec` (selector, title, shortcut, symbol, when the command applies) and the controller's method for it |
| `editing.rs` | `Editing`: running and validating commands, typing in the preview |
| `update.rs` | `Updater`: parsing, colouring, updating the preview; after an edit only the affected paragraphs |
| `progressive.rs` | `Progressive`: rendering large previews step by step |
| `selection.rs`, `scroll.rs` | `Selection`, `ScrollSync`: selection and scroll position of both panes in step |
| `atom.rs`, `atom/edit.rs` | `AtomEditor`: the formula and diagram editor |
| `navigation.rs` | `Links`: links, wiki links, tags, task checkboxes, folding, footnotes |
| `env.rs` | what the preview reads from outside the text: typographic punctuation, the repository for `@name` and `#12`, the BibTeX file, embedded notes |
| `render.rs`, `images.rs`, `picture.rs`, `remote.rs`, `safe_image.rs` | preview text, local and remote images; `Pictures` in `images.rs` |
| `attributes.rs` | text attributes; each key only gets a value of the type it expects |
| `counter.rs`, `changes.rs` | `WordCounter`, `ChangeMarks`: the word count in the corner and the marks for what changed since the last commit |
| `notes.rs`, `sidebar.rs` | `Notes`: the folder of notes, searching, opening and renaming; the views of the sidebar |
| `metadata.rs`, `export_options.rs` | the metadata form; what exports add to the text |
| `shadow.rs` | copies of unsaved text for the recovery after a crash |
| `decoder.rs` | sandboxed helper processes that decode images and graphics |
| `export.rs`, `outline.rs` | `Sharing`: HTML and Word export, the share file, printing; the outline sidebar |
| `js.rs`, `mathfield.rs` | WebKit for MathJax, Mermaid, Graphviz, PlantUML and the visual formula editor |
| `webkit.rs` | calls into WebKit; only the navigation delegate's lifetime stays with `js.rs` and `mathfield.rs` |
| `controller/window.rs`, `toolbar.rs`, `views.rs`, `dialogs.rs`, `style.rs` | building the window, toolbar, text views, dialogs, colours |

- `src/menu.rs`, `src/app.rs`, `src/document.rs`: menu bar, app delegate, document (`MDDocument`, an `NSDocument`
  subclass). `src/word_import.rs`: asking for a Word document and writing its Markdown and images.
- `src/util/`: worker threads (`lanes.rs`), volumes that may stop answering (`volumes.rs`), AppKit helpers, files and
  processes. `src/diag.rs`: what goes wrong out of the user's sight goes to the system log, without any text of a document.
- `src/large_alloc.rs`: large allocations come straight from the kernel and go back to it when freed, so memory
  returns after big edits.
- `resources/js/`: web pages and libraries; versions and checksums in [vendor.lock](../vendor.lock).
- `xtask/`: bundling, signing, the disk image, the icon, vendoring, the self-test runner and the coverage floors.

## Data flow

The source text in the left text view is the single source of truth.

1. Every character change there updates the text copy (`Snapshot`) in place, without reading the text again.
2. An update parses the copy, recolours the source only where colours changed, lays out the preview and replaces only
   the blocks whose content changed.
3. After an edit, only the affected paragraphs are parsed, coloured and laid out again, as long as their neighbours
   stay the same; [docs/partial-parse.md](partial-parse.md) says why that gives what a full parse gives, and what to
   look at when comrak changes. Documents with `%%` comment blocks, changes to the
   front matter and, in a document with link reference definitions, changes to a part holding a bracket are processed
   in full. comrak recognizes a footnote
   reference only with its definition in the same parse and moves definitions behind the text, so the part is parsed
   with a stand-in definition per label and takes its numbers from the old tree; a change to the references or
   definitions in the part, or next to an inline footnote, is processed in full.
4. HTML blocks that open `<details>` or `<div align>` and close it later affect the Markdown blocks in between; a
   reused block must have the same surroundings.
5. While you type in the source of a document up to 2 MB, the partial update runs immediately; when it does not apply,
   or in a larger document, parsing and layout run in the background. While you type in the preview, they run
   immediately, because the preview's cursor depends on the result.
6. Input in the preview is translated by `Bridge` into an edit of the source and takes the same path from there.
   Markdown has too many ways of turning text into syntax for rules alone, so `preview/verify.rs` parses the changed part
   again and compares what it shows with what was typed; an edit that does not hold is escaped further or refused.
7. comrak's inline positions go wrong in a few places, for example behind a link reference definition. `reader/positions.rs`
   checks them against the source when reading a block and reads them again from the source where they do not fit.

## Rules in the controller

1. The controller's own changes to the views run inside `Panes::silently`; the delegate callbacks ignore them. Everywhere
   else the views can only be read, so the compiler checks this. What the callbacks are meant to hear, or what hands a
   view to AppKit, says so by going through `Panes::for_appkit`.
2. The state is borrowed only to read and compute, never across a call that changes, lays out or scrolls a view: `Model`
   lends it to `Send` closures, and no view is `Send`, so the compiler checks this. It changes through its own methods
   only, whose tests need no window.
3. Messages from outside (an image loaded, a formula rendered) arrive delayed through `renders_arrived` and `image_arrived`, never in the
   middle of an update. What another thread, a sheet or a panel answers reaches the controller as a `Message` through a
   `Messenger`, on the main thread and only while the window is open.
4. Redrawing without a text change goes through `invalidate(reason)` only.
5. The text copy is not held across a change of the source, or that change copies the whole text: `with_text` lends it
   for the length of a `Send` closure only.
6. A new command needs a row in `actions.rs`, which gives it its method in `controller.rs` as well, and a branch in
   `Editing::perform`, which the compiler asks for.
7. Every `unsafe` block carries its justification as a `// SAFETY:` comment. Bytes reach the system's image decoders in
   the helper process only, through `decoder::decode_native`; a test scans the sources for other calls.
8. Nothing on the main thread waits for the user's folders, since a network volume can stop answering: files are
   looked at on the thread `DISK`, and the preview reads the answers through `Env::known`. Only an action the user
   waits for, like an export or saving, may touch the disk directly.
9. What has to end also ends after a panic, as the main thread survives one: a raised flag goes through `with_raised`, an
   editing of the text storage through `editing`, and what undo is not to know of through `without_undo`. A test finds these
   written by hand.
10. An address in a document has one reading, in `splat_markdown::url`; a test finds schemes compared by hand.

## Adding a command

The menu bar, the toolbar and the context menu all build their items from the registry in
[src/editor/actions.rs](../src/editor/actions.rs), and menu validation reads there when a command applies and whether it
shows a checkmark. A new command needs:

1. a row of `commands!`: the variant of `Action`, its selector, when it applies and what of its `Spec` it has: title,
   toolbar label, tooltip (starting with a verb, at most 75 characters), shortcut and SF Symbol. The same row gives
   `src/editor/controller.rs` the method AppKit finds the command's target by
2. a branch in `Editing::perform`, which the compiler asks for; a command with variants in a submenu reads the one
   chosen as a `Choice` of `Action::choices`
3. its texts in every `Localizable.strings`

The test `every_command_has_a_method_and_every_method_a_command` checks that registry and methods match. Apple's
standard shortcuts such as ⌘E, ⌘J, ⌘T, ⌘;, ⇧⌘C, ⌥⌘C, ⌥⌘V, ⇧⌘P, ⌥⌘T, ⌃⌘F and ⌘, stay untouched.

## Translations

Code uses the German text as key: `t("Fett")` looks it up at run time, `n("Fett")` marks a constant for later
lookup. Each `resources/<language>.lproj/` holds:

- `Localizable.strings`: every interface text, menu terms after Apple's own glossaries
- `InfoPlist.strings`: the name macOS shows for the document type
- `QuickReference.md`: the Markdown quick reference

`cargo xtask bundle` lists every `.lproj` folder as a localization, and `CFBundleDevelopmentRegion` is English, so
the system menus and the save dialog follow the same language. The i18n test checks that every language has every
text and that every quick reference has the outline of the German one.

`assets/readme/<language>/` holds this README in that language, its screenshot and the demo document the screenshot
shows. A change to this README belongs in those copies as well.
