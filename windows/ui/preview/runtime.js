(() => {
  "use strict";
  const root = document.getElementById("out");
  const styleHost = document.head;
  let activeRender = 0;
  let editTimer = null;
  let editable = true;

  function post(message) {
    window.parent.postMessage({ source: "splat-preview", ...message }, "*");
  }

  function stripGeneratedFigure(node) {
    if (node.classList.contains("splat-math-figure")) {
      const tex = node.dataset.tex || node.textContent || "";
      return document.createTextNode(node.dataset.display === "true" ? `\n\n$$\n${tex}\n$$\n\n` : `$${tex}$`);
    }
    const pre = document.createElement("pre");
    const code = document.createElement("code");
    code.className = `language-${node.dataset.kind || "mermaid"}`;
    code.textContent = node.dataset.source || "";
    pre.append(code);
    return pre;
  }

  function htmlForEditing() {
    const clone = root.cloneNode(true);
    clone.querySelectorAll(".splat-math-figure,.splat-diagram-figure").forEach((node) => node.replaceWith(stripGeneratedFigure(node)));
    clone.querySelectorAll("[contenteditable]").forEach((node) => node.removeAttribute("contenteditable"));
    return clone.innerHTML;
  }

  function queueEdit() {
    if (!editable) return;
    clearTimeout(editTimer);
    editTimer = setTimeout(() => {
      post({ type:"splat:preview-edit", html:htmlForEditing(), renderId:activeRender });
    }, 700);
  }

  function addStyles(parsed) {
    styleHost.querySelectorAll("style[data-render-style]").forEach((node) => node.remove());
    parsed.querySelectorAll("head style").forEach((source) => {
      const style = document.createElement("style");
      style.dataset.renderStyle = "true";
      style.textContent = source.textContent || "";
      styleHost.append(style);
    });
  }

  function figureColor(dark) {
    return dark ? "#e8e8ed" : "#202632";
  }

  async function renderFigures(dark) {
    const errors = [];
    for (const node of root.querySelectorAll(".splat-math-figure")) {
      const tex = node.dataset.tex || node.textContent || "";
      try {
        const result = await renderMath(tex, node.dataset.display === "true", figureColor(dark), "");
        node.innerHTML = result.svg;
        if (result.error) errors.push({ key:node.dataset.figureKey, error:result.error });
      } catch(error) {
        node.textContent = tex;
        node.classList.add("splat-figure-error");
        errors.push({ key:node.dataset.figureKey, error:String(error) });
      }
    }

    for (const node of root.querySelectorAll(".splat-diagram-figure")) {
      const kind = node.dataset.kind || "mermaid";
      const source = node.dataset.source || "";
      try {
        const result = await renderDiagram(kind, source, dark);
        if (result && result.error) throw new Error(result.error);
        const work = document.getElementById("splat-diagram-work");
        const svg = work && work.querySelector("svg");
        if (!svg) throw new Error("Renderer returned no SVG image");
        node.replaceChildren(svg.cloneNode(true));
      } catch(error) {
        const message = document.createElement("div");
        message.className = "splat-figure-error";
        message.textContent = `${kind} rendering failed: ${String(error)}\n\n${source}`;
        node.replaceChildren(message);
        errors.push({ key:node.dataset.figureKey, error:String(error) });
      }
    }
    return errors;
  }

  async function render(message) {
    activeRender = message.id;
    editable = message.editable !== false;
    root.contentEditable = editable ? "true" : "false";
    root.replaceChildren();
    const parsed = new DOMParser().parseFromString(message.html || "", "text/html");
    document.title = parsed.title || "Splat Preview";
    addStyles(parsed);
    root.innerHTML = parsed.body ? parsed.body.innerHTML : String(message.html || "");
    root.querySelectorAll(".splat-math-figure,.splat-diagram-figure").forEach((node) => {
      node.contentEditable = "false";
      node.draggable = false;
    });
    const errors = await renderFigures(Boolean(message.dark));
    if (activeRender !== message.id) return;
    post({ type:"splat:rendered", id:message.id, errors });
  }

  function svgToPng(svg) {
    return new Promise((resolve, reject) => {
      const clone = svg.cloneNode(true);
      const box = clone.viewBox && clone.viewBox.baseVal;
      const width = Math.max(1, Math.ceil(box && box.width ? box.width : svg.getBoundingClientRect().width));
      const height = Math.max(1, Math.ceil(box && box.height ? box.height : svg.getBoundingClientRect().height));
      const scale = Math.min(3, Math.max(1, 1800 / Math.max(width, height)));
      clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
      clone.setAttribute("width", String(Math.ceil(width * scale)));
      clone.setAttribute("height", String(Math.ceil(height * scale)));
      const blob = new Blob([new XMLSerializer().serializeToString(clone)], { type:"image/svg+xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const image = new Image();
      image.onload = () => {
        try {
          const canvas = document.createElement("canvas");
          canvas.width = Math.ceil(width * scale);
          canvas.height = Math.ceil(height * scale);
          const context = canvas.getContext("2d");
          if (!context) throw new Error("Canvas is unavailable");
          context.fillStyle = "#ffffff";
          context.fillRect(0,0,canvas.width,canvas.height);
          context.drawImage(image,0,0,canvas.width,canvas.height);
          resolve({ data:canvas.toDataURL("image/png").split(",",2)[1], width:width * 0.75, height:height * 0.75 });
        } catch(error) { reject(error); }
        finally { URL.revokeObjectURL(url); }
      };
      image.onerror = () => { URL.revokeObjectURL(url); reject(new Error("Could not rasterize SVG figure")); };
      image.src = url;
    });
  }

  async function collectFigures() {
    const assets = [];
    for (const node of root.querySelectorAll(".splat-math-figure,.splat-diagram-figure")) {
      const key = node.dataset.figureKey;
      const svg = node.querySelector("svg");
      if (!key || !svg) continue;
      try { assets.push({ key, ...await svgToPng(svg) }); }
      catch(error) { /* Word export falls back to source text when rasterization is unavailable. */ }
    }
    return assets;
  }

  function exportHtml() {
    const clone = root.cloneNode(true);
    clone.removeAttribute("contenteditable");
    clone.querySelectorAll("[contenteditable]").forEach((node) => node.removeAttribute("contenteditable"));
    const css = [...document.querySelectorAll("style")].map((node) => node.textContent || "").join("\n");
    return { body:clone.innerHTML, css, title:document.title };
  }

  root.addEventListener("input", queueEdit);
  root.addEventListener("click", (event) => {
    const link = event.target.closest && event.target.closest("a[href]");
    if (!link) return;
    const href = link.getAttribute("href") || "";
    if (/^https?:\/\//i.test(href) || /^mailto:/i.test(href)) {
      event.preventDefault();
      post({ type:"splat:open-link", href });
    }
  });

  window.addEventListener("message", async (event) => {
    if (event.source !== window.parent || !event.data || typeof event.data.type !== "string") return;
    const message = event.data;
    if (message.type === "splat:render") {
      try { await render(message); }
      catch(error) { post({ type:"splat:rendered", id:message.id, error:String(error) }); }
    } else if (message.type === "splat:collect") {
      post({ type:"splat:collected", id:message.id, figures:await collectFigures(), ...exportHtml() });
    } else if (message.type === "splat:print") {
      window.focus();
      window.print();
      post({ type:"splat:printed", id:message.id });
    }
  });
})();
