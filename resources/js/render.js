// The libraries load with the first formula or diagram: together they are 17 MB of script, which documents
// without formulas or diagrams never need.
const loading = {};
function load(src) {
  loading[src] ??= new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = src;
    script.onload = resolve;
    script.onerror = () => reject(new Error("cannot load " + src));
    document.head.appendChild(script);
  });
  return loading[src];
}

async function renderMath(tex, display, color) {
  await load("js/tex-svg-full.js");
  await MathJax.startup.promise;
  const node = MathJax.tex2svg(tex, { display });
  const svg = node.querySelector("svg");
  const w = parseFloat(svg.getAttribute("width"));
  const h = parseFloat(svg.getAttribute("height"));
  const style = svg.getAttribute("style") || "";
  const match = style.match(/vertical-align:\s*(-?[\d.]+)ex/);
  const va = match ? parseFloat(match[1]) : 0;
  const error = node.querySelector("[data-mjx-error]");
  svg.setAttribute("width", w * 16);
  svg.setAttribute("height", h * 16);
  svg.setAttribute("color", color);
  svg.removeAttribute("style");
  return { svg: svg.outerHTML, w, h, va, error: error ? error.getAttribute("data-mjx-error") : "" };
}

let counter = 0;
async function renderMermaid(code, dark) {
  await load("js/mermaid.min.js");
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: dark ? "dark" : "default",
    fontFamily: "-apple-system, 'Helvetica Neue', sans-serif",
  });
  const out = document.getElementById("out");
  out.innerHTML = "";
  let svg;
  try {
    ({ svg } = await mermaid.render("splat" + ++counter, code));
  } catch (e) {
    document.getElementById("dsplat" + counter)?.remove();
    return { error: String(e && e.message ? e.message : e) };
  }
  out.innerHTML = svg;
  const el = out.querySelector("svg");
  const box = el.viewBox.baseVal;
  el.style.maxWidth = "none";
  el.setAttribute("width", box.width);
  el.setAttribute("height", box.height);
  const r = el.getBoundingClientRect();
  return { x: r.x, y: r.y, w: r.width, h: r.height };
}

async function renderDiagram(language, code, dark) {
  if (language === "graphviz") return renderGraphviz(code, dark);
  if (language === "plantuml") return renderPlantUml(code, dark);
  return renderMermaid(code, dark);
}

// Shows an SVG at the size its view box gives and reports where it stands, for the PDF of that rectangle.
function place(svgText) {
  const out = document.getElementById("out");
  out.innerHTML = svgText;
  const el = out.querySelector("svg");
  if (!el) return { error: "no picture" };
  const box = el.viewBox && el.viewBox.baseVal;
  if (box && box.width > 0 && box.height > 0) {
    el.setAttribute("width", box.width);
    el.setAttribute("height", box.height);
  }
  el.style.maxWidth = "none";
  el.style.width = "";
  el.style.height = "";
  const r = el.getBoundingClientRect();
  return { x: r.x, y: r.y, w: r.width, h: r.height };
}

async function renderGraphviz(code, dark) {
  await load("js/viz-global.js");
  const viz = await Viz.instance();
  const c = dark ? "#e8e8ed" : "#1d1d1f";
  const font = `fontname="Helvetica", fontcolor="${c}", color="${c}"`;
  // Defaults at the top of the graph; what the graph sets itself later wins.
  const defaults = `graph [${font}, bgcolor="transparent"]; node [${font}]; edge [${font}]; `;
  const at = code.indexOf("{");
  const source = at < 0 ? code : code.slice(0, at + 1) + defaults + code.slice(at + 1);
  try {
    return place(viz.renderString(source, { format: "svg" }));
  } catch (e) {
    document.getElementById("out").innerHTML = "";
    return { error: String(e && e.message ? e.message : e) };
  }
}

async function renderPlantUml(code, dark) {
  await load("js/viz-global.js");
  await load("js/plantuml.js");
  let lines = code.replace(/\n$/, "").split("\n");
  if (!lines.some((l) => l.trim().startsWith("@start"))) lines = ["@startuml", ...lines, "@enduml"];
  let svg;
  try {
    svg = await new Promise((resolve, reject) => PlantUML.renderToString(lines, resolve, reject, { dark }));
  } catch (e) {
    document.getElementById("out").innerHTML = "";
    return { error: String(e && e.message ? e.message : e) };
  }
  return place(svg);
}
