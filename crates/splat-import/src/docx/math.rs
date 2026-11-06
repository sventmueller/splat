//! Word's formulas (OMML) as LaTeX.

use roxmltree::Node;

use super::package::{M, W, named};

/// How deep a formula is read before the rest of it is only its text; each level costs stack.
const MOST_DEPTH: usize = 64;

/// A formula another format holds as OMML, as LaTeX, and whether it stands on lines of its own. `fragment` is a `m:oMath`
/// or `m:oMathPara` with the prefix `m` and no declaration of it.
pub(crate) fn from_omml(fragment: &str) -> Option<(String, bool)> {
    let xml = format!("<m:math xmlns:m=\"{M}\">{fragment}</m:math>");
    let document = crate::xml::parse(&xml).ok()?;
    let root = document.root_element();
    let formula = root.children().find(|child| child.is_element())?;
    let display = named(formula, M, "oMathPara");
    let tex = if display { lines(formula, 0) } else { to_latex(root, 0) };
    (!tex.is_empty()).then_some((tex, display))
}

/// The formulas of a `m:oMathPara`, one line each.
pub(super) fn lines(paragraph: Node, depth: usize) -> String {
    let parts: Vec<String> = paragraph
        .children()
        .filter(|child| named(*child, M, "oMath"))
        .map(|formula| to_latex(formula, depth))
        .collect();
    parts.join(" \\\\ ")
}

/// The formula in a `m:oMath` as LaTeX, starting at nesting `depth`.
pub(super) fn to_latex(formula: Node, depth: usize) -> String {
    let mut out = String::new();
    children(formula, depth, &mut out);
    out.trim().to_string()
}

fn children(parent: Node, depth: usize, out: &mut String) {
    for child in parent.children().filter(|child| child.is_element()) {
        element(child, depth + 1, out);
    }
}

fn part<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|child| named(*child, M, name))
}

/// What `name` holds, in braces, empty if it is missing.
fn braced(node: Node, name: &str, depth: usize, out: &mut String) {
    out.push('{');
    if let Some(found) = part(node, name) {
        children(found, depth, out);
    }
    out.push('}');
}

/// `value` of the property `name` in `node`'s properties `properties`.
fn property<'a>(node: Node<'a, '_>, properties: &str, name: &str) -> Option<&'a str> {
    part(part(node, properties)?, name)?.attribute((M, "val"))
}

/// Whether a switch in `node`'s properties is on; a bare element is.
fn is_switched_on(node: Node, properties: &str, name: &str) -> bool {
    part(node, properties)
        .and_then(|found| part(found, name))
        .is_some_and(|switch| !matches!(switch.attribute((M, "val")), Some("0" | "off" | "false")))
}

/// The text below `node`, at the depth where nothing else of it is read.
fn only_text(node: Node, out: &mut String) {
    for descendant in node
        .descendants()
        .filter(|descendant| descendant.tag_name().name() == "t")
    {
        write_text(descendant.text().unwrap_or(""), out);
    }
}

fn element(node: Node, depth: usize, out: &mut String) {
    if depth >= MOST_DEPTH {
        only_text(node, out);
        return;
    }
    let tag = node.tag_name();
    if tag.namespace() == Some(W) {
        match tag.name() {
            "del" | "moveFrom" | "rPr" => {}
            "t" => write_text(node.text().unwrap_or(""), out),
            _ => children(node, depth, out),
        }
        return;
    }
    if tag.namespace() != Some(M) {
        children(node, depth, out);
        return;
    }
    match tag.name() {
        "r" => run(node, out),
        "t" => write_text(node.text().unwrap_or(""), out),
        "f" => fraction(node, depth, out),
        "sSup" | "sSub" | "sSubSup" | "sPre" => script(node, tag.name(), depth, out),
        "rad" => radical(node, depth, out),
        "nary" => nary(node, depth, out),
        "d" => delimiters(node, depth, out),
        "func" => function(node, depth, out),
        "limLow" => limit(node, '_', "\\underset", depth, out),
        "limUpp" => limit(node, '^', "\\overset", depth, out),
        "acc" => accent(node, depth, out),
        "bar" => bar(node, depth, out),
        "m" => matrix(node, depth, out),
        "eqArr" => equations(node, depth, out),
        name if name.ends_with("Pr") => {}
        _ => children(node, depth, out),
    }
}

