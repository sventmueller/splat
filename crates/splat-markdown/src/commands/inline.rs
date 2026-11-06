use std::ops::Range;

use super::Edit;
use super::lines::{LinePrefix, ListStyle, line_span, setext_underline_behind};
use crate::doc::{
    AlertKind, Block, BlockKind, Doc, Inline, InlineKind, ListKind, block_path, inline_path, walk_blocks, walk_inlines,
};
use crate::text::{Byte, TextAt, line_end, line_start, lines_with_ends, units};

/// A look the formatting commands put on text, each with the markers they write for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineStyle {
    /// `**bold**`.
    Bold,
    /// `*italic*`.
    Italic,
    /// `~~struck~~`.
    Strikethrough,
    /// A code span between backticks.
    Code,
    /// `<ins>underlined</ins>`, for which Markdown has no syntax.
    Underline,
    /// `<mark>marked</mark>`.
    Highlight,
    /// `<sup>raised</sup>`.
    Superscript,
    /// `<sub>lowered</sub>`.
    Subscript,
}

impl InlineStyle {
    fn markers(self) -> (&'static str, &'static str) {
        match self {
            InlineStyle::Bold => ("**", "**"),
            InlineStyle::Italic => ("*", "*"),
            InlineStyle::Strikethrough => ("~~", "~~"),
            InlineStyle::Code => ("`", "`"),
            InlineStyle::Underline => ("<ins>", "</ins>"),
            InlineStyle::Highlight => ("<mark>", "</mark>"),
            InlineStyle::Superscript => ("<sup>", "</sup>"),
            InlineStyle::Subscript => ("<sub>", "</sub>"),
        }
    }

    fn matches(self, kind: &InlineKind) -> bool {
        matches!(
            (self, kind),
            (InlineStyle::Bold, InlineKind::Strong)
                | (InlineStyle::Italic, InlineKind::Emph)
                | (InlineStyle::Strikethrough, InlineKind::Strikethrough)
                | (InlineStyle::Code, InlineKind::Code(_))
                | (InlineStyle::Underline, InlineKind::Underline)
                | (InlineStyle::Highlight, InlineKind::Highlight)
                | (InlineStyle::Superscript, InlineKind::Superscript)
                | (InlineStyle::Subscript, InlineKind::Subscript)
        )
    }
}

/// Nesting order for markers opened together; code comes last because nothing nested in it is parsed.
const NESTING: [InlineStyle; 8] = [
    InlineStyle::Bold,
    InlineStyle::Italic,
    InlineStyle::Strikethrough,
    InlineStyle::Underline,
    InlineStyle::Highlight,
    InlineStyle::Superscript,
    InlineStyle::Subscript,
    InlineStyle::Code,
];

/// Inline styles chosen at a caret outside a word: the text typed next at `at` gets exactly these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The caret the styles were chosen at.
    pub at: Byte,
    /// The styles the text typed there gets, in nesting order.
    pub styles: Vec<InlineStyle>,
}

/// Where text typed at a pending caret goes and the markers around it. `outside` is the position after leaving
/// unwanted spans but before joining an adjacent one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typing {
    /// Where the typed text goes: inside an adjacent span it joins, or at `outside`.
    pub at: Byte,
    /// The caret moved out of the spans the typed text must not continue.
    pub outside: Byte,
    /// The markers in front of the typed text; empty when it joins a span.
    pub open: String,
    /// The markers behind the typed text, in the reverse order of `open`.
    pub close: String,
    /// The styles the typed text ends up with.
    pub styles: Vec<InlineStyle>,
}

impl Typing {
    /// Whether the typed text ends up in a code span.
    pub fn code(&self) -> bool {
        self.styles.contains(&InlineStyle::Code)
    }
}

/// Typed text ready to insert per a [`Typing`]; `caret` counts bytes into `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Where the text goes, in the text before the typing.
    pub at: Byte,
    /// The typed text with any new markers around it.
    pub text: String,
    /// Where the caret stands afterwards, in bytes into `text`.
    pub caret: usize,
    /// Only whitespace was typed; it stays unformatted and the styles wait for the next word.
    pub keep_pending: bool,
}

/// What toggling a style at a caret or a selection comes to.
pub enum Toggle {
    /// A change to the source that puts the style on or takes it off.
    Edit(Edit),
    /// The styles kept for the text typed next; `None` when they are those at the caret again.
    Pending(Option<Pending>),
}

/// The formatting at the caret, as the menus show it with checkmarks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Active {
    /// Inside strong text.
    pub bold: bool,
    /// Inside emphasis.
    pub italic: bool,
    /// Inside struck text.
    pub strikethrough: bool,
    /// Inside a code span.
    pub code: bool,
    /// Inside `<u>` or `<ins>`.
    pub underline: bool,
    /// Inside `==marked==` text or `<mark>`.
    pub highlight: bool,
    /// Inside raised text.
    pub superscript: bool,
    /// Inside lowered text.
    pub subscript: bool,
    /// Inside the text of a link.
    pub link: bool,
    /// The level of the heading at the caret; 0 outside one.
    pub heading: u8,
    /// The kind of list item at the caret, the innermost deciding.
    pub list: Option<ListStyle>,
    /// Inside a block quote, an alert or a callout.
    pub quote: bool,
    /// Inside a table.
    pub table: bool,
    /// Inside a code block.
    pub code_block: bool,
    /// Info string of the fenced code block at the caret.
    pub code_lang: Option<String>,
    /// The kind of the alert or callout at the caret.
    pub alert: Option<AlertKind>,
}

impl Active {
    /// Shows the styles chosen for the next typed text instead of those at the caret.
    pub fn with_pending(mut self, pending: &Pending) -> Active {
        for style in NESTING {
            self.set_inline(style, pending.styles.contains(&style));
        }
        self
    }

