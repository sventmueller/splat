//! Documents, pieces and checks the other modules share.

use splat_markdown::commands::Edit;
use splat_testkit::fuzz_checks::edit_fits;
use splat_testkit::{Rng, TimeLimit, rounds, time_limit};

/// Far beyond what the rounds take when nothing hangs.
pub(super) fn limited() -> TimeLimit {
    time_limit(
        std::time::Duration::from_secs(120)
            + std::time::Duration::from_millis(50) * u32::try_from(rounds(2000)).unwrap_or(u32::MAX),
    )
}

pub(super) const PIECES: &[&str] = &[
    "Wort",
    " ",
    "ä",
    "😀",
    "x",
    "**",
    "*",
    "_",
    "~~",
    "`",
    "$",
    "$$",
    "[",
    "](#a)",
    ")",
    "<u>",
    "</u>",
    "<mark>",
    "</mark>",
    "<sup>",
    "</sup>",
    "\n",
    "\n\n",
    "> ",
    "> [!NOTE]\n> ",
    "- ",
    "1. ",
    "# ",
    "## ",
    "- [ ] ",
    "\\",
    "&amp;",
    "[^1]",
    "\n[^1]: Fuß\n",
    "| a | b |\n|---|---|\n| 1 | 2 |\n",
    "```\n",
    "```python\n",
    "  ",
    "\t",
    "---\n",
    "<br>",
    "![b](x.png)",
    "==",
    "^",
    "~",
    "||",
    ":rocket:",
    ":",
    "%%",
    "^[",
    "]",
    "[[Seite",
    "|Titel]]",
    "![[bild.png]]",
    " {#id .k}",
    "\nBegriff\n\n: ",
    "> [!tip]- ",
    "$`",
    "`$",
    "\\(",
    "\\)",
    "```math\n",
    "```dot\n",
    "#tag",
    "@user",
    "#12",
    "[@doe]",
    "\"",
    "--",
    "...",
    "<details>\n<summary>S</summary>\n\n",
    "\n\n</details>\n",
    "<p align=\"center\">",
    "</p>",
    "<img src=\"x.png\" width=10>",
    "<b>",
    "</b>",
    "<a href=\"#a\">",
    "</a>",
    "<table><tr><td colspan=2>z</td></tr></table>",
    "<!-- c -->",
    "<kbd>",
    "</kbd>",
    "\r",
    "\r\n",
    "\u{1b}",
    "\u{200d}",
    "\u{feff}",
    "[a]: b\n",
    "[a](b\n\"c\") ",
    "[a]",
    "[SS]: http://a\n\n",
    "[ß]: http://b\n\n",
    "[ß]",
    "[ss]",
];

pub(super) fn document(rng: &mut Rng) -> String {
    (0..rng.below(40) + 1).map(|_| *rng.pick(PIECES)).collect()
}

pub(super) fn check(text: &str, edit: &Edit, what: &str) -> String {
    edit_fits(text, edit, what)
}

/// Ordinary documents: blocks in quotes, lists and tables with running text of every kind, where comrak's positions are
/// exact. In the wild mix of `document` they are not always, for example behind an inline footnote.
pub(super) const BLOCKS: [&str; 5] = [
    "```\nzeile eins\nzeile zwei\n```\n\n",
    "~~~rust\nlet x = 1;\n~~~\n\n",
    "> ```\n> im zitat\n> ```\n\n",
    "    eingerückt\n\n",
    "vor\n**fett** und\n![x](y.png)text\nnach\n\n",
];

pub(super) fn ordinary_document(rng: &mut Rng) -> String {
    const INLINE: [&str; 21] = [
        "Wort",
        "äöü",
        "😀",
        "**fett**",
        "*schräg*",
        "`code`",
        "[Link](#a)",
        "~~weg~~",
        "<u>unter</u>",
        "<mark>hell</mark>",
        "Caf&eacute;",
        "&rarr;",
        "&#233;",
        "a &amp; b",
        "\\*stern\\*",
        "\"Zitat\"",
        "'x'",
        "a -- b",
        "c --- d",
        "e...",
        "#tag",
    ];
    let line = |rng: &mut Rng| -> String {
        (0..rng.below(4) + 1)
            .map(|_| *rng.pick(&INLINE))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut text = String::new();
    for _ in 0..rng.below(4) + 1 {
        let (first, next) = match rng.below(7) {
            0 => ("> ", "> "),
            1 => ("- ", "  "),
            2 => ("1. ", "   "),
            3 => ("> - ", ">   "),
            4 => ("## ", ""),
            5 => ("", ""),
            _ => ("> > ", "> > "),
        };
        text.push_str(first);
        text.push_str(&line(rng));
        if first != "## " && rng.below(2) == 0 {
            text.push('\n');
            text.push_str(next);
            text.push_str(&line(rng));
        }
        text.push_str("\n\n");
        if rng.below(5) == 0 {
            text.push_str(&format!("| {} | b |\n|---|---|\n| {} | y |\n\n", line(rng), line(rng)));
        }
        match rng.below(8) {
            0 | 1 => text.push_str(rng.pick(&BLOCKS)),
            2 => text.push_str("- ```\n  in der liste\n  ```\n\n"),
            _ => {}
        }
    }
    text
}

/// Plain words and paragraphs, where formatting twice must give back the text it started from.
pub(super) fn plain_document(rng: &mut Rng) -> String {
    const WORDS: [&str; 6] = ["Wort", "ä", "zwei", "😀", "x", "Ende"];
    let mut text = String::new();
    for p in 0..rng.below(3) + 1 {
        if p > 0 {
            text.push_str("\n\n");
        }
        for w in 0..rng.below(5) + 1 {
            if w > 0 {
                text.push(' ');
            }
            text.push_str(WORDS[rng.below(WORDS.len())]);
        }
    }
    text
}
