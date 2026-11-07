# Splat for Windows 0.2.0 (feature parity preview)

- MathJax 4 formula rendering and offline Mermaid, Graphviz and PlantUML diagrams.
- Editable rendered preview with Markdown write-back.
- Folder note tree, text/tag/backlink search, and note renaming with link rewriting.
- Word (.docx) export through Splat's OOXML writer, with local images and rendered figures where available.
- Self-contained HTML export and Windows print-to-PDF workflow.
- Native Windows menu, Markdown file associations and command-line file-open handling.
- Local crash-recovery draft snapshots with an explicit restore prompt.
- Reuses Splat's Markdown renderer, file-format preservation, safe Git diff and HTML clipboard importer.

This is a preview release. PDF output uses the Windows print dialog; choose Microsoft Print to PDF. The NSIS installer is currently unsigned. Automatic updater installation is disabled until a Tauri updater signing key and Windows code-signing certificate are configured securely.
