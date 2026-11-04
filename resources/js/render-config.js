// MathJax 4 keeps these TeX packages in separate files; in MathJax 3 its full bundle had them all active.
const TEX_PACKAGES = [
  "action", "amscd", "bbox", "begingroup", "boldsymbol", "braket", "bussproofs", "cancel", "cases", "centernot", "color", "colortbl",
  "empheq", "enclose", "extpfeil", "gensymb", "html", "mathtools", "mhchem", "noerrors", "tagformat", "textcomp",
  "unicode", "upgreek", "verb",
];

window.MathJax = {
  // The default for fonts is a CDN, which the page may not reach; a package that fails to load stalls every formula.
  loader: { paths: { fonts: "[mathjax]/fonts" }, load: TEX_PACKAGES.map((name) => `[tex]/${name}`) },
  // Autoload would turn an unknown macro into a request for a file that is not bundled.
  tex: { packages: { "[+]": [...TEX_PACKAGES, "textmacros"], "[-]": ["autoload"] } },
  // By default MathJax 4 splits a long inline formula into several pictures, and the app shows one per formula.
  svg: { font: "mathjax-tex", fontCache: "local", linebreaks: { inline: false } },
  startup: { typeset: false },
  // Enrichment would start a speech worker, which the page's security policy blocks, and every formula would wait for it.
  options: { enableMenu: false, menuOptions: { settings: { enrich: false } } },
};
