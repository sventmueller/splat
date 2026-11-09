# Contributing

Problems go into an [issue](https://github.com/sventmueller/splat/issues/new/choose), ideas and questions into
[Discussions](https://github.com/sventmueller/splat/discussions). Security problems follow the
[security policy](SECURITY.md).

## Pull requests

Small fixes can go straight to a pull request. For anything larger, describe the idea in
[Discussions](https://github.com/sventmueller/splat/discussions/new?category=ideas) first, so that no work goes into a
change Splat will not take.

- [docs/development.md](docs/development.md) explains how to build and test Splat.
- `make check` must pass. It covers formatting, clippy without warnings, all tests and the vendored libraries.
- New interface texts need a translation in every language, see [Translations](docs/development.md#translations). The tests fail
  without them.
- Keep to one change per pull request.

## Windows and Linux

Ports to other systems are welcome. They keep the premises of the Mac app and what the [README](README.md) promises:

- All program logic is Rust, none of it JavaScript. What does not depend on the system lives in the crates under
  [crates/](crates/) and is shared by every platform, not copied.
- The interface uses the system's toolkit, such as WinUI or Win32 on Windows and GTK or Qt on Linux. An interface built
  as a web page, in Tauri, Electron or a similar shell, is not taken.
- It follows the design guidelines of its platform, as the Mac app follows Apple's
  [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/): Microsoft's
  [design guidelines for Windows apps](https://learn.microsoft.com/windows/apps/design/guidelines-overview), the
  [GNOME HIG](https://developer.gnome.org/hig/) for GTK and the [KDE HIG](https://develop.kde.org/hig/) for Qt.
- A web view is used only where a bundled JavaScript library does the work, as on macOS: drawing formulas and diagrams
  (MathJax, Mermaid, Graphviz, PlantUML) and the visual formula editor (MathLive). It never shows the text itself.
- Every interface text goes through `splat-i18n` and the tables in `resources/*.lproj`, in all 35 languages, with
  right-to-left layout for Arabic and Hebrew. The interface follows the system's language, and the translation tests
  cover the port's sources.
- Whatever edits the text, in the source or in the preview, changes only what it must: lines the user did not touch stay
  byte-for-byte identical, and so do the file's encoding and line endings.
- No telemetry: the network carries only the pictures from the web a document shows and the update checks the user
  agreed to. Links to other apps ask first, and whatever decodes untrusted image data runs in a sandboxed helper process.
- The port runs the same checks as `make check` on its own system: formatting, clippy without warnings and all tests,
  the translation tests included. CI runs them on every pull request, and a failing step fails the run.
- Installers come only from the releases of this repository and are signed where the platform signs apps, such as
  Authenticode on Windows. A port links to no other download.
- Describe the port in [Discussions](https://github.com/sventmueller/splat/discussions/new?category=ideas) before
  writing code.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Splat by you, as
defined in the Apache-2.0 license, shall be dual licensed under Apache-2.0 and MIT, without any additional terms or
conditions.