    fn set_inline(&mut self, style: InlineStyle, on: bool) {
        let flag = match style {
            InlineStyle::Bold => &mut self.bold,
            InlineStyle::Italic => &mut self.italic,
            InlineStyle::Strikethrough => &mut self.strikethrough,
            InlineStyle::Code => &mut self.code,
            InlineStyle::Underline => &mut self.underline,
            InlineStyle::Highlight => &mut self.highlight,
            InlineStyle::Superscript => &mut self.superscript,
            InlineStyle::Subscript => &mut self.subscript,
        };
        *flag = on;
    }
}

/// The innermost block at `offset` that holds running text.
pub fn leaf_at(doc: &Doc, offset: Byte) -> Option<&Block> {
    block_path(&doc.blocks, doc.fit_at(offset))
        .into_iter()
        .rev()
        .find(|b| b.is_leaf())
}

/// Inline nodes (containers and code spans) whose source range contains `offset`, outermost first.
fn inlines_around(doc: &Doc, offset: Byte) -> Vec<&Inline> {
    let Some(leaf) = leaf_at(doc, offset) else {
        return vec![];
    };
    let mut path = inline_path(leaf.inlines(), offset);
    let level: &[Inline] = path.last().map_or(leaf.inlines(), |i| &i.children);
    if let Some(code) = level
        .iter()
        .find(|i| matches!(i.kind, InlineKind::Code(_)) && i.range.start <= offset && offset <= i.range.end)
    {
        path.push(code);
    }
    path
}

/// The formatting at `offset`, from the inlines and blocks around it.
pub fn active_at(doc: &Doc, offset: Byte) -> Active {
    let offset = doc.fit_at(offset);
    let mut a = Active::default();
    for i in inlines_around(doc, offset) {
        match i.kind {
            InlineKind::Strong => a.bold = true,
            InlineKind::Emph => a.italic = true,
            InlineKind::Strikethrough => a.strikethrough = true,
            InlineKind::Code(_) => a.code = true,
            InlineKind::Underline => a.underline = true,
            InlineKind::Highlight => a.highlight = true,
            InlineKind::Superscript => a.superscript = true,
            InlineKind::Subscript => a.subscript = true,
            InlineKind::Link { .. } => a.link = true,
            _ => {}
        }
    }
    for b in block_path(&doc.blocks, offset) {
        match &b.kind {
            BlockKind::Heading { level, .. } => a.heading = *level,
            BlockKind::BlockQuote => a.quote = true,
            BlockKind::Alert { kind, .. } => {
                a.quote = true;
                a.alert = Some(*kind);
            }
            BlockKind::Table { .. } => a.table = true,
            BlockKind::CodeBlock { info, fenced, .. } => {
                a.code_block = true;
                if *fenced {
                    a.code_lang = Some(info.split_whitespace().next().unwrap_or("").to_string());
                }
            }
            BlockKind::List { kind, .. } => {
                a.list = Some(match kind {
                    ListKind::Bullet(_) => ListStyle::Bullet,
                    ListKind::Ordered { .. } => ListStyle::Numbered,
                })
            }
            BlockKind::Item { task: Some(_), .. } => a.list = Some(ListStyle::Task),
            _ => {}
        }
    }
    a
}

fn delimiter_ranges(text: &str, i: &Inline) -> (Range<Byte>, Range<Byte>) {
    if let InlineKind::Code(_) = i.kind {
        let content = i.code_content(text);
        return (i.range.start..content.start, content.end..i.range.end);
    }
    let content = i.content_range();
    (i.range.start..content.start, content.end..i.range.end)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '\''
}

fn word_at(text: &str, offset: Byte) -> Option<Range<Byte>> {
    let before = text
        .upto(offset)
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, _)| Byte::new(i));
    let after = text
        .from(offset)
        .char_indices()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, c)| offset + i + c.len_utf8());
    let start = before.unwrap_or(offset);
    let end = after.unwrap_or(offset);
    (start < end).then_some(start..end)
}

fn inside_word(text: &str, at: Byte) -> bool {
    text.upto(at).chars().next_back().is_some_and(is_word_char)
        && text.from(at).chars().next().is_some_and(is_word_char)
}

/// Toggles `style` like a word processor: a selection or the word around the caret is reformatted, while at the
/// edge of a word or between words the style is kept for the text typed next.
pub fn toggle(doc: &Doc, sel: Range<Byte>, style: InlineStyle, pending: Option<&Pending>) -> Toggle {
    let sel = doc.fit(sel);
    if !sel.is_empty() || inside_word(doc.text(), sel.start) {
        return Toggle::Edit(toggle_inline(doc, sel, style));
    }
    let at = sel.start;
    let nodes = format_nodes(doc, at);
    let current = active_styles(&nodes, at);
    let chosen = pending.filter(|p| p.at == at).map_or(&current, |p| &p.styles);
    let styles: Vec<InlineStyle> = NESTING
        .into_iter()
        .filter(|s| chosen.contains(s) != (*s == style))
        .collect();
    if styles == current {
        return Toggle::Pending(None);
    }
    let pending = Pending { at, styles };
    if typing_plan(doc, &pending).is_some() {
        return Toggle::Pending(Some(pending));
    }
    // Typed text cannot leave formatting from the middle of it, so the whole formatted span is toggled instead.
    Toggle::Edit(toggle_inline(doc, sel, style))
}

struct FormatNode {
    style: InlineStyle,
    range: Range<Byte>,
    content: Range<Byte>,
}

