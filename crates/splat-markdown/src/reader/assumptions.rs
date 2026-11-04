//! What Splat relies on in comrak, each read from comrak itself with the options the reader uses. A test failing after an
//! upgrade names what changed; the code to look at next is the one that mentions it in a comment.

use comrak::nodes::{Node, NodeValue};
use comrak::{Arena, parse_document};

use super::options;

/// What `look` finds in the tree comrak makes of `text`.
fn read<T>(text: &str, look: impl for<'a> FnOnce(Node<'a>) -> T) -> T {
    let arena = Arena::new();
    let options = options();
    look(parse_document(&arena, text, &options))
}

/// The tree below the root, indented by depth: a text as it reads, anything else by kind, then its `line:column` range.
fn tree(text: &str) -> Vec<String> {
    read(text, |root| {
        root.descendants()
            .skip(1)
            .map(|node| {
                let data = node.data();
                let kind = match &data.value {
                    NodeValue::Text(t) => format!("{t:?}"),
                    other => {
                        let kind = format!("{other:?}");
                        kind.split(['(', ' ']).next().unwrap_or_default().to_string()
                    }
                };
                let depth = node.ancestors().count() - 2;
                format!("{}{kind} {}", "  ".repeat(depth), data.sourcepos)
            })
            .collect()
    })
}

/// The names of the footnote definitions at the top level, in the order of the tree.
fn definitions(text: &str) -> Vec<String> {
    read(text, |root| {
        root.children()
            .filter_map(|node| match &node.data().value {
                NodeValue::FootnoteDefinition(f) => Some(f.name.clone()),
                _ => None,
            })
            .collect()
    })
}

/// Name and number of every footnote reference.
fn references(text: &str) -> Vec<(String, u32)> {
    read(text, |root| {
        root.descendants()
            .filter_map(|node| match &node.data().value {
                NodeValue::FootnoteReference(f) => Some((f.name.clone(), f.ix)),
                _ => None,
            })
            .collect()
    })
}

/// The first node `pick` has an answer for.
fn first<T>(text: &str, pick: impl Fn(&NodeValue) -> Option<T>) -> T {
    read(text, |root| {
        root.descendants()
            .find_map(|node| pick(&node.data().value))
            .expect("no such node")
    })
}

#[test]
fn positions_count_lines_and_bytes_from_one() {
    assert_eq!(
        tree("äb **c**\n"),
        [
            "Paragraph 1:1-1:9",
            "  \"äb \" 1:1-1:4",
            "  Strong 1:5-1:9",
            "    \"c\" 1:7-1:7"
        ]
    );
}

#[test]
fn a_carriage_return_ends_a_line() {
    assert_eq!(
        tree("a\rb\r\r**c**\r"),
        [
            "Paragraph 1:1-2:1",
            "  \"a\" 1:1-1:1",
            "  SoftBreak 1:2-1:2",
            "  \"b\" 2:1-2:1",
            "Paragraph 4:1-4:5",
            "  Strong 4:1-4:5",
            "    \"c\" 4:3-4:3",
        ]
    );
}

#[test]
fn a_line_break_covers_what_stands_before_the_line_end() {
    assert_eq!(
        tree("a  \nb\\\nc\r\nd\n"),
        [
            "Paragraph 1:1-4:1",
            "  \"a\" 1:1-1:1",
            "  LineBreak 1:2-1:4",
            "  \"b\" 2:1-2:1",
            "  LineBreak 2:2-2:3",
            "  \"c\" 3:1-3:1",
            "  SoftBreak 3:2-3:3",
            "  \"d\" 4:1-4:1",
        ]
    );
}

#[test]
fn footnote_definitions_move_behind_the_text() {
    assert_eq!(
        tree("[^a]: def\n\ntext[^a]\n"),
        [
            "Paragraph 3:1-3:8",
            "  \"text\" 3:1-3:4",
            "  FootnoteReference 3:5-3:8",
            "FootnoteDefinition 1:1-1:9",
            "  Paragraph 1:7-1:9",
            "    \"def\" 1:7-1:9",
        ]
    );
}

