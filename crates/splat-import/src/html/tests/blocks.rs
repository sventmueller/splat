//! Blocks, lists and preformatted text.

use super::*;

#[test]
fn blocks_become_blocks() {
    same(&[
        ("<p>a</p><p>b</p>", "a\n\nb\n"),
        ("<p>a<p>b", "a\n\nb\n"),
        ("<div>a</div><div>b</div>", "a\n\nb\n"),
        ("a<div>b</div>c", "a\n\nb\n\nc\n"),
        ("a</p>b", "a\n\nb\n"),
        ("a</div>b", "a\n\nb\n"),
        (
            "<h1>a</h1><h2>b</h2><h3>c</h3><h4>d</h4><h5>e</h5><h6>f</h6>",
            "# a\n\n## b\n\n### c\n\n#### d\n\n##### e\n\n###### f\n",
        ),
        ("<h1><b>a</b></h1>", "# a\n"),
        ("<h2>a <b>b</b></h2>", "## a **b**\n"),
        ("<h1>a<br>b</h1>", "# a<br>b\n"),
        ("<h1>a<br><br>b</h1>", "# a<br>b\n"),
        ("<h1></h1>", ""),
        ("<hr>", "***\n"),
        ("a<hr>b", "a\n\n---\n\nb\n"),
        ("<p>a<hr>", "a\n\n---\n"),
        ("<blockquote>a</blockquote>", "> a\n"),
        ("<blockquote><p>a</p><p>b</p></blockquote>c", "> a\n>\n> b\n\nc\n"),
        (
            "<blockquote>a<blockquote>b</blockquote></blockquote>",
            "> a\n>\n> > b\n",
        ),
        (
            "<section>a</section><article>b</article><figure>c<figcaption>d</figcaption></figure>",
            "a\n\nb\n\nc\n\nd\n",
        ),
        ("<dl><dt>a</dt><dd>b<dd>c</dl>", "a\n\nb\n\nc\n"),
        ("<dl><dt>a<dt>b</dl>", "a\n\nb\n"),
        ("<p/>a<div/>b", "a\n\nb\n"),
        ("<br/>a", "a\n"),
        ("<span/>a", "a\n"),
    ]);
}

#[test]
fn lists_nest_and_count_from_where_they_start() {
    same(&[
        ("<ul><li>a</li><li>b</li></ul>", "- a\n- b\n"),
        ("<ul><li>a<li>b</ul>", "- a\n- b\n"),
        ("<ol><li>a</li><li>b</li></ol>", "1. a\n2. b\n"),
        ("<ol start=\"3\"><li>a</li></ol>", "3. a\n"),
        ("<ol start=\"x\"><li>a</li></ol>", "1. a\n"),
        ("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>", "- a\n  - b\n- c\n"),
        ("<ul><li>a<ol><li>b</li></ol>c</li></ul>", "- a\n\n  1. b\n\n  c\n"),
        ("<ul><li><p>a</p><p>b</p></li></ul>", "- a\n\n  b\n"),
        ("<ul><li><h2>a</h2></li></ul>", "- ## a\n"),
        ("<ul><li><blockquote>a</blockquote></li></ul>", "- > a\n"),
        ("<ul><li><pre>a</pre></li></ul>", "- ```\n  a\n  ```\n"),
        (
            "<ul><li><table><tr><td>a</td><td>b</td></tr></table></li></ul>",
            "- | a | b |\n  |---|---|\n",
        ),
        ("<ul><li>a</li></ul><ul><li>b</li></ul>", "- a\n\n* b\n"),
        ("<ul><li>a</li></ul>b", "- a\n\nb\n"),
        ("<ol><li>a</li></ol><hr><p>b</p>", "1. a\n\n---\n\nb\n"),
    ]);
}

#[test]
fn a_list_right_inside_a_list_goes_into_the_last_item() {
    same(&[
        ("<ul><li>a</li><ul><li>b</li></ul><li>c</li></ul>", "- a\n  - b\n- c\n"),
        ("<ul><ul><li>b</li></ul></ul>", "- - b\n"),
        (
            "<ol><li>a</li><ol><li>b</li><ol><li>c</li></ol></ol></ol>",
            "1. a\n   1. b\n      1. c\n",
        ),
    ]);
}

#[test]
fn what_strays_into_a_list_makes_an_item() {
    same(&[
        ("<ul>a<li>b</li></ul>", "- a\n- b\n"),
        ("<ul><li>a</li>b</ul>", "- a\n- b\n"),
        ("<ul><hr></ul>", "- ___\n"),
        (
            "<ul><table><tr><td>a</td><td>b</td></tr></table><li>c</li></ul>",
            "- | a | b |\n  |---|---|\n\n- c\n",
        ),
        ("<ul><blockquote>a</blockquote></ul>", "- > a\n"),
    ]);
}

#[test]
fn an_item_without_a_list_makes_one() {
    same(&[
        ("<li>a</li><li>b</li>", "- a\n- b\n"),
        ("<li>a</li>b", "- a\n\nb\n"),
        ("<div><li>a<li>b</div>", "- a\n- b\n"),
    ]);
}