/// Formatting spans of the leaf block at `at`, including a block that ends exactly there.
fn format_nodes(doc: &Doc, at: Byte) -> Vec<FormatNode> {
    let Some(leaf) = leaf_at(doc, at).or_else(|| leaf_at(doc, Byte::new(at.get().checked_sub(1)?))) else {
        return vec![];
    };
    let mut nodes = Vec::new();
    walk_inlines(leaf.inlines(), &mut |i| {
        if let Some(style) = NESTING.into_iter().find(|s| s.matches(&i.kind)) {
            let (open, close) = delimiter_ranges(doc.text(), i);
            nodes.push(FormatNode {
                style,
                range: i.range.clone(),
                content: open.end..close.start,
            });
        }
    });
    nodes
}

fn continues(node: &FormatNode, at: Byte) -> bool {
    node.content.start <= at && at <= node.content.end
}

/// Styles that text typed at `at` would get without intervention.
fn active_styles(nodes: &[FormatNode], at: Byte) -> Vec<InlineStyle> {
    NESTING
        .into_iter()
        .filter(|s| nodes.iter().any(|n| n.style == *s && continues(n, at)))
        .collect()
}

/// The styles in effect at a position of the text.
pub fn styles_at(doc: &Doc, at: Byte) -> Vec<InlineStyle> {
    let at = doc.fit_at(at);
    active_styles(&format_nodes(doc, at), at)
}

/// Decides where text typed at a pending caret goes so it gets exactly the chosen styles: it leaves spans it must not
/// continue, joins an adjacent span of the one style it adds, and otherwise is wrapped in new markers.
pub fn typing_plan(doc: &Doc, pending: &Pending) -> Option<Typing> {
    // A caret that does not lie in this text was left pending in another one.
    if doc.fit_at(pending.at) != pending.at {
        return None;
    }
    let nodes = format_nodes(doc, pending.at);
    let styles = &pending.styles;
    let mut at = pending.at;
    // Each step leaves one node behind, but positions comrak reports for broken markup may lead back into it.
    let mut steps = nodes.len();
    while let Some(inner) = nodes
        .iter()
        .filter(|n| !styles.contains(&n.style) && continues(n, at))
        .min_by_key(|n| units(&n.range))
    {
        at = if steps == 0 {
            return None;
        } else if at == inner.content.end {
            inner.range.end
        } else if at == inner.content.start {
            inner.range.start
        } else {
            return None;
        };
        steps -= 1;
    }
    let present = active_styles(&nodes, at);
    let wrap: Vec<InlineStyle> = styles.iter().copied().filter(|s| !present.contains(s)).collect();
    let joined = match wrap[..] {
        [style] => nodes.iter().find_map(|n| {
            if n.style != style {
                None
            } else if n.range.end == at {
                Some(n.content.end)
            } else if n.range.start == at {
                Some(n.content.start)
            } else {
                None
            }
        }),
        _ => None,
    };
    Some(match joined {
        Some(inside) => Typing {
            at: inside,
            outside: at,
            open: String::new(),
            close: String::new(),
            styles: styles.clone(),
        },
        None => Typing {
            at,
            outside: at,
            open: wrap.iter().map(|s| s.markers().0).collect(),
            close: wrap.iter().rev().map(|s| s.markers().1).collect(),
            styles: styles.clone(),
        },
    })
}

/// Places `typed` per `plan`. Markdown ignores markers next to whitespace, so whitespace at the edges of the typed
/// text stays outside new markers.
pub fn place(plan: &Typing, typed: &str) -> Placed {
    let rest = typed.trim_start();
    if rest.is_empty() {
        return Placed {
            at: plan.outside,
            text: typed.to_string(),
            caret: typed.len(),
            keep_pending: true,
        };
    }
    if plan.open.is_empty() {
        return Placed {
            at: plan.at,
            text: typed.to_string(),
            caret: typed.len(),
            keep_pending: false,
        };
    }
    let lead = &typed[..typed.len() - rest.len()];
    let body = rest.trim_end();
    let trail = &rest[body.len()..];
    let text = format!("{lead}{}{body}{}{trail}", plan.open, plan.close);
    let caret = if trail.is_empty() {
        lead.len() + plan.open.len() + body.len()
    } else {
        text.len()
    };
    Placed {
        at: plan.at,
        text,
        caret,
        keep_pending: false,
    }
}

/// Moves `len` bytes the user already typed at `typed_at` to their placement; `placed.at` refers to the text
/// before the typing.
pub fn move_typed(text: &str, typed_at: Byte, len: usize, placed: &Placed) -> Edit {
    let typed = text.fit(typed_at..typed_at + len);
    let (typed_at, len) = (typed.start, units(&typed));
    if placed.at >= typed_at {
        let placed_end = text.fit_at(placed.at + len);
        let between = text.at(typed.end..placed_end);
        let caret = typed_at + between.len() + placed.caret;
        Edit {
            range: typed_at..placed_end,
            replacement: format!("{between}{}", placed.text),
            selection: caret..caret,
        }
    } else {
        let placed_at = text.fit_at(placed.at);
        let between = text.at(placed_at..typed_at);
        let caret = placed_at + placed.caret;
        Edit {
            range: placed_at..typed.end,
            replacement: format!("{}{between}", placed.text),
            selection: caret..caret,
        }
    }
}

