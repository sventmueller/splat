# Splat for Windows 0.2.0 (feature parity preview)

This is the Windows desktop port of Splat, built with Tauri 2 and Microsoft Edge WebView2. It lives under `windows/` and remains separate from the macOS AppKit implementation. The macOS `main` branch is not changed by this port.

## Migrated features

The Windows shell reuses the repository's platform-independent Rust crates and local assets instead of introducing a second Markdown engine or CDN dependencies.

- **Markdown editing:** split source/preview, source-only and preview-only layouts, syntax highlighting, outline, formatting shortcuts, HTML clipboard import and Git changes.
- **Math and diagrams:** bundled MathJax 4 for inline/display formulas, plus bundled Mermaid, Graphviz and PlantUML with local themes. They render offline.
- **Two-way preview:** the rendered page is editable. Edits are converted back to Markdown through Splat's HTML import pipeline. Formula/diagram figures themselves remain protected objects; edit their source block in the Markdown pane.
- **Folder notes:** choose a folder to browse nested Markdown notes, search text/tags/backlinks, jump to a matching line, and rename notes. Rename uses Splat's link resolver and rewrites affected links in the same note tree.
- **Exports:** Word `.docx` through Splat's OOXML writer, including local images and rasterized formula/diagram figures when available; self-contained HTML from the rendered preview; and PDF via the Windows WebView2 print dialog (choose **Microsoft Print to PDF**).
- **File and recovery:** UTF-8/UTF-16 BOM and CRLF preservation, Windows Markdown file associations, and a local crash-recovery draft stored in the app-data folder. The app asks before restoring a draft from an earlier session.
- **Native shell:** Windows menu for common editing/export actions, file-open launch handling and an entry to the GitHub Releases page for update checks.

No document upload or telemetry is added by this port. Local image paths are resolved relative to the Markdown file and embedded as data for preview/export; remote images are blocked in the live preview.

## Build on Windows 10 / 11 (x64)

Prerequisites:

- Rust toolchain pinned by `rust-toolchain.toml`, with target `x86_64-pc-windows-msvc`.
- Microsoft C++ Build Tools with **Desktop development with C++**.
- Microsoft Edge WebView2 Runtime.
- NSIS for the setup executable.

From the repository root in PowerShell:

```powershell
./windows/prepare-preview.ps1
npm install --global @tauri-apps/cli@2.12.2
cd windows/src-tauri
tauri dev
```

The preparation script stages the licensed bundled JavaScript resources from `resources/js` into the Windows frontend. CI and `windows/build.ps1` run it automatically. Run it before starting `tauri dev`.

Build the NSIS installer:

```powershell
tauri build --bundles nsis
```

Output: `windows/src-tauri/target/release/bundle/nsis/`.

## Release, signing and updates

`.github/workflows/windows-release.yml` builds the x64 installer on Windows runners. Successful builds are uploaded as the `Splat-Windows-x64` Actions artifact. Windows-port builds also create a GitHub Pre-release.

The installer is currently unsigned. Windows SmartScreen may show an unknown-publisher warning. Do not disable Defender or SmartScreen merely to install an unreviewed binary.

The **Check for Updates** menu opens the project's GitHub Releases page. Silent/in-app updates are intentionally not enabled yet: Tauri's updater requires a public verification key in the app config and a private signing key in protected CI secrets. Windows code signing also requires a signing certificate. Keep private keys and certificates in GitHub Actions secrets; never commit them to the repository or paste them into chat.

## Current limitations

- Preview HTML is round-tripped through the safe HTML-to-Markdown importer. It preserves common editing operations, but it is not a byte-for-byte replacement for the macOS preview's positional mapping engine.
- PDF uses the system print dialog rather than the macOS native PDF export pipeline.
- Formula/diagram SVGs are protected in preview; edit the original TeX or diagram code in the source.
- The first Windows release is x64 and unsigned.