#[test]
fn only_the_last_definition_of_a_label_moves_behind_the_text() {
    assert_eq!(
        tree("a[^x]\n\n[^x]: one\n\n[^x]: two\n\nb\n"),
        [
            "Paragraph 1:1-1:5",
            "  \"a\" 1:1-1:1",
            "  FootnoteReference 1:2-1:5",
            "FootnoteDefinition 3:1-3:9",
            "  Paragraph 3:7-3:9",
            "    \"one\" 3:7-3:9",
            "Paragraph 7:1-7:1",
            "  \"b\" 7:1-7:1",
            "FootnoteDefinition 5:1-5:9",
            "  Paragraph 5:7-5:9",
            "    \"two\" 5:7-5:9",
        ]
    );
}

#[test]
fn a_definition_moves_out_of_the_quote_it_stands_in() {
    assert_eq!(
        tree("> [^x]: inner\n\na[^x]\n"),
        [
            "BlockQuote 1:1-1:13",
            "Paragraph 3:1-3:5",
            "  \"a\" 3:1-3:1",
            "  FootnoteReference 3:2-3:5",
            "FootnoteDefinition 1:3-1:13",
            "  Paragraph 1:9-1:13",
            "    \"inner\" 1:9-1:13",
        ]
    );
}

#[test]
fn footnotes_stand_in_the_order_of_their_references() {
    assert_eq!(definitions("a[^r] b[^s]\n\n[^s]: S\n\n[^r]: R\n"), ["r", "s"]);
    assert_eq!(
        definitions("a^[x] b[^r] c^[y]\n\n[^r]: z\n"),
        ["__inline_1", "r", "__inline_2"]
    );
}

#[test]
fn a_reference_needs_a_definition_in_the_same_parse() {
    assert_eq!(tree("x[^a]\n"), ["Paragraph 1:1-1:5", "  \"x[^a]\" 1:1-1:5"]);
    assert_eq!(references("x[^a]\n\n[^a]: y\n"), [("a".to_string(), 1)]);
}

#[test]
fn a_definition_without_a_reference_is_dropped_but_numbers_what_it_refers_to() {
    assert_eq!(definitions("[^u]: alone\n\ntext\n"), Vec::<String>::new());
    let hidden = "[^u]: hidden [^s]\n\na[^r]\n\n[^r]: R\n\n[^s]: S\n";
    assert_eq!(references(hidden), [("r".to_string(), 2)]);
    assert_eq!(definitions(hidden), ["s", "r"]);
}

#[test]
fn inline_notes_are_footnotes_named_by_a_running_number() {
    let text = "a^[x] b^[y]\n";
    assert_eq!(
        references(text),
        [("__inline_1".to_string(), 1), ("__inline_2".to_string(), 2)]
    );
    assert_eq!(definitions(text), ["__inline_1", "__inline_2"]);
}

#[test]
fn an_inline_note_counts_its_positions_from_its_own_text() {
    assert_eq!(
        tree("abcdefgh^[b] c"),
        [
            "Paragraph 1:1-1:14",
            "  \"abcdefgh\" 1:1-1:8",
            "  FootnoteReference 1:9-1:12",
            "  \" c\" 1:13-1:14",
            "FootnoteDefinition 1:1-1:0",
            "  Paragraph 1:1-1:0",
            "    \"b\" 1:1-1:1",
        ]
    );
}

/// The reference stays on the note's first line, and the note's second line counts from the prefixes comrak removed.
#[test]
fn a_note_over_several_lines_loses_its_place() {
    assert_eq!(
        tree("> a^[b [c]\n> d] e f"),
        [
            "BlockQuote 1:1-2:8",
            "  Paragraph 1:3-2:8",
            "    \"a\" 1:3-1:3",
            "    FootnoteReference 1:4-1:13",
            "    \" e f\" 1:14-1:17",
            "FootnoteDefinition 1:1-1:0",
            "  Paragraph 1:1-1:0",
            "    \"b [c]\" 1:1-1:5",
            "    SoftBreak 1:6-1:6",
            "    \"d\" 2:7-2:7",
        ]
    );
}