fn run(node: Node, out: &mut String) {
    let mut text = String::new();
    for child in node.children().filter(|child| child.tag_name().name() == "t") {
        text.push_str(child.text().unwrap_or(""));
    }
    if is_switched_on(node, "rPr", "nor") {
        out.push_str("\\text{");
        for c in text.chars() {
            write_special(c, out);
        }
        out.push('}');
    } else if property(node, "rPr", "sty") == Some("p") && text.len() > 1 && text.chars().all(char::is_alphabetic) {
        // Word sets a word in upright letters this way, as `sin` without the structure of a function.
        if FUNCTIONS.contains(&text.as_str()) {
            out.push('\\');
            out.push_str(&text);
            out.push(' ');
        } else {
            out.push_str("\\mathrm{");
            write_text(&text, out);
            out.push('}');
        }
    } else {
        write_text(&text, out);
    }
}

fn fraction(node: Node, depth: usize, out: &mut String) {
    match property(node, "fPr", "type") {
        Some("noBar") => {
            out.push('{');
            if let Some(found) = part(node, "num") {
                children(found, depth, out);
            }
            out.push_str(" \\atop ");
            if let Some(found) = part(node, "den") {
                children(found, depth, out);
            }
            out.push('}');
        }
        Some("lin") => {
            if let Some(found) = part(node, "num") {
                children(found, depth, out);
            }
            out.push('/');
            if let Some(found) = part(node, "den") {
                children(found, depth, out);
            }
        }
        _ => {
            out.push_str("\\frac");
            braced(node, "num", depth, out);
            braced(node, "den", depth, out);
        }
    }
}

fn script(node: Node, kind: &str, depth: usize, out: &mut String) {
    if kind == "sPre" {
        out.push_str("{}");
    } else {
        braced(node, "e", depth, out);
    }
    if kind != "sSup" {
        out.push('_');
        braced(node, "sub", depth, out);
    }
    if kind != "sSub" {
        out.push('^');
        braced(node, "sup", depth, out);
    }
    if kind == "sPre" {
        braced(node, "e", depth, out);
    }
}

fn radical(node: Node, depth: usize, out: &mut String) {
    out.push_str("\\sqrt");
    let degree = part(node, "deg")
        .filter(|found| !is_switched_on(node, "radPr", "degHide") && found.children().any(|child| child.is_element()));
    if let Some(degree) = degree {
        out.push('[');
        children(degree, depth, out);
        out.push(']');
    }
    braced(node, "e", depth, out);
}

const NARY_OPERATORS: [(&str, &str); 9] = [
    ("∑", "\\sum"),
    ("∏", "\\prod"),
    ("∐", "\\coprod"),
    ("∫", "\\int"),
    ("∬", "\\iint"),
    ("∭", "\\iiint"),
    ("∮", "\\oint"),
    ("⋃", "\\bigcup"),
    ("⋂", "\\bigcap"),
];

fn nary(node: Node, depth: usize, out: &mut String) {
    let operator = property(node, "naryPr", "chr").unwrap_or("∫");
    match NARY_OPERATORS.iter().find(|(chr, _)| *chr == operator) {
        Some((_, command)) => out.push_str(command),
        None => write_text(operator, out),
    }
    for (name, hide, mark) in [("sub", "subHide", '_'), ("sup", "supHide", '^')] {
        let shown = part(node, name)
            .filter(|found| !is_switched_on(node, "naryPr", hide) && found.children().any(|child| child.is_element()));
        if shown.is_some() {
            out.push(mark);
            braced(node, name, depth, out);
        }
    }
    braced(node, "e", depth, out);
}

fn delimiter(chr: &str, out: &mut String) {
    match chr {
        "" => out.push('.'),
        "{" => out.push_str("\\{"),
        "}" => out.push_str("\\}"),
        "‖" => out.push_str("\\|"),
        "⌈" => out.push_str("\\lceil "),
        "⌉" => out.push_str("\\rceil "),
        "⌊" => out.push_str("\\lfloor "),
        "⌋" => out.push_str("\\rfloor "),
        "⟨" | "〈" => out.push_str("\\langle "),
        "⟩" | "〉" => out.push_str("\\rangle "),
        other => out.push_str(other),
    }
}

