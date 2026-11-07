(() => {
  const tauri = window.__TAURI__;
  const invoke = tauri?.core?.invoke;
  const dialog = tauri?.dialog;
  const $ = (selector) => document.querySelector(selector);

  const editor = $("#source-editor");
  const highlightLayer = $("#highlight-layer");
  const previewFrame = $("#preview-frame");
  const stage = $("#editor-stage");
  const state = {
    path: null,
    name: "Untitled.md",
    content: "# Welcome to Splat\n\nA focused Markdown workspace. Write on the left and see the rendered document on the right.\n\n## A small tour\n\n- **Edit** Markdown in the source pane\n- See headings, lists, links and tables in the live preview\n- Save as `.md`, `.markdown`, `.mdown` or `.mkd`\n- Check line changes against the last Git commit\n\n> Your writing stays on this device. The Windows preview does not send document contents over the network.\n\n```rust\nfn main() {\n    println!(\"Hello, Splat!\");\n}\n```\n",
    dirty: false,
    format: { bom: false, crlf: false, encoding: "utf-8" },
    renderSequence: 0,
    previewRequestId: 0,
    previewReady: false,
    pendingRender: null,
    pendingPreviewRequests: new Map(),
    rootFolder: null,
    view: "split",
  };

  let renderTimer = null;
  let toastTimer = null;
  let recoveryTimer = null;

  function toast(message) {
    const node = $("#toast");
    node.textContent = message;
    node.classList.add("visible");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => node.classList.remove("visible"), 2300);
  }

  function ensureDesktopBridge() {
    if (invoke && dialog) return true;
    toast("Run this screen inside the Splat Tauri desktop app.");
    return false;
  }

  function escapeHtml(value) {
    return value.replace(/[&<>"']/g, (character) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[character]);
  }

  function highlightMarkdown(text) {
    let fenced = false;
    let fenceToken = "";
    return text.split("\n").map((line) => {
      const fence = line.match(/^\s*(```+|~~~+)/);
      if (fence) {
        if (!fenced) {
          fenced = true;
          fenceToken = fence[1][0];
        } else if (fence[1][0] === fenceToken) {
          fenced = false;
        }
        return `<span class="tok-fence">${escapeHtml(line)}</span>`;
      }
      if (fenced) return `<span class="tok-code">${escapeHtml(line)}</span>`;

      const heading = line.match(/^(\s{0,3}#{1,6}\s+)(.*)$/);
      if (heading) return `<span class="tok-heading">${escapeHtml(line)}</span>`;
      if (/^\s*&gt;|^\s*>/.test(line)) return `<span class="tok-quote">${escapeHtml(line)}</span>`;

      let html = escapeHtml(line);
      html = html.replace(/(`[^`]+`)/g, '<span class="tok-code">$1</span>');
      html = html.replace(/(!?\[[^\]]*\]\([^)]*\))/g, '<span class="tok-link">$1</span>');
      html = html.replace(/(\*\*[^*]+\*\*|__[^_]+__)/g, '<span class="tok-em">$1</span>');
      html = html.replace(/(\*[^*]+\*|_[^_]+_)/g, '<span class="tok-em">$1</span>');
      html = html.replace(/^(\s*(?:[-+*]|\d+\.)\s+)/, '<span class="tok-list">$1</span>');
      return html || "&nbsp;";
    }).join("\n") + "\n";
  }

  function updateHighlight() {
    highlightLayer.innerHTML = highlightMarkdown(editor.value);
    highlightLayer.scrollTop = editor.scrollTop;
    highlightLayer.scrollLeft = editor.scrollLeft;
  }

  function updateOutline() {
    const list = $("#outline-list");
    const matches = [];
    let offset = 0;
    for (const line of editor.value.split("\n")) {
      const match = line.match(/^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$/);
      if (match) matches.push({ level: match[1].length, title: match[2], offset });
      offset += line.length + 1;
    }
    list.replaceChildren();
    if (!matches.length) {
      const empty = document.createElement("p");
      empty.className = "empty-note";
      empty.textContent = "Headings in this document will appear here.";
      list.append(empty);
      return;
    }
    for (const item of matches) {
      const button = document.createElement("button");
      button.className = `outline-item level-${Math.min(item.level, 3)}`;
      button.textContent = item.title;
      button.title = item.title;
      button.addEventListener("click", () => {
        editor.focus();
        editor.setSelectionRange(item.offset, item.offset);
        const lineNumber = editor.value.slice(0, item.offset).split("\n").length - 1;
        editor.scrollTop = Math.max(0, lineNumber * 22.5 - 45);
      });
      list.append(button);
    }
  }

  function updateStatus() {
    const text = editor.value;
    const words = text.trim() ? text.trim().split(/\s+/u).length : 0;
    const lines = text.split("\n").length;
    const beforeCursor = text.slice(0, editor.selectionStart);
    const cursorLine = beforeCursor.split("\n").length;
    const cursorColumn = beforeCursor.length - beforeCursor.lastIndexOf("\n");
    $("#word-count").textContent = `${words} ${words === 1 ? "word" : "words"}`;
    $("#character-count").textContent = `${text.length} characters`;
    $("#cursor-position").textContent = `Ln ${cursorLine}, Col ${cursorColumn}`;
    $("#status-message").textContent = `${lines} ${lines === 1 ? "line" : "lines"}`;
    $("#encoding-label").textContent = state.format.encoding.toUpperCase().replace("UTF-16LE", "UTF-16 LE").replace("UTF-16BE", "UTF-16 BE");
    $("#line-ending-label").textContent = state.format.crlf ? "CRLF" : "LF";
  }

  function updateDocumentChrome() {
    $("#document-name").textContent = state.name;
    $("#document-path").textContent = state.path || "New document";
    $("#dirty-indicator").classList.toggle("hidden", !state.dirty);
    document.title = `${state.dirty ? "● " : ""}${state.name} · Splat`;
  }

  function postPreviewRequest(type, payload = {}) {
    if (!state.previewReady || !previewFrame.contentWindow) {
      return Promise.reject(new Error("The preview frame is not ready."));
    }
    const id = ++state.previewRequestId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        state.pendingPreviewRequests.delete(id);
        reject(new Error("The preview did not respond in time."));
      }, 45000);
      state.pendingPreviewRequests.set(id, { resolve, reject, timer });
      previewFrame.contentWindow.postMessage({ type, id, ...payload }, "*");
    });
  }

  async function renderPreview() {
    if (!invoke || !state.previewReady) return null;
    const sequence = ++state.renderSequence;
    const html = await invoke("render_markdown", {
      markdown: editor.value,
      title: state.name,
      documentPath: state.path,
      allowRemoteImages: false,
    });
    if (sequence !== state.renderSequence) return null;

    if (state.pendingRender) {
      clearTimeout(state.pendingRender.timer);
      state.pendingRender.resolve(null);
      state.pendingRender = null;
    }
    const id = ++state.previewRequestId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (state.pendingRender && state.pendingRender.id === id) state.pendingRender = null;
        reject(new Error("The preview did not finish rendering in time."));
      }, 45000);
      state.pendingRender = { id, resolve, reject, timer };
      previewFrame.contentWindow.postMessage({
        type: "splat:render",
        id,
        html,
        editable: true,
        dark: window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches,
      }, "*");
    });
  }

  async function collectPreview() {
    await renderPreview();
    return postPreviewRequest("splat:collect");
  }

  function onPreviewMessage(event) {
    if (event.source !== previewFrame.contentWindow || !event.data || event.data.source !== "splat-preview") return;
    const message = event.data;
    if (message.type === "splat:rendered") {
      const pending = state.pendingRender;
      if (!pending || pending.id !== message.id) return;
      clearTimeout(pending.timer);
      state.pendingRender = null;
      pending.resolve(message);
      if (message.errors && message.errors.length) toast("Some formulas or diagrams could not be rendered.");
      return;
    }
    if (message.type === "splat:collected" || message.type === "splat:printed") {
      const pending = state.pendingPreviewRequests.get(message.id);
      if (!pending) return;
      clearTimeout(pending.timer);
      state.pendingPreviewRequests.delete(message.id);
      pending.resolve(message);
      return;
    }
    if (message.type === "splat:open-link") {
      const href = String(message.href || "");
      if (/^https?:\/\//i.test(href) || /^mailto:/i.test(href)) window.open(href, "_blank", "noopener,noreferrer");
      return;
    }
    if (message.type === "splat:preview-edit") {
      invoke("paste_html_as_markdown", { html: String(message.html || "") })
        .then((markdown) => {
          if (typeof markdown !== "string") return;
          editor.value = markdown;
          documentChanged();
          toast("Preview changes applied to Markdown.");
        })
        .catch((error) => toast("Could not convert preview edit: " + String(error)));
    }
  }


  function scheduleRender() {
    clearTimeout(renderTimer);
    renderTimer = setTimeout(() => {
      renderPreview().catch((error) => toast("Preview failed: " + String(error)));
    }, 110);
  }

  function scheduleRecoverySnapshot() {
    clearTimeout(recoveryTimer);
    if (!state.dirty || !invoke) return;
    recoveryTimer = setTimeout(() => {
      invoke("save_recovery_snapshot", {
        snapshot: {
          path: state.path,
          name: state.name,
          content: editor.value,
          format: state.format,
          savedAt: Date.now(),
        },
      }).catch((error) => console.warn("Splat recovery snapshot failed:", error));
    }, 1400);
  }

  async function clearRecoverySnapshot() {
    if (!invoke) return;
    clearTimeout(recoveryTimer);
    try { await invoke("clear_recovery_snapshot"); }
    catch (error) { console.warn("Could not clear recovery snapshot:", error); }
  }

  async function restoreRecoverySnapshot() {
    if (!invoke) return;
    try {
      const snapshot = await invoke("load_recovery_snapshot");
      if (!snapshot) return;
      const age = Date.now() - Number(snapshot.savedAt || 0);
      if (age > 30 * 24 * 60 * 60 * 1000) {
        await clearRecoverySnapshot();
        return;
      }
      const restore = window.confirm(
        "Splat found a recovery draft from an earlier session. Restore " + snapshot.name + "?"
      );
      if (!restore) {
        await clearRecoverySnapshot();
        return;
      }
      setDocument(snapshot);
      state.savedContent = "";
      state.dirty = true;
      updateDocumentChrome();
      scheduleRecoverySnapshot();
      toast("Recovered draft restored");
    } catch (error) {
      toast("Could not restore recovery draft: " + String(error));
    }
  }

  async function openStartupFile() {
    if (!invoke) return false;
    try {
      const path = await invoke("startup_file");
      if (typeof path !== "string" || !path) return false;
      const opened = await invoke("read_markdown_file", { path });
      setDocument(opened);
      state.rootFolder = opened.path.replace(/[\\/][^\\/]+$/, "");
      await clearRecoverySnapshot();
      toast("Opened " + opened.name);
      return true;
    } catch (error) {
      toast("Could not open the file passed by Windows: " + String(error));
      return false;
    }
  }

  function handleNativeMenu(id) {
    switch (id) {
      case "new": newDocument(); break;
      case "open": openFile(); break;
      case "save": saveFile(false); break;
      case "save-as": saveFile(true); break;
      case "export-html": exportHtml(); break;
      case "export-word": exportWord(); break;
      case "export-pdf": exportPdf(); break;
      case "notes": showNotes(); break;
      case "search": showSearch(); break;
      case "check-updates":
        toast("Opening Splat releases. Signed automatic updates require a release-signing key.");
        window.open("https://github.com/lipeilin375/splat/releases", "_blank", "noopener,noreferrer");
        break;
      case "about": toast("Splat for Windows · Feature parity preview"); break;
      default: break;
    }
  }

  function documentChanged() {
    state.content = editor.value;
    state.dirty = state.content !== state.savedContent;
    scheduleRecoverySnapshot();
    updateDocumentChrome();
    updateHighlight();
    updateOutline();
    updateStatus();
    scheduleRender();
  }

  function setDocument(document) {
    state.path = document.path || null;
    state.name = document.name || "Untitled.md";
    state.content = document.content || "";
    state.savedContent = state.content;
    state.format = document.format || { bom: false, crlf: false, encoding: "utf-8" };
    if (!state.rootFolder && state.path) state.rootFolder = state.path.replace(/[\\/][^\\/]+$/, "");
    state.dirty = false;
    editor.value = state.content;
    updateDocumentChrome();
    updateHighlight();
    updateOutline();
    updateStatus();
    renderPreview().catch((error) => toast("Preview failed: " + String(error)));
    editor.focus();
    editor.setSelectionRange(0, 0);
  }

  function askDiscardChanges() {
    return !state.dirty || window.confirm("Discard unsaved changes to this document?");
  }

  async function openFile() {
    if (!ensureDesktopBridge() || !askDiscardChanges()) return;
    try {
      const selected = await dialog.open({
        multiple: false,
        title: "Open Markdown document",
        filters: [{ name: "Markdown", extensions: ["md", "markdown", "mdown", "mkd"] }],
      });
      if (!selected) return;
      const opened = await invoke("read_markdown_file", { path: selected });
      setDocument(opened);
      state.rootFolder = opened.path.replace(/[\\/][^\\/]+$/, "");
      await clearRecoverySnapshot();
      toast(`Opened ${opened.name}`);
      showGitChanges();
    } catch (error) {
      toast(`Open failed: ${String(error)}`);
    }
  }

  function withMarkdownExtension(path) {
    return /\.(md|markdown|mdown|mkd)$/i.test(path) ? path : `${path}.md`;
  }

  async function saveTo(path) {
    try {
      const destination = withMarkdownExtension(path);
      await invoke("save_markdown_file", {
        path: destination,
        content: editor.value,
        format: state.format,
      });
      state.path = destination;
      state.name = destination.split(/[\\/]/).pop() || "Untitled.md";
      state.content = editor.value;
      state.savedContent = editor.value;
      state.dirty = false;
      updateDocumentChrome();
      updateStatus();
      await clearRecoverySnapshot();
      toast("Saved on this device");
    } catch (error) {
      toast(`Save failed: ${String(error)}`);
    }
  }

  async function saveFile(saveAs = false) {
    if (!ensureDesktopBridge()) return;
    if (state.path && !saveAs) return saveTo(state.path);
    try {
      const path = await dialog.save({
        title: "Save Markdown document",
        defaultPath: state.name || "Untitled.md",
        filters: [{ name: "Markdown", extensions: ["md", "markdown", "mdown", "mkd"] }],
      });
      if (path) await saveTo(path);
    } catch (error) {
      toast(`Save failed: ${String(error)}`);
    }
  }

  function newDocument() {
    if (!askDiscardChanges()) return;
    setDocument({
      path: null,
      name: "Untitled.md",
      content: "",
      format: { bom: false, crlf: false, encoding: "utf-8" },
    });
    clearRecoverySnapshot();
    toast("New document");
  }

  function htmlEscape(value) {
    return String(value).replace(/[&<>"']/g, (character) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[character]);
  }

  async function exportHtml() {
    if (!ensureDesktopBridge()) return;
    try {
      const path = await dialog.save({
        title: "Export as HTML",
        defaultPath: state.name.replace(/\.(md|markdown|mdown|mkd)$/i, "") + ".html",
        filters: [{ name: "HTML document", extensions: ["html", "htm"] }],
      });
      if (!path) return;
      const rendered = await collectPreview();
      const page = "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n" +
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n" +
        "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data: https:; style-src 'unsafe-inline'; font-src data:\">\n" +
        "<title>" + htmlEscape(rendered.title || state.name) + "</title>\n<style>" + (rendered.css || "") +
        "</style>\n</head>\n<body><main id=\"out\">" + (rendered.body || "") + "</main></body>\n</html>";
      const destination = /\.html?$/i.test(path) ? path : path + ".html";
      await invoke("save_html_file", { path: destination, html: page });
      toast("Self-contained HTML exported.");
    } catch (error) {
      toast("HTML export failed: " + String(error));
    }
  }

  async function exportWord() {
    if (!ensureDesktopBridge()) return;
    try {
      const path = await dialog.save({
        title: "Export as Word document",
        defaultPath: state.name.replace(/\.(md|markdown|mdown|mkd)$/i, "") + ".docx",
        filters: [{ name: "Word document", extensions: ["docx"] }],
      });
      if (!path) return;
      const rendered = await collectPreview();
      await invoke("export_docx_file", {
        path: /\.docx$/i.test(path) ? path : path + ".docx",
        markdown: editor.value,
        title: state.name.replace(/\.(md|markdown|mdown|mkd)$/i, ""),
        documentPath: state.path,
        figures: rendered.figures || [],
      });
      toast("Word document exported.");
    } catch (error) {
      toast("Word export failed: " + String(error));
    }
  }

  async function exportPdf() {
    if (!ensureDesktopBridge()) return;
    try {
      await collectPreview();
      await postPreviewRequest("splat:print");
      toast("Choose Microsoft Print to PDF in the Windows print dialog.");
    } catch (error) {
      toast("PDF print failed: " + String(error));
    }
  }

  async function chooseNotesFolder() {
    if (!ensureDesktopBridge()) return null;
    try {
      const folder = await dialog.open({
        directory: true,
        multiple: false,
        title: "Choose a folder of Markdown notes",
      });
      if (typeof folder !== "string" || !folder) return null;
      state.rootFolder = folder;
      return folder;
    } catch (error) {
      toast("Folder selection failed: " + String(error));
      return null;
    }
  }

  async function loadNotes(root = state.rootFolder) {
    if (!root) {
      root = await chooseNotesFolder();
      if (!root) return;
    }
    const tree = await invoke("scan_notes", { root });
    state.rootFolder = root;
    $("#notes-root-label").textContent = root;
    const host = $("#notes-tree");
    host.replaceChildren();
    if (!tree.top || tree.top.length === 0) {
      const empty = document.createElement("p");
      empty.className = "empty-note";
      empty.textContent = "No Markdown notes were found in this folder.";
      host.append(empty);
      return;
    }
    for (const node of tree.top) renderNoteNode(node, host);
    if (tree.cut || tree.unreadable) {
      const note = document.createElement("p");
      note.className = "search-summary";
      note.textContent = (tree.cut ? "Folder scan reached the safety limit. " : "") +
        (tree.unreadable ? tree.unreadable + " folders or files could not be read." : "");
      host.append(note);
    }
  }

  function renderNoteNode(node, host) {
    const wrapper = document.createElement("div");
    wrapper.className = node.folder ? "note-folder" : "note-file";
    const row = document.createElement("div");
    row.className = "note-tree-row";
    const main = document.createElement("button");
    main.className = "note-node-main";
    main.textContent = (node.folder ? "▸" : "M") + "  " + node.name;
    main.title = node.path;
    if (node.folder) {
      const children = document.createElement("div");
      children.className = "note-children hidden";
      main.addEventListener("click", () => {
        const opening = children.classList.contains("hidden");
        children.classList.toggle("hidden", !opening);
        main.textContent = (opening ? "▾" : "▸") + "  " + node.name;
      });
      for (const child of node.children || []) renderNoteNode(child, children);
      row.append(main);
      wrapper.append(row, children);
    } else {
      main.addEventListener("click", () => openNotePath(node.path));
      const rename = document.createElement("button");
      rename.className = "note-rename";
      rename.textContent = "⋯";
      rename.title = "Rename note and rewrite links";
      rename.addEventListener("click", () => renameNote(node.path, node.name));
      row.append(main, rename);
      wrapper.append(row);
    }
    host.append(wrapper);
  }

  async function openNotePath(path) {
    if (!askDiscardChanges()) return;
    try {
      const opened = await invoke("read_markdown_file", { path });
      setDocument(opened);
      state.rootFolder = opened.path.replace(/[\\/][^\\/]+$/, "");
      await clearRecoverySnapshot();
      toast("Opened " + opened.name);
      showGitChanges();
    } catch (error) {
      toast("Open note failed: " + String(error));
    }
  }

  async function renameNote(path, currentName) {
    if (state.dirty) {
      toast("Save the open note before renaming another note or rewriting its links.");
      return;
    }
    const defaultName = currentName.replace(/\.(md|markdown|mdown|mkd)$/i, "");
    const newName = window.prompt("New note name. Links to this note will be updated.", defaultName);
    if (!newName || newName.trim() === defaultName) return;
    const openPath = state.path;
    try {
      const result = await invoke("rename_note", { path, newName });
      if (openPath) {
        const refreshedPath = openPath === path ? result.newPath : openPath;
        const refreshed = await invoke("read_markdown_file", { path: refreshedPath });
        setDocument(refreshed);
      }
      await loadNotes(state.rootFolder);
      toast("Renamed note and updated " + result.rewrittenFiles + " linked files.");
    } catch (error) {
      toast("Rename failed: " + String(error));
    }
  }

  async function searchNotes() {
    if (!state.rootFolder) {
      const root = await chooseNotesFolder();
      if (!root) return;
    }
    const query = $("#note-search-query").value.trim();
    const summary = $("#note-search-summary");
    const results = $("#note-search-results");
    results.replaceChildren();
    if (!query) {
      summary.textContent = "Enter text, a tag, or a note path to search.";
      return;
    }
    summary.textContent = "Searching Markdown notes…";
    try {
      const found = await invoke("search_notes", {
        root: state.rootFolder,
        query,
        mode: $("#note-search-mode").value,
      });
      const hitCount = found.files.reduce((total, file) => total + file.hits.length, 0);
      summary.textContent = found.files.length + " files · " + hitCount + " matches" +
        (found.unreadable ? " · " + found.unreadable + " unreadable" : "");
      if (!found.files.length) {
        const empty = document.createElement("p");
        empty.className = "empty-note";
        empty.textContent = "No matches.";
        results.append(empty);
      }
      for (const file of found.files) {
        const card = document.createElement("section");
        card.className = "search-file";
        const title = document.createElement("button");
        title.className = "search-file-title";
        title.textContent = file.path.split(/[\\/]/).pop() || file.path;
        title.title = file.path;
        title.addEventListener("click", () => openSearchHit(file.path, file.hits[0]));
        card.append(title);
        for (const hit of file.hits.slice(0, 20)) {
          const item = document.createElement("button");
          item.className = "search-hit";
          item.textContent = "L" + hit.line + ": " + hit.context;
          item.title = "Open this match";
          item.addEventListener("click", () => openSearchHit(file.path, hit));
          card.append(item);
        }
        if (file.more) {
          const more = document.createElement("p");
          more.className = "search-summary";
          more.textContent = "More matches in this file were omitted.";
          card.append(more);
        }
        results.append(card);
      }
    } catch (error) {
      summary.textContent = "Search failed: " + String(error);
    }
  }

  async function openSearchHit(path, hit) {
    await openNotePath(path);
    if (!hit) return;
    const lines = editor.value.split("\n");
    let offset = 0;
    for (let i = 0; i < Math.max(0, hit.line - 1); i++) offset += (lines[i] || "").length + 1;
    const lineText = lines[Math.max(0, hit.line - 1)] || "";
    let byteColumn = 0;
    let utf16Column = 0;
    for (const character of lineText) {
      if (byteColumn >= hit.column) break;
      const width = new TextEncoder().encode(character).length;
      if (byteColumn + width > hit.column) break;
      byteColumn += width;
      utf16Column += character.length;
    }
    const position = Math.min(editor.value.length, offset + utf16Column);
    editor.focus();
    editor.setSelectionRange(position, position);
    updateStatus();
  }

  function showPanel(name, title) {
    $("#sidebar-title").textContent = title;
    $("#outline-list").classList.toggle("hidden", name !== "outline");
    $("#git-panel").classList.toggle("hidden", name !== "git");
    $("#notes-panel").classList.toggle("hidden", name !== "notes");
    $("#search-panel").classList.toggle("hidden", name !== "search");
    document.querySelectorAll(".rail-button").forEach((button) => {
      button.classList.toggle("active", button.id === name + "-tab");
    });
  }

  async function showNotes() {
    showPanel("notes", "FOLDER NOTES");
    if (!state.rootFolder && state.path) state.rootFolder = state.path.replace(/[\\/][^\\/]+$/, "");
    try { await loadNotes(); }
    catch (error) { toast("Cannot scan notes: " + String(error)); }
  }

  async function showSearch() {
    showPanel("search", "SEARCH NOTES");
    if (!state.rootFolder && state.path) state.rootFolder = state.path.replace(/[\\/][^\\/]+$/, "");
    if (!state.rootFolder) await chooseNotesFolder();
  }


  async function showGitChanges() {
    const panel = $("#git-panel");
    const list = $("#git-change-list");
    const summary = $("#git-summary");
    showPanel("git", "GIT CHANGES");
    panel.classList.remove("hidden");
    list.replaceChildren();
    if (!state.path || !invoke) {
      summary.textContent = "Save or open a file inside a Git repository to inspect its changes.";
      return;
    }
    summary.textContent = "Comparing this file with the last commit…";
    try {
      const changes = await invoke("git_changes", { path: state.path });
      const added = changes.filter((item) => item.kind === "added").length;
      const modified = changes.filter((item) => item.kind === "modified").length;
      const removed = changes.filter((item) => item.kind === "removed").length;
      summary.textContent = changes.length
        ? `${added} added · ${modified} modified · ${removed} removed`
        : "No committed changes found, or this file is not tracked by Git.";
      for (const change of changes) {
        const card = document.createElement("div");
        card.className = `git-change ${change.kind}`;
        const heading = document.createElement("strong");
        heading.textContent = `${change.kind.toUpperCase()} · line ${change.startLine}${change.endLine > change.startLine ? `–${change.endLine}` : ""}`;
        const snippet = document.createElement("span");
        snippet.textContent = change.text.trim().slice(0, 180) || "Removed lines";
        card.append(heading, snippet);
        list.append(card);
      }
    } catch (error) {
      summary.textContent = `Git comparison failed: ${String(error)}`;
    }
  }

  function showOutline() {
    showPanel("outline", "OUTLINE");
    updateOutline();
  }

  function wrapSelection(marker) {
    const start = editor.selectionStart;
    const end = editor.selectionEnd;
    const selected = editor.value.slice(start, end);
    const replacement = `${marker}${selected}${marker}`;
    editor.setRangeText(replacement, start, end, "select");
    editor.focus();
    documentChanged();
  }

  function prefixCurrentLine(prefix) {
    const start = editor.selectionStart;
    const lineStart = editor.value.lastIndexOf("\n", Math.max(0, start - 1)) + 1;
    editor.setRangeText(prefix, lineStart, lineStart, "end");
    editor.focus();
    documentChanged();
  }

  function insertLink(image = false) {
    const address = window.prompt(image ? "Image URL or relative path" : "Link URL");
    if (!address) return;
    const start = editor.selectionStart;
    const end = editor.selectionEnd;
    const selected = editor.value.slice(start, end) || (image ? "image description" : "link text");
    const inserted = image ? `![${selected}](${address})` : `[${selected}](${address})`;
    editor.setRangeText(inserted, start, end, "end");
    editor.focus();
    documentChanged();
  }

  function setView(view) {
    state.view = view;
    stage.dataset.view = view;
    document.querySelectorAll(".view-button").forEach((button) => {
      button.classList.toggle("active", button.dataset.view === view);
    });
  }

  editor.addEventListener("input", documentChanged);
  editor.addEventListener("keyup", updateStatus);
  editor.addEventListener("click", updateStatus);
  editor.addEventListener("scroll", () => {
    highlightLayer.scrollTop = editor.scrollTop;
    highlightLayer.scrollLeft = editor.scrollLeft;
  });
  editor.addEventListener("paste", (event) => {
    const html = event.clipboardData?.getData("text/html");
    if (!html || !invoke) return;
    const plain = event.clipboardData?.getData("text/plain") || "";
    event.preventDefault();
    invoke("paste_html_as_markdown", { html }).then((markdown) => {
      editor.setRangeText(markdown, editor.selectionStart, editor.selectionEnd, "end");
      documentChanged();
    }).catch(() => {
      editor.setRangeText(plain, editor.selectionStart, editor.selectionEnd, "end");
      documentChanged();
    });
  });

  $("#new-file").addEventListener("click", newDocument);
  $("#open-file").addEventListener("click", openFile);
  $("#save-file").addEventListener("click", () => saveFile(false));
  $("#more-actions").addEventListener("click", () => saveFile(true));
  $("#export-html").addEventListener("click", exportHtml);
  $("#export-word").addEventListener("click", exportWord);
  $("#export-pdf").addEventListener("click", exportPdf);
  $("#notes-tab").addEventListener("click", showNotes);
  $("#search-tab").addEventListener("click", showSearch);
  $("#choose-notes-folder").addEventListener("click", async () => { const root = await chooseNotesFolder(); if (root) await loadNotes(root); });
  $("#note-search-button").addEventListener("click", searchNotes);
  $("#note-search-query").addEventListener("keydown", (event) => { if (event.key === "Enter") searchNotes(); });
  previewFrame.addEventListener("load", () => {
    state.previewReady = true;
    renderPreview().catch((error) => toast("Preview failed: " + String(error)));
  });
  window.addEventListener("message", onPreviewMessage);
  $("#outline-tab").addEventListener("click", showOutline);
  $("#git-tab").addEventListener("click", showGitChanges);
  $("#help-tab").addEventListener("click", () => toast("Splat for Windows · Early preview · Local-first Markdown editing"));
  $("#sidebar-collapse").addEventListener("click", () => $("#sidebar").classList.toggle("collapsed"));
  document.querySelectorAll(".view-button").forEach((button) => button.addEventListener("click", () => setView(button.dataset.view)));
  document.querySelectorAll("[data-wrap]").forEach((button) => button.addEventListener("click", () => wrapSelection(button.dataset.wrap)));
  document.querySelectorAll("[data-prefix]").forEach((button) => button.addEventListener("click", () => prefixCurrentLine(button.dataset.prefix)));
  $("#insert-link").addEventListener("click", () => insertLink(false));
  $("#insert-image").addEventListener("click", () => insertLink(true));
  $("#insert-rule").addEventListener("click", () => prefixCurrentLine("---\n"));

  window.addEventListener("keydown", (event) => {
    if (!(event.ctrlKey || event.metaKey)) return;
    const key = event.key.toLowerCase();
    if (key === "s") { event.preventDefault(); saveFile(event.shiftKey); }
    else if (key === "o") { event.preventDefault(); openFile(); }
    else if (key === "n") { event.preventDefault(); newDocument(); }
    else if (key === "b") { event.preventDefault(); wrapSelection("**"); }
    else if (key === "i") { event.preventDefault(); wrapSelection("*"); }
  });

  if (!ensureDesktopBridge()) {
    $("#status-message").textContent = "Preview UI · Tauri bridge unavailable";
  }
  editor.value = state.content;
  state.savedContent = state.content;
  updateDocumentChrome();
  updateHighlight();
  updateOutline();
  updateStatus();
  if (invoke) {
    invoke("app_version").then((version) => { $("#version-label").textContent = "Splat " + version; }).catch(() => {});
    openStartupFile().then((opened) => { if (!opened) restoreRecoverySnapshot(); });
  }
  if (window.__TAURI__?.event?.listen) {
    window.__TAURI__.event.listen("splat-menu", (event) => handleNativeMenu(String(event.payload || "")))
      .catch((error) => console.warn("Could not listen for native menu events:", error));
  }
})();