#[test]
fn inlines_behind_a_link_reference_definition_count_from_its_line() {
    let inlines = [
        "  \"foo \" 1:1-1:4",
        "  Strong 1:5-1:11",
        "    \"bar\" 1:7-1:9",
        "  \" baz\" 1:12-1:15",
    ];
    assert_eq!(tree("foo **bar** baz\n")[1..], inlines);
    assert_eq!(tree("[a]: b\nfoo **bar** baz\n")[1..], inlines);
    assert_eq!(
        tree("> [a]: b\n> foo **bar**\n"),
        [
            "BlockQuote 1:1-2:13",
            "  Paragraph 1:3-2:13",
            "    \"foo \" 1:3-1:6",
            "    Strong 1:7-1:13",
            "      \"bar\" 1:9-1:11",
        ]
    );
}

#[test]
fn a_paragraph_holds_the_link_reference_definitions_at_its_start() {
    assert_eq!(tree("[a]: b\nfoo\n")[0], "Paragraph 1:1-2:3");
    assert_eq!(tree("[a]: b\n"), Vec::<String>::new(), "nothing else, nothing shown");
}

#[test]
fn inlines_behind_a_link_over_several_lines_lose_their_place() {
    assert_eq!(
        tree("> [a](b\n> \"c\") **d** e\n"),
        [
            "BlockQuote 1:1-2:14",
            "  Paragraph 1:3-2:14",
            "    Link 1:3-1:7",
            "      \"a\" 1:4-1:4",
            "    \" \" 1:13-1:13",
            "    Strong 1:14-1:18",
            "      \"d\" 1:16-1:16",
            "    \" e\" 1:19-1:20",
        ]
    );
}

#[test]
fn an_escaped_pipe_takes_a_byte_from_the_positions_behind_it() {
    assert_eq!(
        tree("a|b\n-|-\n\\|ä**b**|x\n")[6..],
        [
            "  TableRow 3:1-3:11",
            "    TableCell 3:1-3:9",
            "      \"|ä\" 3:1-3:3",
            "      Strong 3:4-3:8",
            "        \"b\" 3:6-3:6",
            "    TableCell 3:11-3:11",
            "      \"x\" 3:11-3:11",
        ]
    );
}

#[test]
fn a_single_pair_of_pipes_is_an_escaped_tag() {
    assert_eq!(
        tree("a |x| b\n"),
        [
            "Paragraph 1:1-1:7",
            "  \"a \" 1:1-1:2",
            "  EscapedTag 1:3-1:5",
            "    \"x\" 1:4-1:4",
            "  \" b\" 1:6-1:7",
        ]
    );
    assert_eq!(
        tree("a|b\n-|-\n\\|x\\| y|z\n")[7..10],
        [
            "    TableCell 3:1-3:7",
            "      EscapedTag 3:1-3:3",
            "        \"x\" 3:2-3:2",
        ]
    );
}

#[test]
fn cells_beyond_the_header_are_dropped() {
    assert_eq!(
        tree("a|b\n-|-\n1|2|3\n")[6..],
        [
            "  TableRow 3:1-3:5",
            "    TableCell 3:1-3:1",
            "      \"1\" 3:1-3:1",
            "    TableCell 3:3-3:3",
            "      \"2\" 3:3-3:3",
        ]
    );
}

#[test]
fn a_missing_cell_is_an_empty_one_on_the_closing_pipe() {
    assert_eq!(
        tree("| a | b |\n|---|---|\n| 1 |\n")[6..],
        [
            "  TableRow 3:1-3:5",
            "    TableCell 3:2-3:4",
            "      \"1\" 3:3-3:3",
            "    TableCell 3:5-3:5",
        ]
    );
}

#[test]
fn a_tab_counted_as_spaces_makes_code_longer_than_its_source() {
    let text = "-\t\tfoo\n";
    assert_eq!(tree(text)[2], "    CodeBlock 1:3-1:6");
    let literal = first(text, |value| match value {
        NodeValue::CodeBlock(code) => Some(code.literal.clone()),
        _ => None,
    });
    assert_eq!(literal, "  foo\n", "two spaces for the one tab that is left");
}

#[test]
fn an_html_block_behind_a_tab_ends_past_its_line() {
    let text = "-\n\t<div>\nb\n";
    assert_eq!(
        tree(text),
        [
            "List 1:1-2:8",
            "  Item 1:1-2:6",
            "    HtmlBlock 2:2-2:8",
            "Paragraph 3:1-3:1",
            "  \"b\" 3:1-3:1",
        ]
    );
    let literal = first(text, |value| match value {
        NodeValue::HtmlBlock(html) => Some(html.literal.clone()),
        _ => None,
    });
    assert_eq!(literal, "  <div>\n");
}