fn delimiters(node: Node, depth: usize, out: &mut String) {
    out.push_str("\\left");
    delimiter(property(node, "dPr", "begChr").unwrap_or("("), out);
    let separator = property(node, "dPr", "sepChr").unwrap_or("|");
    let mut first = true;
    for inner in node.children().filter(|child| named(*child, M, "e")) {
        if !first {
            delimiter(separator, out);
        }
        first = false;
        children(inner, depth, out);
    }
    out.push_str("\\right");
    delimiter(property(node, "dPr", "endChr").unwrap_or(")"), out);
}

/// The word in `node`, if it holds nothing but runs of text.
fn plain_word(node: Node) -> Option<String> {
    let mut word = String::new();
    for child in node.children().filter(|child| child.is_element()) {
        let tag = child.tag_name().name();
        if tag.ends_with("Pr") {
            continue;
        }
        if !named(child, M, "r") {
            return None;
        }
        for text in child.children().filter(|inner| inner.tag_name().name() == "t") {
            word.push_str(text.text().unwrap_or(""));
        }
    }
    Some(word.trim().to_string())
}

const FUNCTIONS: [&str; 26] = [
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "coth", "log",
    "ln", "lg", "exp", "lim", "max", "min", "sup", "inf", "det", "gcd", "dim", "ker",
];

fn function(node: Node, depth: usize, out: &mut String) {
    if let Some(name) = part(node, "fName") {
        match plain_word(name) {
            Some(word) if FUNCTIONS.contains(&word.as_str()) => {
                out.push('\\');
                out.push_str(&word);
            }
            Some(word) => {
                out.push_str("\\operatorname{");
                write_text(&word, out);
                out.push('}');
            }
            None => children(name, depth, out),
        }
    }
    braced(node, "e", depth, out);
}

fn limit(node: Node, mark: char, stacked: &str, depth: usize, out: &mut String) {
    let word = part(node, "e").and_then(plain_word);
    match word.filter(|word| FUNCTIONS.contains(&word.as_str())) {
        Some(word) => {
            out.push('\\');
            out.push_str(&word);
            out.push(mark);
            braced(node, "lim", depth, out);
        }
        None => {
            out.push_str(stacked);
            braced(node, "lim", depth, out);
            braced(node, "e", depth, out);
        }
    }
}

fn accent(node: Node, depth: usize, out: &mut String) {
    let chr = property(node, "accPr", "chr").unwrap_or("\u{302}");
    out.push_str(match chr {
        "\u{303}" | "~" => "\\tilde",
        "\u{304}" | "\u{305}" | "¯" => "\\bar",
        "\u{20D7}" | "→" => "\\vec",
        "\u{307}" | "˙" => "\\dot",
        "\u{308}" | "¨" => "\\ddot",
        _ => "\\hat",
    });
    braced(node, "e", depth, out);
}

fn bar(node: Node, depth: usize, out: &mut String) {
    out.push_str(if property(node, "barPr", "pos") == Some("top") {
        "\\overline"
    } else {
        "\\underline"
    });
    braced(node, "e", depth, out);
}

fn matrix(node: Node, depth: usize, out: &mut String) {
    out.push_str("\\begin{matrix} ");
    let mut first_row = true;
    for row in node.children().filter(|child| named(*child, M, "mr")) {
        if !first_row {
            out.push_str(" \\\\ ");
        }
        first_row = false;
        let mut first_cell = true;
        for cell in row.children().filter(|child| named(*child, M, "e")) {
            if !first_cell {
                out.push_str(" & ");
            }
            first_cell = false;
            children(cell, depth, out);
        }
    }
    out.push_str(" \\end{matrix}");
}

fn equations(node: Node, depth: usize, out: &mut String) {
    let mut first = true;
    for equation in node.children().filter(|child| named(*child, M, "e")) {
        if !first {
            out.push_str(" \\\\ ");
        }
        first = false;
        children(equation, depth, out);
    }
}