#[test]
fn checkboxes_at_the_start_of_an_item_make_tasks() {
    same(&[
        (
            "<ul class=\"contains-task-list\"><li class=\"task-list-item\"><input type=\"checkbox\" disabled> a</li>\
             <li class=\"task-list-item\"><input type=\"checkbox\" checked disabled> b</li></ul>",
            "- [ ] a\n- [x] b\n",
        ),
        ("<ul><li><p><input type=checkbox> a</p></li></ul>", "- [ ] a\n"),
        ("<ul><li>a <input type=checkbox> b</li></ul>", "- a b\n"),
        ("<ul><li><input type=radio> a</li></ul>", "- a\n"),
        (
            "<ul><li><input type=checkbox><input type=checkbox checked> a</li></ul>",
            "- [ ] a\n",
        ),
        ("<input type=checkbox> a", "a\n"),
        (
            "<ul><li role=\"checkbox\" aria-checked=\"true\">a</li><li role=\"checkbox\" aria-checked=\"false\">b</li></ul>",
            "- [x] a\n- [ ] b\n",
        ),
        ("<ul><li role=\"listitem\">a</li></ul>", "- a\n"),
    ]);
}

#[test]
fn preformatted_text_keeps_its_lines() {
    same(&[
        ("<pre>a\n  b</pre>", "```\na\n  b\n```\n"),
        ("<pre>\na</pre>", "```\na\n```\n"),
        ("<pre>\r\na</pre>", "```\na\n```\n"),
        ("<pre>\n\na</pre>", "```\n\na\n```\n"),
        ("<pre><code>a</code></pre>", "```\na\n```\n"),
        (
            "<pre><code class=\"language-rust\">fn a() {}\n</code></pre>",
            "```rust\nfn a() {}\n```\n",
        ),
        ("<pre class=\"lang-c\">a</pre>", "```c\na\n```\n"),
        (
            "<div class=\"highlight highlight-source-rust\"><pre>a</pre></div>",
            "```rust\na\n```\n",
        ),
        (
            "<pre><span class=\"k\">a</span><b>b</b><br>c<img src=x></pre>",
            "```\nab\nc\n```\n",
        ),
        ("<pre>a &lt;b&gt; &amp;</pre>", "```\na <b> &\n```\n"),
        ("<pre>a", "```\na\n```\n"),
        ("<pre></pre>", ""),
        ("<pre>a</div>b</pre>", "```\na\nb\n```\n"),
        ("<pre><pre>a</pre>b</pre>", "```\na\nb\n```\n"),
        ("<p>a<pre>b</pre>c", "a\n\n```\nb\n```\n\nc\n"),
        ("<pre><script>x</script>a</pre>", "```\na\n```\n"),
    ]);
}

#[test]
fn what_editors_copy_as_styled_blocks_is_preformatted_too() {
    same(&[
        (
            "<meta charset='utf-8'><div style=\"color: #d4d4d4;background-color: #1e1e1e;font-family: Menlo, Monaco, 'Courier New', monospace;\
             font-weight: normal;font-size: 12px;line-height: 18px;white-space: pre;\"><div><span style=\"color: #569cd6;\">fn</span>\
             <span style=\"color: #d4d4d4;\"> main() {</span></div><br><div>    x();</div><div>}</div></div>",
            "```\nfn main() {\n\n    x();\n}\n```\n",
        ),
        ("<div style=\"white-space:pre\">a  b</div>", "a b\n"),
        ("<div style=\"font-family:monospace\">a  b</div>", "a b\n"),
        ("<div style=\"white-space:pre;font-family:Arial\">a  b</div>", "a b\n"),
        (
            "<span style=\"white-space:pre;font-family:Menlo\">a  b</span>",
            "`a b`\n",
        ),
        (
            "<p style=\"white-space:pre;font-family:Menlo\">a  b</p>",
            "```\na  b\n```\n",
        ),
    ]);
}

#[test]
fn the_result_is_tidy_for_whoever_asks_what_it_holds() {
    let rich = from_html("<b>a</b><b> </b><b>b</b>").tidied();
    let [Block::Paragraph(spans)] = rich.blocks.as_slice() else {
        panic!("{rich:?}");
    };
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].piece, Span::text("a b").piece);
    assert_ne!(spans[0].marks, Marks::default());
    let list = from_html("<ol start=\"7\"><li>a</ol>").tidied();
    assert!(matches!(
        list.blocks.as_slice(),
        [Block::List(crate::rich::List {
            kind: ListKind::Numbered { start: 7 },
            ..
        })]
    ));
}

#[test]
fn nesting_beyond_the_limit_flattens() {
    let depth = 100_000;
    for (open, close) in [
        ("<div>", "</div>"),
        ("<span>", "</span>"),
        ("<b>", "</b>"),
        ("<ul><li>", "</li></ul>"),
        ("<ul>", "</ul>"),
        ("<blockquote>", "</blockquote>"),
        ("<table><tr><td>", "</td></tr></table>"),
        ("<a href=\"x\">", "</a>"),
        ("<h1>", "</h1>"),
    ] {
        let html = format!("{}text{}after", open.repeat(depth), close.repeat(depth));
        let written = md(&html);
        assert!(written.contains("text") && written.contains("after"), "{open}");
        let again = format!("{}text", open.repeat(depth));
        assert!(md(&again).contains("text"), "{open}");
        let _ = parse(&written);
    }
}

#[test]
fn a_deep_list_stays_within_what_the_writer_can_nest() {
    let html = "<ul><li>".repeat(70) + "x";
    let written = md(&html);
    assert!(written.contains("- x"), "{written}");
    assert!(written.lines().count() == 1, "{written}");
}