#[test]
fn character_references_need_their_semicolon_and_a_known_name() {
    let text = "&amp; &eacute; &NotEqualTilde; &#0; &#xE000; &#1114112; &bogus; &amp\n";
    assert_eq!(
        tree(text)[1],
        "  \"& é ≂\\u{338} \u{FFFD} \u{FFFD} \u{FFFD} &bogus; &amp\" 1:1-1:68"
    );
}

#[test]
fn one_tilde_is_subscript_and_two_strike_through() {
    assert_eq!(
        tree("~a~ ~~b~~\n"),
        [
            "Paragraph 1:1-1:9",
            "  Subscript 1:1-1:3",
            "    \"a\" 1:2-1:2",
            "  \" \" 1:4-1:4",
            "  Strikethrough 1:5-1:9",
            "    \"b\" 1:7-1:7",
        ]
    );
}

#[test]
fn a_task_knows_where_its_symbol_stands_and_a_blank_one_has_none() {
    let task = |text: &str| {
        first(text, |value| match value {
            NodeValue::TaskItem(item) => Some((item.symbol, item.symbol_sourcepos.to_string())),
            _ => None,
        })
    };
    assert_eq!(task("- [x] a\n"), (Some('x'), "1:4-1:4".to_string()));
    assert_eq!(task("- [ ] a\n"), (None, "1:4-1:4".to_string()));
}

#[test]
fn lists_and_items_start_at_their_marker() {
    assert_eq!(tree("2) a\n")[..2], ["List 1:1-1:4", "  Item 1:1-1:4"]);
    assert_eq!(tree("  * c\n")[..2], ["List 1:3-1:5", "  Item 1:3-1:5"]);
    let start = first("2) a\n", |value| match value {
        NodeValue::List(list) => Some(list.start),
        _ => None,
    });
    assert_eq!(start, 2);
}

#[test]
fn front_matter_covers_its_delimiter_lines() {
    assert_eq!(
        tree("---\ntitle: x\n---\n\ny\n"),
        ["FrontMatter 1:1-3:3", "Paragraph 5:1-5:1", "  \"y\" 5:1-5:1"]
    );
}

#[test]
fn a_fenced_code_block_covers_its_fences_and_its_literal_does_not() {
    let literal = |text: &str| {
        first(text, |value| match value {
            NodeValue::CodeBlock(code) => Some((code.info.clone(), code.literal.clone())),
            _ => None,
        })
    };
    assert_eq!(tree("```rs x\na\n```\n"), ["CodeBlock 1:1-3:3"]);
    assert_eq!(literal("```rs x\na\n```\n"), ("rs x".to_string(), "a\n".to_string()));
    assert_eq!(tree("```\na\n"), ["CodeBlock 1:1-2:1"], "without an end");
}

#[test]
fn heading_attributes_stay_out_of_the_text_but_in_the_range() {
    assert_eq!(tree("# T {#i .k}\n"), ["Heading 1:1-1:11", "  \"T\" 1:3-1:3"]);
    let id = read("# T {#i .k}\n", |root| {
        let heading = root.first_child().expect("a heading");
        let data = heading.data();
        data.attrs.as_ref().and_then(|attributes| attributes.id.clone())
    });
    assert_eq!(id.as_deref(), Some("i"));
}

#[test]
fn a_callout_title_is_the_first_child_and_a_block_of_its_own() {
    assert_eq!(
        tree("> [!bug]- Absturz\n> Inhalt\n"),
        [
            "Callout 1:1-2:8",
            "  CalloutTitle 1:11-1:17",
            "    Paragraph 1:11-1:17",
            "      \"Absturz\" 1:11-1:17",
            "  Paragraph 2:3-2:8",
            "    \"Inhalt\" 2:3-2:8",
        ]
    );
}

#[test]
fn github_alerts_read_as_callouts() {
    assert_eq!(tree("> [!NOTE]\n> x\n")[0], "Callout 1:1-2:3");
    let name = first("> [!NOTE]\n> x\n", |value| match value {
        NodeValue::Callout(callout) => Some(callout.callout_type.clone()),
        _ => None,
    });
    assert_eq!(name, "note");
}

