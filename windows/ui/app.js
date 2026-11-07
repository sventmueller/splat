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
    renderTimer = setTimeout(renderPreview, 110);
  }

  function documentChanged() {
    state.content = editor.value;
    state.dirty = state.content !== state.savedContent;
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
    renderPreview();
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
    toast("New document");
  }

  async function exportHtml() {
    if (!ensureDesktopBridge()) return;
    try {
      const path = await dialog.save({
        title: "Export as HTML",
        defaultPath: `${state.name.replace(/\.(md|markdown|mdown|mkd)$/i, "")}.html`,
        filters: [{ name: "HTML document", extensions: ["html", "htm"] }],
      });
      if (!path) return;
      const html = await invoke("render_markdown", {
        markdown: editor.value,
        title: state.name,
        allowRemoteImages: true,
      });
      const destination = /\.html?$/i.test(path) ? path : `${path}.html`;
      await invoke("save_html_file", { path: destination, html });
      toast("HTML exported");
    } catch (error) {
      toast(`Export failed: ${String(error)}`);
    }
  }

  async function showGitChanges() {
    const panel = $("#git-panel");
    const list = $("#git-change-list");
    const summary = $("#git-summary");
    $("#sidebar-title").textContent = "GIT CHANGES";
    $("#outline-list").classList.add("hidden");
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
    $("#sidebar-title").textContent = "OUTLINE";
    $("#git-panel").classList.add("hidden");
    $("#outline-list").classList.remove("hidden");
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
  $("#outline-tab").addEventListener("click", () => { showOutline(); $("#outline-tab").classList.add("active"); $("#git-tab").classList.remove("active"); });
  $("#git-tab").addEventListener("click", () => { showGitChanges(); $("#git-tab").classList.add("active"); $("#outline-tab").classList.remove("active"); });
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
  renderPreview();
  if (invoke) invoke("app_version").then((version) => { $("#version-label").textContent = `Splat ${version}`; }).catch(() => {});
})();