/// Symbols as LaTeX commands; the space keeps a letter behind them from becoming part of the command's name.
const SYMBOLS: [(char, &str); 78] = [
    ('α', "\\alpha "),
    ('β', "\\beta "),
    ('γ', "\\gamma "),
    ('δ', "\\delta "),
    ('ε', "\\varepsilon "),
    ('ϵ', "\\epsilon "),
    ('ζ', "\\zeta "),
    ('η', "\\eta "),
    ('θ', "\\theta "),
    ('ϑ', "\\vartheta "),
    ('ι', "\\iota "),
    ('κ', "\\kappa "),
    ('λ', "\\lambda "),
    ('μ', "\\mu "),
    ('ν', "\\nu "),
    ('ξ', "\\xi "),
    ('π', "\\pi "),
    ('ϖ', "\\varpi "),
    ('ρ', "\\rho "),
    ('σ', "\\sigma "),
    ('ς', "\\varsigma "),
    ('τ', "\\tau "),
    ('υ', "\\upsilon "),
    ('φ', "\\varphi "),
    ('ϕ', "\\phi "),
    ('χ', "\\chi "),
    ('ψ', "\\psi "),
    ('ω', "\\omega "),
    ('Γ', "\\Gamma "),
    ('Δ', "\\Delta "),
    ('Θ', "\\Theta "),
    ('Λ', "\\Lambda "),
    ('Ξ', "\\Xi "),
    ('Π', "\\Pi "),
    ('Σ', "\\Sigma "),
    ('Υ', "\\Upsilon "),
    ('Φ', "\\Phi "),
    ('Ψ', "\\Psi "),
    ('Ω', "\\Omega "),
    ('≤', "\\leq "),
    ('≥', "\\geq "),
    ('≠', "\\neq "),
    ('≈', "\\approx "),
    ('≡', "\\equiv "),
    ('∼', "\\sim "),
    ('≃', "\\simeq "),
    ('≅', "\\cong "),
    ('∝', "\\propto "),
    ('∞', "\\infty "),
    ('→', "\\to "),
    ('←', "\\leftarrow "),
    ('↔', "\\leftrightarrow "),
    ('⇒', "\\Rightarrow "),
    ('⇐', "\\Leftarrow "),
    ('⇔', "\\Leftrightarrow "),
    ('↦', "\\mapsto "),
    ('×', "\\times "),
    ('÷', "\\div "),
    ('·', "\\cdot "),
    ('⋅', "\\cdot "),
    ('±', "\\pm "),
    ('∓', "\\mp "),
    ('∂', "\\partial "),
    ('∇', "\\nabla "),
    ('∈', "\\in "),
    ('∉', "\\notin "),
    ('⊂', "\\subset "),
    ('⊆', "\\subseteq "),
    ('⊃', "\\supset "),
    ('⊇', "\\supseteq "),
    ('∪', "\\cup "),
    ('∩', "\\cap "),
    ('∅', "\\emptyset "),
    ('∀', "\\forall "),
    ('∃', "\\exists "),
    ('¬', "\\neg "),
    ('∧', "\\wedge "),
    ('∨', "\\vee "),
];

/// A plain letter or digit for one of Unicode's mathematical letters, which LaTeX sets by itself.
fn math_letter(c: char) -> Option<char> {
    let code = u32::from(c);
    match code {
        0x1D400..=0x1D6A3 => {
            let at = (code - 0x1D400) % 52;
            char::from_u32(if at < 26 { 0x41 + at } else { 0x61 + at - 26 })
        }
        0x1D7CE..=0x1D7FF => char::from_u32(0x30 + (code - 0x1D7CE) % 10),
        0x210E => Some('h'),
        _ => None,
    }
}

/// What only a backslash keeps from meaning something.
fn write_special(c: char, out: &mut String) {
    match c {
        '{' | '}' | '%' | '#' | '&' | '_' | '$' => {
            out.push('\\');
            out.push(c);
        }
        '\\' => out.push_str("\\backslash "),
        c => out.push(c),
    }
}

