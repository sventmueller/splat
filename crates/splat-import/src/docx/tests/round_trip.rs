//! What Splat's Word export writes comes back as the Markdown it was.

use super::*;

#[test]
fn what_splat_writes_comes_back_as_it_was() {
    for source in [
        "# Eins\n\n## Zwei\n\n###### Sechs\n",
        "Text mit **fett**, *kursiv*, ~~weg~~, <ins>unter</ins>, <mark>hell</mark>, <sup>hoch</sup>, <sub>tief</sub> und `code`.\n",
        "***beides*** und **fett *kursiv***\n",
        "- a\n- b\n  - c\n  - d\n- e\n",
        "1. a\n2. b\n\nText\n\n5. x\n6. y\n",
        "- [ ] offen\n- [x] fertig\n",
        "| A | B | C |\n|:---:|---:|---|\n| 1 | 2 | 3 |\n| `x` | **y** | z |\n",
        "[Web](https://a.bc/d) und [Post](mailto:a@b.cd)\n",
        "Text[^1] und mehr[^2]\n\n[^1]: Fuß\n\n[^2]: Noch *eine*\n",
        "![Bild](media/image1.png)\n",
        "> Zitat\n>\n> - Punkt\n\nDanach\n",
        "```\nfn a() {\n\n    b\n}\n```\n",
        "Vor\n\n---\n\nNach\n",
        "- a\n\n  weiter\n\n- b\n",
        "`code`\n",
        "Text[^1]\n\n---\n\n```\nx\n```\n\n[^1]: Fuß\n",
    ] {
        assert_eq!(round_trip(source), source, "{source}");
    }
}

#[test]
fn a_table_with_wide_cells_comes_back_with_them() {
    let source = "<table><tr><td colspan=\"2\">A</td></tr><tr><td>1</td><td>2</td></tr></table>\n";
    let back = round_trip(source);
    assert!(back.contains("<th colspan=\"2\">A</th>"), "{back}");
    assert!(back.contains("<td>1</td><td>2</td>"), "{back}");
}
