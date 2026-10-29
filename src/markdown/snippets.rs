use std::ops::Range;

use crate::i18n::{n, t};

/// Spot to fill in a formula template; the Tab key moves between them.
pub const PLACEHOLDER: char = '□';

pub struct Snippet {
    pub label: &'static str,
    pub tip: &'static str,
    pub text: &'static str,
}

pub const FORMULA_BUTTONS: &[Snippet] = &[
    Snippet {
        label: "a⁄b",
        tip: n("Bruch"),
        text: "\\frac{□}{□}",
    },
    Snippet {
        label: "√",
        tip: n("Wurzel"),
        text: "\\sqrt{□}",
    },
    Snippet {
        label: "xⁿ",
        tip: n("Hochgestellt, z. B. Exponent"),
        text: "^{□}",
    },
    Snippet {
        label: "xₙ",
        tip: n("Tiefgestellt, z. B. Index"),
        text: "_{□}",
    },
    Snippet {
        label: "∑",
        tip: n("Summe mit Grenzen"),
        text: "\\sum_{□}^{□}",
    },
    Snippet {
        label: "∫",
        tip: n("Integral mit Grenzen"),
        text: "\\int_{□}^{□}",
    },
    Snippet {
        label: "lim",
        tip: n("Grenzwert"),
        text: "\\lim_{□ \\to □}",
    },
    Snippet {
        label: "( )",
        tip: n("Klammern, die mit dem Inhalt wachsen"),
        text: "\\left( □ \\right)",
    },
    Snippet {
        label: n("Matrix"),
        tip: n("2×2-Matrix"),
        text: "\\begin{pmatrix} □ & □ \\\\ □ & □ \\end{pmatrix}",
    },
];

pub const GREEK: &[(&str, &str)] = &[
    ("α", "\\alpha "),
    ("β", "\\beta "),
    ("γ", "\\gamma "),
    ("δ", "\\delta "),
    ("ε", "\\varepsilon "),
    ("θ", "\\theta "),
    ("λ", "\\lambda "),
    ("μ", "\\mu "),
    ("π", "\\pi "),
    ("ρ", "\\rho "),
    ("σ", "\\sigma "),
    ("τ", "\\tau "),
    ("φ", "\\varphi "),
    ("ω", "\\omega "),
    ("Γ", "\\Gamma "),
    ("Δ", "\\Delta "),
    ("Θ", "\\Theta "),
    ("Λ", "\\Lambda "),
    ("Π", "\\Pi "),
    ("Σ", "\\Sigma "),
    ("Φ", "\\Phi "),
    ("Ω", "\\Omega "),
];

pub const RELATIONS: &[(&str, &str)] = &[
    ("≤", "\\le "),
    ("≥", "\\ge "),
    ("≠", "\\ne "),
    ("≈", "\\approx "),
    ("≡", "\\equiv "),
    ("∝", "\\propto "),
    ("±", "\\pm "),
    ("·", "\\cdot "),
    ("×", "\\times "),
    ("÷", "\\div "),
    ("→", "\\to "),
    ("⇒", "\\Rightarrow "),
    ("⇔", "\\Leftrightarrow "),
    ("∞", "\\infty "),
    ("∈", "\\in "),
    ("∉", "\\notin "),
    ("⊂", "\\subset "),
    ("∪", "\\cup "),
    ("∩", "\\cap "),
    ("∂", "\\partial "),
    ("∇", "\\nabla "),
    ("∀", "\\forall "),
    ("∃", "\\exists "),
];

/// Text to insert for a formula template; a selection fills the first placeholder. Also returns the first
/// placeholder left to fill.
pub fn formula_insert(template: &str, selected: &str) -> (String, Option<Range<usize>>) {
    let text = if selected.is_empty() {
        template.to_string()
    } else {
        template.replacen(PLACEHOLDER, selected, 1)
    };
    let next = text.find(PLACEHOLDER).map(|i| i..i + PLACEHOLDER.len_utf8());
    (text, next)
}

/// MathLive's template syntax: `#@` takes the selection, `#?` is an empty placeholder.
pub fn mathlive_template(template: &str) -> String {
    template.replacen(PLACEHOLDER, "#@", 1).replace(PLACEHOLDER, "#?")
}