/// Text typed into the source at a caret with pending styles, moved into their markers: `base` is the text before
/// the typing, `text` after it. Returns the edit and whether the styles still wait for the next word; `None` when
/// the change is not text of one line inserted at the pending caret.
pub fn typed_at_pending(base: &Doc, text: &str, pending: &Pending) -> Option<(Edit, bool)> {
    let doc = base;
    let base = doc.text();
    let at = pending.at;
    let len = text.len().saturating_sub(base.len());
    if len == 0
        || doc.fit_at(at) != at
        || !text.boundary(at)
        || !text.boundary(at + len)
        || text.upto(at) != base.upto(at)
        || text.from(at + len) != base.from(at)
        || text.at(at..at + len).contains('\n')
    {
        return None;
    }
    let plan = typing_plan(doc, pending)?;
    let placed = place(&plan, text.at(at..at + len));
    Some((move_typed(text, at, len, &placed), placed.keep_pending))
}

/// Takes `style` off the span of it that holds the selection, or else puts it on the selection, or on the word around
/// an empty one; over several lines, line by line.
pub fn toggle_inline(doc: &Doc, sel: Range<Byte>, style: InlineStyle) -> Edit {
    let sel = doc.fit(sel);
    let text = doc.text();
    let around_start = inlines_around(doc, sel.start);
    let around_end = inlines_around(doc, sel.end);
    let existing = around_start
        .iter()
        .rev()
        .find(|i| style.matches(&i.kind) && around_end.iter().any(|j| j.range == i.range))
        .copied();
    if let Some(node) = existing {
        let (open, close) = delimiter_ranges(text, node);
        let inner = text.at(open.end..close.start);
        let shift = |o: Byte| {
            if o <= open.start {
                o
            } else if o <= open.end {
                open.start
            } else if o <= close.start {
                o - units(&open)
            } else {
                close.start - units(&open)
            }
        };
        return Edit {
            range: node.range.clone(),
            replacement: inner.to_string(),
            selection: shift(sel.start)..shift(sel.end),
        };
    }

    if text.at(sel.clone()).contains(['\n', '\r']) {
        return wrap_lines(doc, sel, style);
    }

    let (open, close) = style.markers();
    let target = if sel.is_empty() && inside_word(text, sel.start) {
        word_at(text, sel.start).unwrap_or(sel.clone())
    } else {
        sel.clone()
    };
    let slice = text.at(target.clone());
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    let inner = target.start + lead..target.end - trail.min(slice.len() - lead);
    let content = StyleSpans::of(doc, &inner, style).removed_from(text, &inner);
    let (open, close) = if style == InlineStyle::Code {
        code_markers(&content)
    } else {
        (open.to_string(), close.to_string())
    };
    let replacement = format!("{open}{content}{close}");
    let selection = if sel.is_empty() {
        let into_it = sel.start.get().saturating_sub(inner.start.get());
        let cursor = inner.start + open.len() + into_it.min(content.len());
        cursor..cursor
    } else {
        inner.start + open.len()..inner.start + open.len() + content.len()
    };
    Edit {
        range: inner,
        replacement,
        selection,
    }
}

/// The delimiters of a code span around `content`: longer than any run of backticks in it, and apart from one at its
/// edge.
pub fn code_markers(content: &str) -> (String, String) {
    let fence = "`".repeat(longest_backtick_run(content) + 1);
    let pad = if content.starts_with('`') || content.ends_with('`') {
        " "
    } else {
        ""
    };
    (format!("{fence}{pad}"), format!("{pad}{fence}"))
}

pub(super) fn longest_backtick_run(s: &str) -> usize {
    s.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// The spans of a style entirely inside a part of the text, by where they start.
struct StyleSpans<'a>(Vec<&'a Inline>);

impl<'a> StyleSpans<'a> {
    fn of(doc: &'a Doc, part: &Range<Byte>, style: InlineStyle) -> StyleSpans<'a> {
        let mut found = Vec::new();
        if style != InlineStyle::Code {
            walk_blocks(&doc.blocks, &mut |b| {
                if b.is_leaf() && b.range.start < part.end && part.start < b.range.end {
                    walk_inlines(b.inlines(), &mut |i| {
                        if style.matches(&i.kind) && part.start <= i.range.start && i.range.end <= part.end {
                            found.push(i);
                        }
                    });
                }
            });
        }
        found.sort_by_key(|i| i.range.start);
        StyleSpans(found)
    }

    fn contained_in(&self, part: &Range<Byte>) -> impl Iterator<Item = &'a Inline> {
        let first = self.0.partition_point(|i| i.range.start < part.start);
        self.0[first..]
            .iter()
            .copied()
            .take_while(|i| i.range.start < part.end)
            .filter(|i| i.range.end <= part.end)
    }

    /// Whether one span holds all of `body`, apart from the marks of other styles around it.
    fn hold_all_of(&self, text: &str, body: &Range<Byte>) -> bool {
        let only_marks = |part: &str| part.chars().all(|c| matches!(c, '*' | '_' | '~' | '='));
        self.contained_in(body).any(|span| {
            only_marks(text.at(body.start..span.range.start)) && only_marks(text.at(span.range.end..body.end))
        })
    }

    /// The text of `part` without the marks of these spans.
    fn removed_from(&self, text: &str, part: &Range<Byte>) -> String {
        let mut marks: Vec<Range<Byte>> = self
            .contained_in(part)
            .flat_map(|span| {
                let (open, close) = delimiter_ranges(text, span);
                [open, close]
            })
            .collect();
        marks.sort_by_key(|m| m.start);
        let mut out = String::with_capacity(units(part));
        let mut at = part.start;
        for mark in marks {
            if mark.start >= at {
                out.push_str(text.at(at..mark.start));
                at = mark.end;
            }
        }
        out.push_str(text.at(at..part.end));
        out
    }
}

