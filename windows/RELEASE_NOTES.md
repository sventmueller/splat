# Splat for Windows (feature parity preview)

- Offline formula rendering through bundled MathJax 4.
- Offline Mermaid, Graphviz and PlantUML diagram rendering.
- Editable rendered preview with Markdown write-back.
- Folder-wide note tree, text/tag/backlink search and safe rename with link rewriting.
- Word (.docx) export using Splat's native OOXML writer; embedded images and rendered figures are included when available.
- HTML export from the rendered page, including locally rendered figures.
- Print dialog for PDF output (select Microsoft Print to PDF on Windows).
- Reuses the existing file-format preservation, Git diff, Markdown renderer and HTML clipboard importer.

This remains a preview release. Formula/diagram editing in the source is supported; rendered figure objects are protected from direct inline edits. PDF output uses the system print dialog. The first Windows build is unsigned.
