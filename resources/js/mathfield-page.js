MathfieldElement.fontsDirectory = new URL("js/mathlive-fonts/", location.href).href;
MathfieldElement.soundsDirectory = null;
const mf = document.getElementById("mf");
mf.mathVirtualKeyboardPolicy = "manual";
let quiet = false;
const post = (message) => window.webkit.messageHandlers.splat.postMessage(message);
mf.addEventListener("input", () => { if (!quiet) post({ latex: mf.value }); });
mf.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    post({ cancel: true });
  }
}, { capture: true });
function setLatex(latex) {
  quiet = true;
  mf.value = latex;
  quiet = false;
  mf.focus();
}
function insertLatex(template) {
  mf.insert(template, { format: "latex", selectionMode: "placeholder" });
  mf.focus();
  post({ latex: mf.value });
}