fn wrap_lines(doc: &Doc, sel: Range<Byte>, style: InlineStyle) -> Edit {
    let text = doc.text();
    let (open, close) = style.markers();
    let lines = line_span(text, &sel);
    let all = text.at(lines.clone());
    let body_of = |line: &str, at: Byte| {
        let content = &line[LinePrefix::parse(line).content..];
        let start = at + (line.len() - content.len()) + (content.len() - content.trim_start().len());
        start..start + content.trim().len()
    };
    let spans = StyleSpans::of(doc, &lines, style);
    let mut at = lines.start;
    let mut all_wrapped = style != InlineStyle::Code;
    for (line, ending) in lines_with_ends(all) {
        let body = body_of(line, at);
        all_wrapped = all_wrapped && (body.is_empty() || spans.hold_all_of(text, &body));
        at += line.len() + ending.len();
    }
    let mut out = String::new();
    let mut at = lines.start;
    for (line, ending) in lines_with_ends(all) {
        let body = body_of(line, at);
        // What stands around it stays: indentation, and the spaces of a hard line break.
        out.push_str(text.at(at..body.start));
        if body.is_empty() {
            // Nothing to mark.
        } else if style == InlineStyle::Code {
            let (fence_open, fence_close) = code_markers(text.at(body.clone()));
            out.push_str(&format!("{fence_open}{}{fence_close}", text.at(body.clone())));
        } else if all_wrapped {
            out.push_str(&spans.removed_from(text, &body));
        } else {
            out.push_str(&format!("{open}{}{close}", spans.removed_from(text, &body)));
        }
        let trail = text.at(body.end..at + line.len());
        at += line.len() + ending.len();
        out.push_str(trail);
        out.push_str(ending);
    }
    let end = lines.start + out.len();
    Edit {
        range: lines.clone(),
        replacement: out,
        selection: lines.start..end,
    }
}