/// The next placeholder after (or before) the caret, wrapping around.
pub fn next_placeholder(text: &str, caret: usize, backwards: bool) -> Option<Range<usize>> {
    let all: Vec<usize> = text.match_indices(PLACEHOLDER).map(|(i, _)| i).collect();
    let pick = if backwards {
        all.iter().rev().find(|&&i| i < caret.saturating_sub(1)).or(all.last())
    } else {
        all.iter().find(|&&i| i >= caret).or(all.first())
    };
    pick.map(|&i| i..i + PLACEHOLDER.len_utf8())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramKind {
    Flowchart,
    Sequence,
    Gantt,
    Other,
}

pub fn diagram_kind(code: &str) -> DiagramKind {
    let first = code
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .unwrap_or("");
    let word = first.split_whitespace().next().unwrap_or("");
    match word {
        "flowchart" | "graph" => DiagramKind::Flowchart,
        "sequenceDiagram" => DiagramKind::Sequence,
        "gantt" => DiagramKind::Gantt,
        _ => DiagramKind::Other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rectangle,
    Rounded,
    Diamond,
    Circle,
    Database,
}

impl Shape {
    pub const ALL: [(Shape, &'static str); 5] = [
        (Shape::Rectangle, n("Rechteck")),
        (Shape::Rounded, n("Abgerundet")),
        (Shape::Diamond, n("Raute (Entscheidung)")),
        (Shape::Circle, n("Kreis")),
        (Shape::Database, n("Datenbank")),
    ];

    fn wrap(self, id: &str, label: &str) -> (String, Range<usize>) {
        let (open, close) = match self {
            Shape::Rectangle => ("[", "]"),
            Shape::Rounded => ("(", ")"),
            Shape::Diamond => ("{", "}"),
            Shape::Circle => ("((", "))"),
            Shape::Database => ("[(", ")]"),
        };
        let start = id.len() + open.len();
        (format!("{id}{open}{label}{close}"), start..start + label.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramAction {
    Node(Shape),
    Edge,
    LabeledEdge,
    Direction(&'static str),
    Group,
    Participant,
    Message,
    Reply,
    Note,
    Loop,
    Alternative,
    Section,
    Task,
    Milestone,
}

pub const DIRECTIONS: [(&str, &str); 4] = [
    ("TD", n("Oben nach unten")),
    ("LR", n("Links nach rechts")),
    ("BT", n("Unten nach oben")),
    ("RL", n("Rechts nach links")),
];

/// A change to the diagram code: lines inserted after the caret's line, or a replaced range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramEdit {
    pub range: Range<usize>,
    pub text: String,
    /// Part of `text` to select afterwards, usually the first label to rename.
    pub select: Range<usize>,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// Node ids of a flowchart in order of appearance: words directly followed by a shape or next to an arrow.
fn flow_nodes(code: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in code.lines().skip(1) {
        let t = line.trim();
        if t.starts_with("%%")
            || t.starts_with("subgraph")
            || t == "end"
            || t.starts_with("style")
            || t.starts_with("class")
        {
            continue;
        }
        let mut chars = t.char_indices().peekable();
        let mut depth = 0i32;
        while let Some((i, c)) = chars.next() {
            match c {
                '[' | '(' | '{' => depth += 1,
                ']' | ')' | '}' => depth -= 1,
                '|' if depth == 0 => {
                    for (_, d) in chars.by_ref() {
                        if d == '|' {
                            break;
                        }
                    }
                }
                _ if depth == 0
                    && is_ident_start(c)
                    && (i == 0 || !t[..i].ends_with(|p: char| p.is_alphanumeric())) =>
                {
                    let len = t[i..]
                        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                        .unwrap_or(t.len() - i);
                    let word = &t[i..i + len];
                    if !out.iter().any(|w| w == word) {
                        out.push(word.to_string());
                    }
                    for _ in 1..word.chars().count() {
                        chars.next();
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn fresh_id(taken: &[String]) -> String {
    ('A'..='Z')
        .map(String::from)
        .chain((1..).map(|n| format!("N{n}")))
        .find(|id| !taken.contains(id))
        .unwrap()
}

/// Participants of a sequence diagram in order of appearance.
fn participants(code: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    };
    for line in code.lines().skip(1) {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("participant ").or_else(|| t.strip_prefix("actor ")) {
            add(rest.split(" as ").next().unwrap_or(rest));
        } else if let Some(arrow) = t.find("->") {
            let from = &t[..arrow];
            let after = t[arrow..].trim_start_matches(['-', '>', 'x', ')', '+']);
            let to = after.split(':').next().unwrap_or("");
            add(from.trim_end_matches(['-', '+']));
            add(to);
        }
    }
    out
}

/// Last task id of a Gantt chart, from `:id, ...` task definitions.
fn last_task(code: &str) -> Option<String> {
    code.lines()
        .filter_map(|l| l.split_once(':').map(|(_, rest)| rest))
        .filter_map(|rest| {
            let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
            let id = match parts.as_slice() {
                [first, id, _, ..]
                    if *first == "milestone" || *first == "crit" || *first == "done" || *first == "active" =>
                {
                    *id
                }
                [id, _, ..] => *id,
                _ => return None,
            };
            (!id.is_empty() && !id.contains(' ') && !id.starts_with("after") && !id.chars().next()?.is_ascii_digit())
                .then(|| id.to_string())
        })
        .next_back()
}

fn count_tasks(code: &str, prefix: &str) -> usize {
    (1..).find(|n| !code.contains(&format!("{prefix}{n},"))).unwrap_or(1)
}

/// Code change for a toolbar action, inserted after the line holding the caret.
pub fn diagram_snippet(code: &str, caret: usize, action: DiagramAction, today: &str) -> DiagramEdit {
    if let DiagramAction::Direction(dir) = action {
        let header_end = code.find('\n').unwrap_or(code.len());
        let header = &code[..header_end];
        let word_end = header.find(char::is_whitespace).unwrap_or(header.len());
        let range = word_end..header_end;
        let text = format!(" {dir}");
        return DiagramEdit {
            select: 1..text.len(),
            range,
            text,
        };
    }
    let caret = caret.min(code.len());
    let line_end = code[caret..].find('\n').map_or(code.len(), |i| caret + i);
    let line_start = code[..line_end].rfind('\n').map_or(0, |i| i + 1);
    let caret_indent = code[line_start..line_end].len() - code[line_start..line_end].trim_start().len();
    let indent = " ".repeat(caret_indent.max(4));
    let (body, select): (String, Range<usize>) = match action {
        DiagramAction::Node(shape) => {
            let id = fresh_id(&flow_nodes(code));
            shape.wrap(&id, t("Text"))
        }
        DiagramAction::Edge | DiagramAction::LabeledEdge => {
            let nodes = flow_nodes(code);
            let from = nodes.last().cloned().unwrap_or_else(|| "A".to_string());
            let mut taken = nodes.clone();
            taken.push(from.clone());
            let id = fresh_id(&taken);
            let arrow = if action == DiagramAction::Edge {
                " --> ".to_string()
            } else {
                format!(" -->|{}| ", t("Text"))
            };
            let (node, label) = Shape::Rectangle.wrap(&id, t("Text"));
            let prefix = format!("{from}{arrow}");
            if action == DiagramAction::LabeledEdge {
                let start = from.len() + " -->|".len();
                (format!("{prefix}{node}"), start..start + t("Text").len())
            } else {
                (
                    format!("{prefix}{node}"),
                    prefix.len() + label.start..prefix.len() + label.end,
                )
            }
        }
        DiagramAction::Group => {
            let id = fresh_id(&flow_nodes(code));
            let group = t("Gruppe");
            let text = format!("subgraph {group}\n{indent}    {id}[{}]\n{indent}end", t("Text"));
            (text, "subgraph ".len().."subgraph ".len() + group.len())
        }
        DiagramAction::Participant => {
            let names = participants(code);
            let name = (1..)
                .map(|n| format!("{}{n}", t("Person")))
                .find(|n| !names.contains(n))
                .unwrap();
            let text = format!("participant {name}");
            (text, "participant ".len().."participant ".len() + name.len())
        }
        DiagramAction::Message | DiagramAction::Reply | DiagramAction::Note => {
            let names = participants(code);
            let a = names.first().cloned().unwrap_or_else(|| "A".to_string());
            let b = names.get(1).cloned().unwrap_or_else(|| "B".to_string());
            let (text, label) = match action {
                DiagramAction::Message => (format!("{a}->>{b}: {}", t("Nachricht")), t("Nachricht")),
                DiagramAction::Reply => (format!("{b}-->>{a}: {}", t("Antwort")), t("Antwort")),
                _ => (format!("Note right of {a}: {}", t("Notiz")), t("Notiz")),
            };
            let end = text.len();
            (text, end - label.len()..end)
        }
        DiagramAction::Loop | DiagramAction::Alternative => {
            let names = participants(code);
            let a = names.first().cloned().unwrap_or_else(|| "A".to_string());
            let b = names.get(1).cloned().unwrap_or_else(|| "B".to_string());
            let text = if action == DiagramAction::Loop {
                format!(
                    "loop {}\n{indent}    {a}->>{b}: {}\n{indent}end",
                    t("Wiederholung"),
                    t("Nachricht")
                )
            } else {
                format!(
                    "alt {}\n{indent}    {a}->>{b}: {}\n{indent}else {}\n{indent}    {a}->>{b}: {}\n{indent}end",
                    t("Fall A"),
                    t("Ja"),
                    t("Fall B"),
                    t("Nein")
                )
            };
            let first_word = text.find(' ').unwrap_or(0) + 1;
            let end = text.find('\n').unwrap_or(text.len());
            (text, first_word..end)
        }
        DiagramAction::Section => (
            format!("section {}", t("Abschnitt")),
            "section ".len().."section ".len() + t("Abschnitt").len(),
        ),
        DiagramAction::Task | DiagramAction::Milestone => {
            let milestone = action == DiagramAction::Milestone;
            let prefix = if milestone { "m" } else { "t" };
            let id = format!("{prefix}{}", count_tasks(code, prefix));
            let start = last_task(code).map_or_else(|| today.to_string(), |t| format!("after {t}"));
            let (label, text) = if milestone {
                let label = t("Meilenstein");
                (label, format!("{label} :milestone, {id}, {start}, 0d"))
            } else {
                let label = t("Aufgabe");
                (label, format!("{label} :{id}, {start}, 3d"))
            };
            (text, 0..label.len())
        }
        DiagramAction::Direction(_) => unreachable!(),
    };
    let lead = format!("\n{indent}");
    DiagramEdit {
        range: line_end..line_end,
        select: lead.len() + select.start..lead.len() + select.end,
        text: format!("{lead}{body}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(code: &str, e: &DiagramEdit) -> (String, String) {
        let mut s = code.to_string();
        s.replace_range(e.range.clone(), &e.text);
        let selected = e.text[e.select.clone()].to_string();
        (s, selected)
    }

    #[test]
    fn formula_templates_fill_the_first_placeholder_with_the_selection() {
        assert_eq!(formula_insert("\\frac{□}{□}", "x").0, "\\frac{x}{□}");
        let (t, next) = formula_insert("\\sqrt{□}", "");
        assert_eq!(&t[next.unwrap()], "□");
        assert_eq!(mathlive_template("\\frac{□}{□}"), "\\frac{#@}{#?}");
        let s = "a □ b □";
        assert_eq!(next_placeholder(s, 0, false), Some(2..5));
        assert_eq!(next_placeholder(s, 5, false), Some(8..11));
        assert_eq!(next_placeholder(s, 11, false), Some(2..5));
        assert_eq!(next_placeholder(s, 8, true), Some(2..5));
        assert_eq!(next_placeholder(s, 2, true), Some(8..11));
    }

    #[test]
    fn flowchart_snippets_continue_from_the_last_node() {
        let code = "flowchart TD\n    A[Start] --> B{Entscheidung}\n    B -->|Ja| C[Weiter]";
        assert_eq!(diagram_kind(code), DiagramKind::Flowchart);
        assert_eq!(flow_nodes(code), vec!["A", "B", "C"]);
        let (s, sel) = apply(code, &diagram_snippet(code, code.len(), DiagramAction::Edge, ""));
        assert!(s.ends_with("\n    C --> D[Text]"), "{s}");
        assert_eq!(sel, "Text");
        let (s, sel) = apply(code, &diagram_snippet(code, 0, DiagramAction::Node(Shape::Diamond), ""));
        assert!(s.starts_with("flowchart TD\n    D{Text}\n"), "{s}");
        assert_eq!(sel, "Text");
        let (s, _) = apply(code, &diagram_snippet(code, 3, DiagramAction::Direction("LR"), ""));
        assert!(s.starts_with("flowchart LR\n"));
    }

    #[test]
    fn sequence_and_gantt_snippets_use_existing_names() {
        let code = "sequenceDiagram\n    Alice->>Bob: Anfrage";
        let (s, sel) = apply(code, &diagram_snippet(code, code.len(), DiagramAction::Reply, ""));
        assert!(s.ends_with("\n    Bob-->>Alice: Antwort"), "{s}");
        assert_eq!(sel, "Antwort");
        let code = "gantt\n    dateFormat YYYY-MM-DD\n    Aufgabe A :a1, 2026-01-05, 10d";
        let (s, sel) = apply(
            code,
            &diagram_snippet(code, code.len(), DiagramAction::Task, "2026-10-28"),
        );
        assert!(s.ends_with("\n    Aufgabe :t1, after a1, 3d"), "{s}");
        assert_eq!(sel, "Aufgabe");
        let empty = "gantt\n    dateFormat YYYY-MM-DD";
        let (s, _) = apply(
            empty,
            &diagram_snippet(empty, empty.len(), DiagramAction::Milestone, "2026-10-28"),
        );
        assert!(s.ends_with("Meilenstein :milestone, m1, 2026-10-28, 0d"), "{s}");
    }
}