fn write_text(text: &str, out: &mut String) {
    for c in text.chars() {
        if c.is_ascii() {
            write_special(c, out);
        } else if let Some((_, command)) = SYMBOLS.iter().find(|(symbol, _)| *symbol == c) {
            out.push_str(command);
        } else if c == '−' {
            out.push('-');
        } else {
            out.push(math_letter(c).unwrap_or(c));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latex(inner: &str) -> String {
        let xml = format!("<m:oMath xmlns:m=\"{M}\" xmlns:w=\"{W}\" xmlns:x=\"urn:x\">{inner}</m:oMath>");
        let document = roxmltree::Document::parse(&xml).unwrap();
        to_latex(document.root_element(), 0)
    }

    fn r(text: &str) -> String {
        format!("<m:r><m:t>{text}</m:t></m:r>")
    }

    #[test]
    fn a_run_is_its_text_with_commands_for_symbols() {
        assert_eq!(latex(&r("a+b")), "a+b");
        assert_eq!(latex(&r("α≤∞")), "\\alpha \\leq \\infty");
        assert_eq!(latex(&r("x_1 {y} %#&amp;$\\")), "x\\_1 \\{y\\} \\%\\#\\&\\$\\backslash");
        assert_eq!(latex(&r("𝑥𝐵𝒶ℎ𝟏−é")), "xBah1-é");
        assert_eq!(
            latex("<m:r><m:rPr><m:nor/></m:rPr><m:t>if x_1</m:t></m:r>"),
            "\\text{if x\\_1}"
        );
        assert_eq!(
            latex("<m:r><m:rPr><m:nor m:val=\"0\"/></m:rPr><m:t>α</m:t><w:t>β</w:t></m:r>"),
            "\\alpha \\beta"
        );
    }

    #[test]
    fn a_word_in_upright_letters_stays_one() {
        let upright = |text: &str| format!("<m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>{text}</m:t></m:r>");
        assert_eq!(latex(&format!("{}{}", upright("sin"), r("x"))), "\\sin x");
        assert_eq!(latex(&upright("rate")), "\\mathrm{rate}");
        assert_eq!(latex(&upright("x")), "x", "a single letter needs no command");
        assert_eq!(latex(&upright("a+b")), "a+b", "not a word");
    }

    #[test]
    fn fractions_come_in_their_three_forms() {
        let parts = format!("<m:num>{}</m:num><m:den>{}</m:den>", r("a"), r("b"));
        assert_eq!(latex(&format!("<m:f>{parts}</m:f>")), "\\frac{a}{b}");
        assert_eq!(
            latex(&format!("<m:f><m:fPr><m:type m:val=\"bar\"/></m:fPr>{parts}</m:f>")),
            "\\frac{a}{b}"
        );
        assert_eq!(
            latex(&format!("<m:f><m:fPr><m:type m:val=\"noBar\"/></m:fPr>{parts}</m:f>")),
            "{a \\atop b}"
        );
        assert_eq!(
            latex(&format!("<m:f><m:fPr><m:type m:val=\"lin\"/></m:fPr>{parts}</m:f>")),
            "a/b"
        );
        assert_eq!(latex("<m:f/>"), "\\frac{}{}");
        assert_eq!(
            latex("<m:f><m:fPr><m:type m:val=\"noBar\"/></m:fPr></m:f>"),
            "{ \\atop }"
        );
        assert_eq!(latex("<m:f><m:fPr><m:type m:val=\"lin\"/></m:fPr></m:f>"), "/");
    }

    #[test]
    fn scripts_stand_where_they_belong() {
        let (e, sub, sup) = (
            format!("<m:e>{}</m:e>", r("x")),
            format!("<m:sub>{}</m:sub>", r("i")),
            format!("<m:sup>{}</m:sup>", r("2")),
        );
        assert_eq!(latex(&format!("<m:sSup>{e}{sup}</m:sSup>")), "{x}^{2}");
        assert_eq!(latex(&format!("<m:sSub>{e}{sub}</m:sSub>")), "{x}_{i}");
        assert_eq!(latex(&format!("<m:sSubSup>{e}{sub}{sup}</m:sSubSup>")), "{x}_{i}^{2}");
        assert_eq!(latex(&format!("<m:sPre>{sub}{sup}{e}</m:sPre>")), "{}_{i}^{2}{x}");
    }

    #[test]
    fn a_root_shows_its_degree_unless_it_hides_it() {
        let inner = format!("<m:e>{}</m:e>", r("x"));
        assert_eq!(latex(&format!("<m:rad>{inner}</m:rad>")), "\\sqrt{x}");
        assert_eq!(
            latex(&format!("<m:rad><m:deg>{}</m:deg>{inner}</m:rad>", r("3"))),
            "\\sqrt[3]{x}"
        );
        assert_eq!(
            latex(&format!(
                "<m:rad><m:radPr><m:degHide/></m:radPr><m:deg>{}</m:deg>{inner}</m:rad>",
                r("3")
            )),
            "\\sqrt{x}"
        );
        assert_eq!(latex(&format!("<m:rad><m:deg/>{inner}</m:rad>")), "\\sqrt{x}");
    }

    #[test]
    fn big_operators_take_their_limits() {
        let body = format!(
            "<m:sub>{}</m:sub><m:sup>{}</m:sup><m:e>{}</m:e>",
            r("i"),
            r("n"),
            r("x")
        );
        assert_eq!(
            latex(&format!(
                "<m:nary><m:naryPr><m:chr m:val=\"∑\"/></m:naryPr>{body}</m:nary>"
            )),
            "\\sum_{i}^{n}{x}"
        );
        assert_eq!(latex(&format!("<m:nary>{body}</m:nary>")), "\\int_{i}^{n}{x}");
        assert_eq!(
            latex(&format!(
                "<m:nary><m:naryPr><m:chr m:val=\"⋃\"/><m:subHide/><m:supHide m:val=\"on\"/></m:naryPr>{body}</m:nary>"
            )),
            "\\bigcup{x}"
        );
        assert_eq!(
            latex(&format!(
                "<m:nary><m:naryPr><m:chr m:val=\"★\"/></m:naryPr><m:sub/><m:e>{}</m:e></m:nary>",
                r("x")
            )),
            "★{x}"
        );
    }

    #[test]
    fn delimiters_take_what_they_are_given() {
        let two = format!("<m:e>{}</m:e><m:e>{}</m:e>", r("a"), r("b"));
        assert_eq!(latex(&format!("<m:d>{two}</m:d>")), "\\left(a|b\\right)");
        assert_eq!(
            latex(&format!(
                "<m:d><m:dPr><m:begChr m:val=\"{{\"/><m:sepChr m:val=\",\"/><m:endChr m:val=\"\"/></m:dPr>{two}</m:d>"
            )),
            "\\left\\{a,b\\right."
        );
        for (chr, wanted) in [
            ("}", "\\}"),
            ("‖", "\\|"),
            ("⌈", "\\lceil "),
            ("⌉", "\\rceil "),
            ("⌊", "\\lfloor "),
            ("⌋", "\\rfloor "),
            ("⟨", "\\langle "),
            ("⟩", "\\rangle "),
            ("[", "["),
        ] {
            let tex = latex(&format!(
                "<m:d><m:dPr><m:begChr m:val=\"{chr}\"/></m:dPr><m:e>{}</m:e></m:d>",
                r("a")
            ));
            assert_eq!(tex, format!("\\left{wanted}a\\right)"), "{chr}");
        }
    }

    #[test]
    fn functions_are_known_by_name() {
        let call = |name: &str| {
            latex(&format!(
                "<m:func><m:funcPr/><m:fName><m:ctrlPr/><m:r><m:t>{name}</m:t></m:r></m:fName><m:e>{}</m:e></m:func>",
                r("x")
            ))
        };
        assert_eq!(call("sin"), "\\sin{x}");
        assert_eq!(call("Foo"), "\\operatorname{Foo}{x}");
        let limit_name = format!(
            "<m:func><m:fName><m:limLow><m:e>{}</m:e><m:lim>{}</m:lim></m:limLow></m:fName><m:e>{}</m:e></m:func>",
            r("lim"),
            r("x→0"),
            r("f")
        );
        assert_eq!(latex(&limit_name), "\\lim_{x\\to 0}{f}");
    }

    #[test]
    fn limits_stack_unless_they_belong_to_an_operator() {
        let (lim, x) = (format!("<m:lim>{}</m:lim>", r("n")), format!("<m:e>{}</m:e>", r("x")));
        assert_eq!(latex(&format!("<m:limLow>{x}{lim}</m:limLow>")), "\\underset{n}{x}");
        assert_eq!(latex(&format!("<m:limUpp>{x}{lim}</m:limUpp>")), "\\overset{n}{x}");
        assert_eq!(
            latex(&format!("<m:limUpp><m:e>{}</m:e>{lim}</m:limUpp>", r("max"))),
            "\\max^{n}"
        );
        assert_eq!(
            latex(&format!("<m:limLow><m:e><m:f/></m:e>{lim}</m:limLow>")),
            "\\underset{n}{\\frac{}{}}"
        );
    }

    #[test]
    fn accents_and_bars_read_their_character() {
        let accent_of = |chr: &str| {
            latex(&format!(
                "<m:acc><m:accPr><m:chr m:val=\"{chr}\"/></m:accPr><m:e>{}</m:e></m:acc>",
                r("x")
            ))
        };
        assert_eq!(latex(&format!("<m:acc><m:e>{}</m:e></m:acc>", r("x"))), "\\hat{x}");
        assert_eq!(accent_of("\u{303}"), "\\tilde{x}");
        assert_eq!(accent_of("\u{305}"), "\\bar{x}");
        assert_eq!(accent_of("\u{20D7}"), "\\vec{x}");
        assert_eq!(accent_of("\u{307}"), "\\dot{x}");
        assert_eq!(accent_of("\u{308}"), "\\ddot{x}");
        assert_eq!(accent_of("?"), "\\hat{x}");
        assert_eq!(
            latex(&format!(
                "<m:bar><m:barPr><m:pos m:val=\"top\"/></m:barPr><m:e>{}</m:e></m:bar>",
                r("x")
            )),
            "\\overline{x}"
        );
        assert_eq!(
            latex(&format!("<m:bar><m:e>{}</m:e></m:bar>", r("x"))),
            "\\underline{x}"
        );
    }

    #[test]
    fn a_matrix_keeps_its_rows_and_cells() {
        let row = |a: &str, b: &str| format!("<m:mr><m:e>{}</m:e><m:e>{}</m:e></m:mr>", r(a), r(b));
        assert_eq!(
            latex(&format!("<m:m><m:mPr/>{}{}</m:m>", row("a", "b"), row("c", "d"))),
            "\\begin{matrix} a & b \\\\ c & d \\end{matrix}"
        );
    }

    #[test]
    fn what_only_holds_something_is_what_it_holds() {
        let x = format!("<m:e>{}</m:e>", r("x"));
        for wrapper in ["box", "borderBox", "groupChr", "phant"] {
            assert_eq!(latex(&format!("<m:{wrapper}>{x}</m:{wrapper}>")), "x", "{wrapper}");
        }
        assert_eq!(
            latex(&format!("<m:eqArr>{x}<m:e>{}</m:e></m:eqArr>", r("y"))),
            "x \\\\ y"
        );
        assert_eq!(latex(&format!("<x:unknown>{}</x:unknown>", r("z"))), "z");
        assert_eq!(latex(&format!("<m:unknown>{}</m:unknown>", r("z"))), "z");
        assert_eq!(latex(&format!("<w:ins>{}</w:ins>", r("a"))), "a");
        assert_eq!(latex(&format!("<w:del>{}</w:del>", r("a"))), "");
        assert_eq!(latex("<w:t>α</w:t>"), "\\alpha");
        assert_eq!(latex("<m:t>α</m:t>"), "\\alpha");
    }

    #[test]
    fn nesting_beyond_the_limit_is_read_as_text() {
        let depth = 5000;
        let deep = format!("{}{}{}", "<m:e>".repeat(depth), r("x"), "</m:e>".repeat(depth));
        assert_eq!(latex(&deep), "x");
        let fractions = format!(
            "{}{}{}",
            "<m:f><m:num>".repeat(depth),
            r("y"),
            "</m:num></m:f>".repeat(depth)
        );
        let tex = latex(&fractions);
        assert!(
            tex.ends_with("}") && tex.contains('y') && tex.len() < 100_000,
            "{}",
            tex.len()
        );
    }
}