#[test]
fn quotes_nest_as_deep_as_they_are_written() {
    let text = format!("{}x\n", "> ".repeat(500));
    let depth = read(&text, |root| {
        root.descendants().map(|node| node.ancestors().count()).max()
    });
    assert_eq!(depth, Some(503));
}

#[test]
fn an_address_that_is_its_own_link_has_its_text_at_the_same_place() {
    assert_eq!(
        tree("a https://b.de <https://c.de>\n"),
        [
            "Paragraph 1:1-1:29",
            "  \"a \" 1:1-1:2",
            "  Link 1:3-1:14",
            "    \"https://b.de\" 1:3-1:14",
            "  \" \" 1:15-1:15",
            "  Link 1:16-1:29",
            "    \"https://c.de\" 1:17-1:28",
        ]
    );
}

#[test]
fn a_wiki_link_holds_its_title_or_else_its_target() {
    let text = "[[Seite|Titel]] ![[bild.png]]\n";
    assert_eq!(
        tree(text),
        [
            "Paragraph 1:1-1:29",
            "  WikiLink 1:1-1:15",
            "    \"Titel\" 1:9-1:13",
            "  \" \" 1:16-1:16",
            "  WikiLink 1:17-1:29",
            "    \"bild.png\" 1:20-1:27",
        ]
    );
    let links = read(text, |root| {
        root.descendants()
            .filter_map(|node| match &node.data().value {
                NodeValue::WikiLink(link) => Some((link.url.clone(), link.embed)),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(links, [("Seite".to_string(), false), ("bild.png".to_string(), true)]);
}

#[test]
fn a_formula_in_two_dollars_is_display_math_inside_a_paragraph() {
    assert_eq!(tree("$$\nx\n$$\n"), ["Paragraph 1:1-3:2", "  Math 1:1-3:2"]);
    let math = |text: &str| {
        first(text, |value| match value {
            NodeValue::Math(math) => Some((math.display_math, math.literal.clone())),
            _ => None,
        })
    };
    assert_eq!(math("$$\nx\n$$\n"), (true, "\nx\n".to_string()));
    assert_eq!(math("a $x$\n"), (false, "x".to_string()));
}

#[test]
fn a_shortcode_knows_its_emoji() {
    let emoji = first("a :rocket: b\n", |value| match value {
        NodeValue::ShortCode(code) => Some(code.emoji.clone()),
        _ => None,
    });
    assert_eq!(emoji, "🚀");
}

/// `label_key` has to compare as this does.
#[test]
fn labels_compare_by_unicode_case_folding() {
    let target = |text: &str| {
        first(text, |value| match value {
            NodeValue::Link(link) => Some(link.url.clone()),
            _ => None,
        })
    };
    assert_eq!(target("[ß]\n\n[SS]: http://a\n\n[ß]: http://b\n"), "http://a");
    assert_eq!(target("[foo  bar]\n\n[FOO\tBAR]: http://a\n"), "http://a");
}

/// `Reader::code_content` goes by this instead of reading the last line.
#[test]
fn only_a_real_closing_fence_closes_a_fenced_code_block() {
    let closed = |text: &str| {
        first(text, |value| match value {
            NodeValue::CodeBlock(code) => Some(code.closed),
            _ => None,
        })
    };
    assert!(closed("```\na\n```\n"));
    assert!(closed("> ```\n> a\n> ```\n"));
    assert!(!closed("````\na\n```\n"));
    assert!(!closed("```\na\n~~~\n"));
    assert!(!closed("```\na\n"));
}

/// `vendor/comrak/PATCH.md`: `reference_budget` has to say what this shows.
#[test]
fn links_may_expand_to_100_kb_however_short_the_text() {
    let links = |text: &str| {
        tree(text)
            .iter()
            .filter(|line| line.trim_start().starts_with("Link "))
            .count()
    };
    assert_eq!(links("[a] [a]\n\n[a]: https://example.com/some/long/address\n"), 2);
    let address = format!("https://example.com/{}", "x".repeat(2000));
    let many = format!("{}\n\n[a]: {address}\n", "[a] ".repeat(60));
    assert_eq!(links(&many), 100_000 / address.len());
}