/// Takes the formatting off what the selection touches: inline styles and code spans, and the marks of headings on its
/// lines; links and pictures stay.
pub fn remove_formatting(doc: &Doc, sel: Range<Byte>) -> Edit {
    let sel = doc.fit(sel);
    let text = doc.text();
    let span = line_span(text, &sel);
    let mut removals: Vec<Range<Byte>> = Vec::new();
    walk_blocks(&doc.blocks, &mut |b| {
        if !b.is_leaf() || b.range.start >= span.end.max(span.start + 1) || b.range.end <= span.start {
            return;
        }
        walk_inlines(b.inlines(), &mut |i| {
            let touches = i.range.start < sel.end.max(sel.start + 1) && i.range.end > sel.start;
            let formatting = matches!(i.kind, InlineKind::Code(_))
                || i.is_container()
                    && !matches!(
                        i.kind,
                        InlineKind::Link { .. } | InlineKind::Image { .. } | InlineKind::WikiLink { .. }
                    );
            if touches && formatting {
                let (open, close) = delimiter_ranges(text, i);
                removals.push(open);
                removals.push(close);
            }
        });
        // Only a heading's marks: a line of code or of a paragraph may start with `#` too.
        if let BlockKind::Heading { setext, .. } = b.kind {
            let line = line_start(text, b.range.start);
            let first = line_end(text, b.range.start);
            let prefix = LinePrefix::parse(text.at(line..first));
            if let Some((marks, _)) = prefix.heading {
                removals.push(line + marks.start..line + marks.end);
            }
            if setext {
                let title_end = line_span(text, &(b.range.start..b.range.start)).end;
                if let Some(underline) = setext_underline_behind(text, &(line..title_end)) {
                    removals.push(title_end..underline);
                }
            }
        }
    });
    removals.sort_by_key(|r| r.start);
    // A span over several lines takes its marks along from lines the selection does not touch.
    let start = removals.first().map_or(span.start, |r| r.start.min(span.start));
    let mut out = String::new();
    let mut pos = start;
    for r in &removals {
        if r.start < pos {
            continue;
        }
        out.push_str(text.at(pos..r.start));
        pos = r.end;
    }
    let end_of_removals = removals.iter().map(|r| r.end).max().unwrap_or(span.end).max(span.end);
    out.push_str(text.at(pos.min(end_of_removals)..end_of_removals));
    let end = start + out.len();
    Edit {
        range: start..end_of_removals,
        replacement: out,
        selection: start..end,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{applied, run};
    use super::*;
    use crate::reader::parse;
    use crate::text::bytes;

    fn removed(t: &str, at: usize) -> String {
        let mut out = t.to_string();
        remove_formatting(&parse(t), bytes(at..at)).apply_to(&mut out);
        out
    }

    #[test]
    fn removing_formatting_takes_both_marks_of_a_span_over_several_lines() {
        let t = "**foo\nbar** baz\n";
        assert_eq!(removed(t, t.find("foo").unwrap()), "foo\nbar baz\n");
        assert_eq!(removed(t, t.find("bar").unwrap()), "foo\nbar baz\n");
    }

    fn toggled(t: &str, style: InlineStyle) -> String {
        let mut out = t.to_string();
        toggle_inline(&parse(t), bytes(0..t.trim_end().len()), style).apply_to(&mut out);
        out
    }

    #[test]
    fn marking_lines_that_hold_spans_of_the_style_joins_them() {
        assert_eq!(
            toggled("**a** und **b**\n**c**\n", InlineStyle::Bold),
            "**a und b**\n**c**\n"
        );
        assert_eq!(toggled("**a** und **b**\n", InlineStyle::Bold), "**a und b**\n");
    }

    #[test]
    fn lines_that_all_are_one_span_of_the_style_lose_it() {
        assert_eq!(toggled("**a**\n\n__b__\n", InlineStyle::Bold), "a\n\nb\n");
        assert_eq!(toggled("***a***\n\n***b***\n", InlineStyle::Bold), "*a*\n\n*b*\n");
        assert_eq!(toggled("*a*\n\n*b*\n", InlineStyle::Italic), "a\n\nb\n");
        assert_eq!(toggled("**a**\n\n**b**\n", InlineStyle::Italic), "***a***\n\n***b***\n");
    }

    #[test]
    fn an_empty_html_pair_goes_as_a_whole() {
        let t = "<b title=\"äää\"></b> x\n";
        let d = parse(t);
        for caret in [t.find("</b>").unwrap(), 1, 3] {
            let mut out = t.to_string();
            toggle_inline(&d, bytes(caret..caret), InlineStyle::Bold).apply_to(&mut out);
            assert_eq!(out, " x\n", "caret at {caret}");
        }
        let mut out = t.to_string();
        remove_formatting(&d, bytes(1..1)).apply_to(&mut out);
        assert_eq!(out, " x\n");
    }

    #[test]
    fn removing_formatting_leaves_a_link_over_several_lines_alone() {
        let t = "> [a](b\n> \"c\") **d** e\n";
        assert_eq!(
            applied(t, &remove_formatting(&parse(t), bytes(0..t.len()))),
            "> [a](b\n> \"c\") d e\n"
        );
    }

    #[test]
    fn removing_formatting_leaves_lines_of_code_alone() {
        for t in ["```\n# comment\n```\n", "para\n\n```\n# comment\n```\n"] {
            let d = parse(t);
            assert_eq!(applied(t, &remove_formatting(&d, bytes(0..t.len()))), t);
        }
    }

    #[test]
    fn removing_formatting_ends_a_heading_of_either_kind() {
        let atx = "## Titel\n\nx\n";
        assert_eq!(
            applied(atx, &remove_formatting(&parse(atx), bytes(3..3))),
            "Titel\n\nx\n"
        );
        let setext = "Titel\n=====\n\nx\n";
        assert_eq!(
            applied(setext, &remove_formatting(&parse(setext), bytes(2..2))),
            "Titel\n\nx\n"
        );
    }

    #[test]
    fn styling_several_lines_twice_gives_them_back() {
        for t in ["Wort eins\n\nWort zwei\n", "foo  \nbar\n", "a\r\nb\r\n", "a\rb\r"] {
            let d = parse(t);
            let sel = 0..t.trim_end().len();
            let on = toggle_inline(&d, bytes(sel), InlineStyle::Bold);
            let styled = applied(t, &on);
            assert!(styled.contains("**"), "{styled:?}");
            let off = toggle_inline(&parse(&styled), on.selection.clone(), InlineStyle::Bold);
            assert_eq!(applied(&styled, &off), t);
        }
    }

    #[test]
    fn styling_several_lines_keeps_their_endings_and_hard_breaks() {
        let t = "foo  \nbar\n";
        assert_eq!(
            applied(t, &toggle_inline(&parse(t), bytes(0..9), InlineStyle::Bold)),
            "**foo**  \n**bar**\n"
        );
        let crlf = "foo\r\nbar\r\n";
        assert_eq!(
            applied(crlf, &toggle_inline(&parse(crlf), bytes(0..8), InlineStyle::Bold)),
            "**foo**\r\n**bar**\r\n"
        );
    }

    #[test]
    fn code_over_several_lines_is_longer_than_the_backticks_in_it() {
        let t = "a`b\nc\n";
        assert_eq!(
            applied(t, &toggle_inline(&parse(t), bytes(0..5), InlineStyle::Code)),
            "``a`b``\n`c`\n"
        );
    }

    #[test]
    fn bold_wraps_selection_and_unwraps_again() {
        let (s, sel) = run("ein Wort hier", 4..8, |d, r| toggle_inline(d, r, InlineStyle::Bold));
        assert_eq!(s, "ein **Wort** hier");
        assert_eq!(sel, "Wort");
        let (s2, sel2) = run(&s, 6..10, |d, r| toggle_inline(d, r, InlineStyle::Bold));
        assert_eq!(s2, "ein Wort hier");
        assert_eq!(sel2, "Wort");
    }

    #[test]
    fn bold_on_cursor_uses_word_and_trims_spaces() {
        let (s, _) = run("ein Wort hier", 5..5, |d, r| toggle_inline(d, r, InlineStyle::Bold));
        assert_eq!(s, "ein **Wort** hier");
        let (s2, _) = run("ein Wort hier", 3..9, |d, r| toggle_inline(d, r, InlineStyle::Italic));
        assert_eq!(s2, "ein *Wort* hier");
    }

    #[test]
    fn code_and_underline() {
        let (s, _) = run("a b`c d", 2..5, |d, r| toggle_inline(d, r, InlineStyle::Code));
        assert_eq!(s, "a ``b`c`` d");
        let (s2, _) = run("x und y", 2..5, |d, r| toggle_inline(d, r, InlineStyle::Underline));
        assert_eq!(s2, "x <ins>und</ins> y");
        let (s3, _) = run(&s2, 8..10, |d, r| toggle_inline(d, r, InlineStyle::Underline));
        assert_eq!(s3, "x und y");
        let (s4, _) = run("x <u>und</u> y", 6..8, |d, r| {
            toggle_inline(d, r, InlineStyle::Underline)
        });
        assert_eq!(s4, "x und y");
    }

    #[test]
    fn remove_formatting_strips_markers_and_heading() {
        let (s, _) = run("## **a** *b* `c`", 0..16, remove_formatting);
        assert_eq!(s, "a b c");
    }

    fn type_pending(text: &str, at: usize, style: InlineStyle, typed: &str) -> String {
        let d = parse(text);
        let Toggle::Pending(Some(p)) = toggle(&d, bytes(at..at), style, None) else {
            panic!("no pending style at {at} in {text:?}");
        };
        let mut s = text.to_string();
        s.insert_str(at, typed);
        let (edit, _) = typed_at_pending(&parse(text), &s, &p).expect("typing at the pending caret");
        edit.apply_to(&mut s);
        s
    }

    #[test]
    fn caret_after_a_word_formats_what_is_typed_next() {
        assert_eq!(type_pending("ein Wort\n", 8, InlineStyle::Bold, "x"), "ein Wort**x**\n");
        assert_eq!(
            type_pending("ein Wort \n", 9, InlineStyle::Bold, "neu"),
            "ein Wort **neu**\n"
        );
        assert_eq!(
            type_pending("ein Wort\n", 8, InlineStyle::Superscript, " 2"),
            "ein Wort <sup>2</sup>\n"
        );
        let t = "ein Wort hier";
        assert!(matches!(
            toggle(&parse(t), bytes(6..6), InlineStyle::Bold, None),
            Toggle::Edit(_)
        ));
    }

    #[test]
    fn only_text_of_one_line_typed_at_the_pending_caret_is_moved() {
        let base = "ein Wort\n";
        let p = Pending {
            at: Byte::new(8),
            styles: vec![InlineStyle::Bold],
        };
        let base = &parse(base);
        assert!(typed_at_pending(base, "ein Wortx\n", &p).is_some());
        assert!(typed_at_pending(base, base.text(), &p).is_none());
        assert!(typed_at_pending(base, "ein Wort\nx\n", &p).is_none());
        assert!(typed_at_pending(base, "xein Wort\n", &p).is_none());
    }

    #[test]
    fn caret_at_the_end_of_bold_stops_or_continues_it() {
        assert_eq!(type_pending("a **fett**\n", 8, InlineStyle::Bold, "x"), "a **fett**x\n");
        assert_eq!(
            type_pending("a **fett** \n", 10, InlineStyle::Bold, "x"),
            "a **fettx** \n"
        );
        let s = type_pending("a **fett**\n", 8, InlineStyle::Italic, "x");
        assert_eq!(s, "a **fett*x***\n");
        let a = active_at(&parse(&s), Byte::new(s.find('x').unwrap()));
        assert!(a.bold && a.italic);
        let s = type_pending("ein Wort\n", 8, InlineStyle::Bold, "x");
        assert!(active_at(&parse(&s), Byte::new(s.find('x').unwrap())).bold);
        assert!(!active_at(&parse(&s), Byte::new(5)).bold);
    }

    #[test]
    fn pending_style_survives_whitespace_and_toggles_off_again() {
        let t = "ein Wort\n";
        let d = parse(t);
        let Toggle::Pending(Some(p)) = toggle(&d, bytes(8..8), InlineStyle::Bold, None) else {
            panic!("no pending style");
        };
        let placed = place(&typing_plan(&d, &p).unwrap(), " ");
        assert!(placed.keep_pending);
        assert_eq!(placed.at, Byte::new(8));
        assert!(matches!(
            toggle(&d, bytes(8..8), InlineStyle::Bold, Some(&p)),
            Toggle::Pending(None)
        ));
    }

    #[test]
    fn caret_between_words_inside_bold_toggles_the_span() {
        let t = "**foo bar**\n";
        let Toggle::Edit(e) = toggle(&parse(t), bytes(6..6), InlineStyle::Bold, None) else {
            panic!("expected an edit");
        };
        let mut s = t.to_string();
        e.apply_to(&mut s);
        assert_eq!(s, "foo bar\n");
    }

    #[test]
    fn highlight_wraps_in_mark() {
        let (s, sel) = run("ein Wort", 4..8, |d, r| toggle_inline(d, r, InlineStyle::Highlight));
        assert_eq!(s, "ein <mark>Wort</mark>");
        assert_eq!(sel, "Wort");
        assert!(active_at(&parse(&s), Byte::new(12)).highlight);
    }

    #[test]
    fn active_state() {
        let t = "## **fett** x\n";
        let d = parse(t);
        let a = active_at(&d, Byte::new(6));
        assert!(a.bold);
        assert_eq!(a.heading, 2);
        let a = active_at(&d, Byte::new(12));
        assert!(!a.bold);
    }

    #[test]
    fn the_state_at_the_caret_knows_every_format_and_block() {
        let at = |t: &str, p: usize| active_at(&parse(t), Byte::new(p));
        assert!(at("~~ab~~\n", 3).strikethrough);
        assert!(at("`ab`\n", 2).code);
        assert!(at("<u>ab</u>\n", 4).underline);
        assert!(at("<sup>ab</sup>\n", 6).superscript);
        assert!(at("<sub>ab</sub>\n", 6).subscript);
        assert!(at("[ab](c)\n", 2).link);
        assert!(at("> ab\n", 3).quote);
        let alert = at("> [!NOTE]\n> ab\n", 12);
        assert!(alert.quote && alert.alert.is_some());
        assert!(at("| a |\n|---|\n| b |\n", 14).table);
        assert_eq!(at("- ab\n", 3).list, Some(ListStyle::Bullet));
        assert_eq!(at("1. ab\n", 4).list, Some(ListStyle::Numbered));
        assert_eq!(at("- [ ] ab\n", 7).list, Some(ListStyle::Task));
    }

    #[test]
    fn a_code_span_counts_only_where_the_caret_touches_it() {
        let d = parse("x `a` y *z*\n");
        for at in [0, 6, 9] {
            assert!(!active_at(&d, Byte::new(at)).code, "{at}");
        }
        for at in [2, 3, 5] {
            assert!(active_at(&d, Byte::new(at)).code, "{at}");
        }
    }

    #[test]
    fn styles_chosen_for_the_next_text_replace_those_at_the_caret() {
        let pending = Pending {
            at: Byte::new(2),
            styles: vec![InlineStyle::Bold],
        };
        let a = active_at(&parse("*ab*\n"), Byte::new(2)).with_pending(&pending);
        assert!(a.bold && !a.italic);
    }

    #[test]
    fn a_word_goes_on_through_hyphens_underscores_and_apostrophes() {
        for word in ["foo-bar", "foo_bar", "it's"] {
            let t = format!("x {word} y");
            let (s, _) = run(&t, 3..3, |d, r| toggle_inline(d, r, InlineStyle::Bold));
            assert_eq!(s, format!("x **{word}** y"));
        }
    }

    #[test]
    fn typed_text_knows_whether_it_lands_in_code() {
        let plan = |style| {
            typing_plan(
                &parse("a "),
                &Pending {
                    at: Byte::new(2),
                    styles: vec![style],
                },
            )
            .unwrap()
        };
        assert!(plan(InlineStyle::Code).code());
        assert!(!plan(InlineStyle::Bold).code());
    }

    fn typed_with_caret(text: &str, at: usize, style: InlineStyle, typed: &str) -> String {
        let d = parse(text);
        let Toggle::Pending(Some(p)) = toggle(&d, bytes(at..at), style, None) else {
            panic!("no pending style at {at} in {text:?}");
        };
        let mut s = text.to_string();
        s.insert_str(at, typed);
        let (edit, _) = typed_at_pending(&parse(text), &s, &p).expect("typing at the pending caret");
        edit.apply_to(&mut s);
        s.insert(edit.selection.start.get(), '|');
        s
    }

    #[test]
    fn text_typed_beside_a_span_of_its_style_joins_it_and_the_caret_follows() {
        assert_eq!(typed_with_caret("a *b* c", 2, InlineStyle::Italic, "x"), "a *x|b* c");
        assert_eq!(typed_with_caret("x **b**", 0, InlineStyle::Bold, "y"), "**y|**x **b**");
        assert_eq!(
            typed_with_caret("a **b** c", 8, InlineStyle::Bold, "x"),
            "a **b** **x|**c"
        );
    }

    #[test]
    fn nothing_typed_is_nothing_to_move() {
        let pending = Pending {
            at: Byte::new(1),
            styles: vec![InlineStyle::Bold],
        };
        assert!(typed_at_pending(&parse("ab"), "ab", &pending).is_none());
    }

    #[test]
    fn a_selection_reaching_out_of_bold_is_wrapped_and_not_unwrapped() {
        let (s, _) = run("**ab** cd", 3..8, |d, r| toggle_inline(d, r, InlineStyle::Bold));
        assert_ne!(s, "ab cd");
    }

    #[test]
    fn the_caret_keeps_its_letter_when_bold_goes_or_comes() {
        let caret = |t: &str, at: usize| {
            toggle_inline(&parse(t), bytes(at..at), InlineStyle::Bold)
                .selection
                .start
        };
        assert_eq!(caret("**abc**", 3), Byte::new(1), "inside");
        assert_eq!(caret("**abc**", 6), Byte::new(3), "in the closing marker");
        assert_eq!(caret("x abc y", 3), Byte::new(5), "in a word that gets its markers");
    }

    #[test]
    fn a_selection_inside_a_word_stays_that_selection() {
        let (s, sel) = run("abcdef", 2..4, |d, r| toggle_inline(d, r, InlineStyle::Bold));
        assert_eq!(s, "ab**cd**ef");
        assert_eq!(sel, "cd");
    }

    #[test]
    fn code_markers_outgrow_the_backticks_inside() {
        let code = |t: &str| run(t, 0..t.len(), |d, r| toggle_inline(d, r, InlineStyle::Code)).0;
        assert_eq!(code("a``b"), "```a``b```");
        assert_eq!(code("`a"), "`` `a ``");
        assert_eq!(code("a`"), "`` a` ``");
    }

    #[test]
    fn a_selection_over_lines_formats_each_line_and_keeps_its_prefix() {
        let bold = |t: &str| run(t, 0..t.len(), |d, r| toggle_inline(d, r, InlineStyle::Bold)).0;
        assert_eq!(bold("ab\ncd"), "**ab**\n**cd**");
        assert_eq!(bold("> ab\n> cd"), "> **ab**\n> **cd**");
        assert_eq!(bold("  ab\n  cd"), "  **ab**\n  **cd**");
        assert_eq!(bold("ab\n\ncd"), "**ab**\n\n**cd**");
    }

    #[test]
    fn typing_before_bold_with_the_style_switched_off_lands_before_its_markers() {
        let pending = Pending {
            at: Byte::new(4),
            styles: Vec::new(),
        };
        let (edit, _) = typed_at_pending(&parse("x **ab**"), "x **cab**", &pending).unwrap();
        let mut s = "x **cab**".to_string();
        edit.apply_to(&mut s);
        assert_eq!(s, "x c**ab**");
        assert_eq!(edit.selection, bytes(3..3));
    }

    #[test]
    fn typing_that_does_not_end_on_a_letter_is_not_moved() {
        let pending = Pending {
            at: Byte::new(0),
            styles: vec![InlineStyle::Bold],
        };
        assert!(typed_at_pending(&parse("x"), "日", &pending).is_none());
    }

    #[test]
    fn removing_formatting_takes_only_the_spans_at_the_caret() {
        let clean = |at: usize| run("**a** b **c**", at..at, remove_formatting).0;
        assert_eq!(clean(0), "a b **c**");
        assert_eq!(clean(5), "**a** b **c**", "behind the span");
        assert_eq!(clean(6), "**a** b **c**");
        assert_eq!(clean(7), "**a** b **c**", "right before the span");
        assert_eq!(clean(8), "**a** b c");
    }
}
