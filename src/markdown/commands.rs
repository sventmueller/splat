use std::ops::Range;

use super::doc::{
    AlertKind, Block, BlockKind, Doc, Inline, InlineKind, block_path, inline_path, walk_blocks, walk_inlines,
};
use crate::i18n::{t, tf};

/// A single replacement in the Markdown source; `selection` is expressed in the edited text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub replacement: String,
    pub selection: Range<usize>,
}

impl Edit {
    pub fn apply_to(&self, text: &mut String) {
        text.replace_range(self.range.clone(), &self.replacement);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineStyle {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Underline,
    Highlight,
    Superscript,
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
    pub at: usize,
    pub styles: Vec<InlineStyle>,
}

/// Where text typed at a pending caret goes and the markers around it. `outside` is the position after leaving
/// unwanted spans but before joining an adjacent one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typing {
    pub at: usize,
    pub outside: usize,
    pub open: String,
    pub close: String,
    pub styles: Vec<InlineStyle>,
}

impl Typing {
    pub fn code(&self) -> bool {
        self.styles.contains(&InlineStyle::Code)
    }
}

/// Typed text ready to insert per a [`Typing`]; `caret` counts bytes into `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub at: usize,
    pub text: String,
    pub caret: usize,
    /// Only whitespace was typed; it stays unformatted and the styles wait for the next word.
    pub keep_pending: bool,
}

pub enum Toggle {
    Edit(Edit),
    Pending(Option<Pending>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStyle {
    Bullet,
    Numbered,
    Task,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Active {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub underline: bool,
    pub highlight: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub link: bool,
    pub heading: u8,
    pub list: Option<ListStyle>,
    pub quote: bool,
    pub table: bool,
    pub code_block: bool,
    /// Info string of the fenced code block at the caret.
    pub code_lang: Option<String>,
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

pub fn leaf_at(doc: &Doc, offset: usize) -> Option<&Block> {
    block_path(&doc.blocks, offset).into_iter().rev().find(|b| b.is_leaf())
}

/// Inline nodes (containers and code spans) whose source range contains `offset`, outermost first.
fn inlines_around(doc: &Doc, offset: usize) -> Vec<&Inline> {
    let Some(leaf) = leaf_at(doc, offset) else {
        return vec![];
    };
    let mut path = inline_path(&leaf.inlines, offset);
    let level: &[Inline] = path.last().map_or(&leaf.inlines, |i| &i.children);
    if let Some(code) = level
        .iter()
        .find(|i| matches!(i.kind, InlineKind::Code(_)) && i.range.start <= offset && offset <= i.range.end)
    {
        path.push(code);
    }
    path
}

pub fn active_at(doc: &Doc, offset: usize) -> Active {
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
                    super::doc::ListKind::Bullet(_) => ListStyle::Bullet,
                    super::doc::ListKind::Ordered { .. } => ListStyle::Numbered,
                })
            }
            BlockKind::Item { task: Some(_), .. } => a.list = Some(ListStyle::Task),
            _ => {}
        }
    }
    a
}

fn delimiter_ranges(text: &str, i: &Inline) -> (Range<usize>, Range<usize>) {
    if let InlineKind::Code(_) = i.kind {
        let src = &text[i.range.clone()];
        let ticks = src.bytes().take_while(|&b| b == b'`').count();
        return (i.range.start..i.range.start + ticks, i.range.end - ticks..i.range.end);
    }
    let content = i.content_range();
    (i.range.start..content.start, content.end..i.range.end)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '\''
}

fn word_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let before = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, _)| i);
    let after = text[offset..]
        .char_indices()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, c)| offset + i + c.len_utf8());
    let start = before.unwrap_or(offset);
    let end = after.unwrap_or(offset);
    (start < end).then_some(start..end)
}

fn inside_word(text: &str, at: usize) -> bool {
    text[..at].chars().next_back().is_some_and(is_word_char) && text[at..].chars().next().is_some_and(is_word_char)
}

/// Toggles `style` like a word processor: a selection or the word around the caret is reformatted, while at the
/// edge of a word or between words the style is kept for the text typed next.
pub fn toggle(text: &str, doc: &Doc, sel: Range<usize>, style: InlineStyle, pending: Option<&Pending>) -> Toggle {
    if !sel.is_empty() || inside_word(text, sel.start) {
        return Toggle::Edit(toggle_inline(text, doc, sel, style));
    }
    let at = sel.start;
    let nodes = format_nodes(text, doc, at);
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
    if typing_plan(text, doc, &pending).is_some() {
        return Toggle::Pending(Some(pending));
    }
    // Typed text cannot leave formatting from the middle of it, so the whole formatted span is toggled instead.
    Toggle::Edit(toggle_inline(text, doc, sel, style))
}

struct FormatNode {
    style: InlineStyle,
    range: Range<usize>,
    content: Range<usize>,
}

/// Formatting spans of the leaf block at `at`, including a block that ends exactly there.
fn format_nodes(text: &str, doc: &Doc, at: usize) -> Vec<FormatNode> {
    let Some(leaf) = leaf_at(doc, at).or_else(|| leaf_at(doc, at.checked_sub(1)?)) else {
        return vec![];
    };
    let mut nodes = Vec::new();
    walk_inlines(&leaf.inlines, &mut |i| {
        if let Some(style) = NESTING.into_iter().find(|s| s.matches(&i.kind)) {
            let (open, close) = delimiter_ranges(text, i);
            nodes.push(FormatNode {
                style,
                range: i.range.clone(),
                content: open.end..close.start,
            });
        }
    });
    nodes
}

fn continues(node: &FormatNode, at: usize) -> bool {
    node.content.start <= at && at <= node.content.end
}

/// Styles that text typed at `at` would get without intervention.
fn active_styles(nodes: &[FormatNode], at: usize) -> Vec<InlineStyle> {
    NESTING
        .into_iter()
        .filter(|s| nodes.iter().any(|n| n.style == *s && continues(n, at)))
        .collect()
}

/// Decides where text typed at a pending caret goes so it gets exactly the chosen styles: it leaves spans it must not
/// continue, joins an adjacent span of the one style it adds, and otherwise is wrapped in new markers.
pub fn typing_plan(text: &str, doc: &Doc, pending: &Pending) -> Option<Typing> {
    let nodes = format_nodes(text, doc, pending.at);
    let styles = &pending.styles;
    let mut at = pending.at;
    while let Some(inner) = nodes
        .iter()
        .filter(|n| !styles.contains(&n.style) && continues(n, at))
        .min_by_key(|n| n.range.len())
    {
        at = if at == inner.content.end {
            inner.range.end
        } else if at == inner.content.start {
            inner.range.start
        } else {
            return None;
        };
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
pub fn move_typed(text: &str, typed_at: usize, len: usize, placed: &Placed) -> Edit {
    if placed.at >= typed_at {
        let between = &text[typed_at + len..placed.at + len];
        let caret = typed_at + between.len() + placed.caret;
        Edit {
            range: typed_at..placed.at + len,
            replacement: format!("{between}{}", placed.text),
            selection: caret..caret,
        }
    } else {
        let between = &text[placed.at..typed_at];
        let caret = placed.at + placed.caret;
        Edit {
            range: placed.at..typed_at + len,
            replacement: format!("{}{between}", placed.text),
            selection: caret..caret,
        }
    }
}

/// Text typed into the source at a caret with pending styles, moved into their markers: `base` is the text before
/// the typing, `text` after it. Returns the edit and whether the styles still wait for the next word; `None` when
/// the change is not text of one line inserted at the pending caret.
pub fn typed_at_pending(base: &str, text: &str, pending: &Pending) -> Option<(Edit, bool)> {
    let at = pending.at;
    let len = text.len().saturating_sub(base.len());
    if len == 0
        || !text.is_char_boundary(at)
        || !text.is_char_boundary(at + len)
        || text[..at] != base[..at]
        || text[at + len..] != base[at..]
        || text[at..at + len].contains('\n')
    {
        return None;
    }
    let plan = typing_plan(base, &super::doc::parse(base), pending)?;
    let placed = place(&plan, &text[at..at + len]);
    Some((move_typed(text, at, len, &placed), placed.keep_pending))
}

pub fn toggle_inline(text: &str, doc: &Doc, sel: Range<usize>, style: InlineStyle) -> Edit {
    let around_start = inlines_around(doc, sel.start);
    let around_end = inlines_around(doc, sel.end);
    let existing = around_start
        .iter()
        .rev()
        .find(|i| style.matches(&i.kind) && around_end.iter().any(|j| j.range == i.range))
        .copied();
    if let Some(node) = existing {
        let (open, close) = delimiter_ranges(text, node);
        let inner = &text[open.end..close.start];
        let shift = |o: usize| {
            if o <= open.start {
                o
            } else if o <= open.end {
                open.start
            } else if o <= close.start {
                o - open.len()
            } else {
                close.start - open.len()
            }
        };
        return Edit {
            range: node.range.clone(),
            replacement: inner.to_string(),
            selection: shift(sel.start)..shift(sel.end),
        };
    }

    if text[sel.clone()].contains('\n') {
        return wrap_lines(text, sel, style);
    }

    let (open, close) = style.markers();
    let target = if sel.is_empty() && inside_word(text, sel.start) {
        word_at(text, sel.start).unwrap_or(sel.clone())
    } else {
        sel.clone()
    };
    let slice = &text[target.clone()];
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    let inner = target.start + lead..target.end - trail.min(slice.len() - lead);
    let content = &text[inner.clone()];
    let (open, close) = if style == InlineStyle::Code {
        let run = longest_backtick_run(content);
        let fence = "`".repeat(run + 1);
        let pad = if content.starts_with('`') || content.ends_with('`') {
            " "
        } else {
            ""
        };
        (format!("{fence}{pad}"), format!("{pad}{fence}"))
    } else {
        (open.to_string(), close.to_string())
    };
    let replacement = format!("{open}{content}{close}");
    let selection = if sel.is_empty() {
        let cursor = inner.start + open.len() + (sel.start.saturating_sub(inner.start)).min(content.len());
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

fn longest_backtick_run(s: &str) -> usize {
    s.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

fn wrap_lines(text: &str, sel: Range<usize>, style: InlineStyle) -> Edit {
    let (open, close) = style.markers();
    let lines = line_span(text, &sel);
    let mut out = String::new();
    for (n, line) in text[lines.clone()].split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let p = LinePrefix::parse(line);
        let content = &line[p.content..];
        if content.trim().is_empty() {
            out.push_str(line);
            continue;
        }
        let lead = content.len() - content.trim_start().len();
        out.push_str(&line[..p.content + lead]);
        out.push_str(open);
        out.push_str(content.trim());
        out.push_str(close);
    }
    let end = lines.start + out.len();
    Edit {
        range: lines.clone(),
        replacement: out,
        selection: lines.start..end,
    }
}

/// Byte range of the full lines touched by `sel`, without the final newline.
pub fn line_span(text: &str, sel: &Range<usize>) -> Range<usize> {
    let start = text[..sel.start].rfind('\n').map_or(0, |i| i + 1);
    let mut end_anchor = sel.end;
    if sel.end > sel.start && text[..sel.end].ends_with('\n') {
        end_anchor -= 1;
    }
    let end = text[end_anchor..].find('\n').map_or(text.len(), |i| end_anchor + i);
    start..end.max(start)
}

/// Container and block prefixes at the start of a source line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinePrefix {
    pub quote_end: usize,
    pub list: Option<(Range<usize>, ListStyle)>,
    pub heading: Option<(Range<usize>, u8)>,
    pub content: usize,
}

impl LinePrefix {
    pub fn parse(line: &str) -> LinePrefix {
        let b = line.as_bytes();
        let mut i = 0;
        let mut quote_end = 0;
        loop {
            let mut j = i;
            while j < b.len() && b[j] == b' ' && j - i < 3 {
                j += 1;
            }
            if j < b.len() && b[j] == b'>' {
                j += 1;
                if j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                i = j;
                quote_end = i;
            } else {
                break;
            }
        }
        let mut p = LinePrefix {
            quote_end,
            content: i,
            ..Default::default()
        };
        let mut j = i;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        let marker_start = j;
        let mut kind = None;
        if j < b.len() && matches!(b[j], b'-' | b'*' | b'+') && b.get(j + 1).is_none_or(|c| *c == b' ') {
            j += 1;
            kind = Some(ListStyle::Bullet);
        } else {
            let digits = b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
            if (1..=9).contains(&digits)
                && matches!(b.get(j + digits), Some(b'.' | b')'))
                && b.get(j + digits + 1).is_none_or(|c| *c == b' ')
            {
                j += digits + 1;
                kind = Some(ListStyle::Numbered);
            }
        }
        if let Some(mut k) = kind {
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            if b[j..].len() >= 3 && b[j] == b'[' && matches!(b[j + 1], b' ' | b'x' | b'X') && b[j + 2] == b']' {
                j += 3;
                while j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                k = ListStyle::Task;
            }
            p.list = Some((marker_start..j, k));
            p.content = j;
            if kind == Some(ListStyle::Bullet) && line[marker_start..].starts_with("---") {
                p.list = None;
                p.content = i;
            }
        }
        let rest = &line[p.content..];
        let hashes = rest.bytes().take_while(|&c| c == b'#').count();
        if (1..=6).contains(&hashes) && rest.as_bytes().get(hashes).is_none_or(|c| *c == b' ') {
            let end = p.content + hashes + usize::from(rest.len() > hashes);
            p.heading = Some((p.content..end, hashes as u8));
            p.content = end;
        }
        p
    }
}

fn map_lines(text: &str, sel: &Range<usize>, mut f: impl FnMut(usize, &str, &LinePrefix) -> String) -> Edit {
    let span = line_span(text, sel);
    let mut out = String::new();
    for (n, line) in text[span.clone()].split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        out.push_str(&f(n, line, &LinePrefix::parse(line)));
    }
    let delta = out.len() as isize - span.len() as isize;
    let new_end = (span.end as isize + delta) as usize;
    let selection = if sel.is_empty() {
        let line_end_after = new_end;
        let pos = (sel.start as isize + delta).clamp(span.start as isize, line_end_after as isize) as usize;
        let pos = if sel.start == span.start {
            span.start + (out.len().min(out.find('\n').unwrap_or(out.len())))
        } else {
            pos
        };
        pos..pos
    } else {
        span.start..new_end
    };
    Edit {
        range: span,
        replacement: out,
        selection,
    }
}

pub fn set_heading(text: &str, sel: Range<usize>, level: u8) -> Edit {
    map_lines(text, &sel, |_, line, p| {
        let start = p.heading.as_ref().map_or(p.content, |(r, _)| r.start);
        let body = &line[p.content..];
        if level == 0 {
            format!("{}{}", &line[..start], body)
        } else {
            format!("{}{} {}", &line[..start], "#".repeat(level as usize), body)
        }
    })
}

pub fn toggle_list(text: &str, sel: Range<usize>, style: ListStyle) -> Edit {
    let span = line_span(text, &sel);
    let all_same = text[span.clone()]
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .all(|l| LinePrefix::parse(l).list.is_some_and(|(_, k)| k == style));
    let mut number = 0;
    map_lines(text, &sel, |_, line, p| {
        if line.trim().is_empty() {
            return line.to_string();
        }
        let (keep, body) = match &p.list {
            Some((r, _)) => (&line[..r.start], &line[r.end..]),
            None => (&line[..p.quote_end], &line[p.quote_end..]),
        };
        if all_same {
            return format!("{keep}{body}");
        }
        number += 1;
        let marker = match style {
            ListStyle::Bullet => "- ".to_string(),
            ListStyle::Numbered => format!("{number}. "),
            ListStyle::Task => "- [ ] ".to_string(),
        };
        format!("{keep}{marker}{body}")
    })
}

pub fn toggle_quote(text: &str, sel: Range<usize>) -> Edit {
    let span = line_span(text, &sel);
    let all_quoted = text[span.clone()]
        .split('\n')
        .all(|l| LinePrefix::parse(l).quote_end > 0);
    map_lines(text, &sel, |_, line, _| {
        if all_quoted {
            let trimmed = line.trim_start_matches(' ');
            let rest = trimmed.strip_prefix('>').unwrap_or(trimmed);
            rest.strip_prefix(' ').unwrap_or(rest).to_string()
        } else if line.is_empty() {
            ">".to_string()
        } else {
            format!("> {line}")
        }
    })
}

/// Inserts `block` as its own block at the cursor, adding blank lines where needed.
/// `cursor` is the offset inside `block` where the caret should land.
pub fn insert_block(text: &str, sel: Range<usize>, block: &str, cursor: usize) -> Edit {
    let at = if sel.is_empty() { sel.start } else { sel.end };
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let insert_at = if text[line_start..line_end].trim().is_empty() {
        line_start
    } else {
        line_end
    };
    let before = &text[..insert_at];
    let after = &text[insert_at..];
    let lead = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let trail = if after.is_empty() {
        "\n"
    } else if after.starts_with("\n\n") {
        ""
    } else if after.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let replacement = format!("{lead}{block}{trail}");
    let caret = insert_at + lead.len() + cursor;
    Edit {
        range: insert_at..insert_at,
        replacement,
        selection: caret..caret,
    }
}

pub fn insert_code_block(text: &str, sel: Range<usize>, lang: &str) -> Edit {
    if sel.is_empty() {
        let block = format!("```{lang}\n\n```");
        return insert_block(text, sel, &block, lang.len() + 4);
    }
    let span = line_span(text, &sel);
    let body = &text[span.clone()];
    let fence = "`".repeat(longest_backtick_run(body).max(2) + 1);
    let replacement = format!("{fence}{lang}\n{body}\n{fence}");
    let start = span.start + fence.len() + lang.len() + 1;
    Edit {
        range: span,
        replacement,
        selection: start..start + body.len(),
    }
}

/// Sets the language of the fenced code block at the caret, or inserts a new code block in that language.
pub fn code_block_language(text: &str, doc: &Doc, sel: Range<usize>, lang: &str) -> Edit {
    let block = block_path(&doc.blocks, sel.start)
        .into_iter()
        .rev()
        .find(|b| matches!(b.kind, BlockKind::CodeBlock { fenced: true, .. }));
    let Some(block) = block else {
        return insert_code_block(text, sel, lang);
    };
    let line_end = text[block.range.start..]
        .find('\n')
        .map_or(block.range.end, |i| block.range.start + i);
    let fence = text[block.range.start..line_end].trim_start();
    let fence_start = line_end - fence.len();
    let marker = fence.chars().next().unwrap_or('`');
    let info_start = fence_start + fence.chars().take_while(|c| *c == marker).count();
    Edit {
        range: info_start..line_end,
        replacement: lang.to_string(),
        selection: sel,
    }
    .keeping_selection_after(info_start, line_end - info_start, lang.len())
}

impl Edit {
    /// Shifts the unchanged selection by the length change of an edit made before it.
    fn keeping_selection_after(mut self, at: usize, removed: usize, inserted: usize) -> Edit {
        let shift = |o: usize| {
            if o <= at {
                o
            } else if o < at + removed {
                at + inserted
            } else {
                o + inserted - removed
            }
        };
        self.selection = shift(self.selection.start)..shift(self.selection.end);
        self
    }
}

/// Formats the paragraph at the caret (or the selected lines) as an alert of `kind`, like a quote. On an alert it
/// changes the type, or removes the alert when it already has this type; on an empty line it inserts a new one.
pub fn insert_alert(text: &str, doc: &Doc, sel: Range<usize>, kind: AlertKind) -> Edit {
    let existing = block_path(&doc.blocks, sel.start)
        .into_iter()
        .rev()
        .find_map(|b| match &b.kind {
            BlockKind::Alert {
                marker, kind: current, ..
            } => Some((b.range.clone(), marker.clone(), *current)),
            _ => None,
        });
    if let Some((block, marker, current)) = existing {
        if current == kind {
            return remove_alert(text, &block);
        }
        let keyword = marker.start + 2..marker.start + text[marker.clone()].find(']').unwrap_or(2);
        let keyword_len = keyword.len();
        return Edit {
            range: keyword.clone(),
            replacement: kind.keyword().to_string(),
            selection: sel,
        }
        .keeping_selection_after(keyword.start, keyword_len, kind.keyword().len());
    }
    let head = format!("> [!{}]", kind.keyword());
    let paragraph = leaf_at(doc, sel.start)
        .filter(|b| sel.is_empty() && matches!(b.kind, BlockKind::Paragraph))
        .filter(|b| block_path(&doc.blocks, b.range.start).len() == 1)
        .map(|b| b.range.clone());
    let lines = match (sel.is_empty(), paragraph) {
        (false, _) => line_span(text, &sel),
        (true, Some(p)) => line_span(text, &p),
        (true, None) => {
            let block = format!("{head}\n> {}", t("Text"));
            let mut e = insert_block(text, sel, &block, head.len() + 3);
            e.selection = e.selection.start..e.selection.start + t("Text").len();
            return e;
        }
    };
    let body: Vec<String> = text[lines.clone()]
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect();
    let replacement = format!("{head}\n{}", body.join("\n"));
    let end = lines.start + replacement.len();
    Edit {
        range: lines.clone(),
        replacement,
        selection: lines.start..end,
    }
}

/// Turns an alert back into plain paragraphs: drops the `[!TYPE]` line and the quote markers.
fn remove_alert(text: &str, block: &Range<usize>) -> Edit {
    let lines = line_span(text, block);
    let body: Vec<&str> = text[lines.clone()]
        .split('\n')
        .skip(1)
        .map(|l| {
            let l = l.trim_start_matches(' ');
            let l = l.strip_prefix('>').unwrap_or(l);
            l.strip_prefix(' ').unwrap_or(l)
        })
        .collect();
    let replacement = body.join("\n");
    let end = lines.start + replacement.len();
    Edit {
        range: lines.clone(),
        replacement,
        selection: lines.start..end,
    }
}

/// Anchor names of all headings as GitHub generates them, or as given with `{#id}`, with the source range of each
/// heading.
pub fn heading_anchors(doc: &Doc) -> Vec<(u8, String, String, Range<usize>)> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    walk_blocks(&doc.blocks, &mut |b| {
        if let BlockKind::Heading { level, id, .. } = &b.kind {
            let title = plain_text(&b.inlines);
            let anchor = match id {
                Some(id) => id.to_string(),
                None => {
                    let base = slug(&title);
                    let n = seen.entry(base.clone()).or_insert(0);
                    let anchor = if *n == 0 { base.clone() } else { format!("{base}-{n}") };
                    *n += 1;
                    anchor
                }
            };
            out.push((*level, title, anchor, b.range.clone()));
        }
    });
    out
}

fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => s.push_str(t),
            InlineKind::SoftBreak | InlineKind::LineBreak => s.push(' '),
            InlineKind::Math { literal, .. } => s.push_str(literal),
            InlineKind::Emoji(e) => s.push_str(e),
            InlineKind::Html(_) | InlineKind::FootnoteRef { .. } | InlineKind::Comment => {}
            _ => s.push_str(&plain_text(&i.children)),
        }
    }
    s
}

/// GitHub's heading anchors: lower case, punctuation dropped, spaces turned into hyphens.
fn slug(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// Inserts a linked table of contents, or regenerates the one at the caret. A single top-level title is left out.
pub fn insert_toc(text: &str, doc: &Doc, sel: Range<usize>) -> Option<Edit> {
    let mut headings = heading_anchors(doc);
    if headings.iter().filter(|h| h.0 == 1).count() == 1 && headings.first().is_some_and(|h| h.0 == 1) {
        headings.remove(0);
    }
    let top = headings.iter().map(|h| h.0).min()?;
    let lines: Vec<String> = headings
        .iter()
        .filter(|h| h.0 <= top + 2)
        .map(|(level, title, anchor, _)| {
            let indent = "  ".repeat((level - top) as usize);
            format!("{indent}- [{}](#{anchor})", escape_label(title))
        })
        .collect();
    let toc = lines.join("\n");
    let existing = block_path(&doc.blocks, sel.start)
        .into_iter()
        .find(|b| matches!(b.kind, BlockKind::List { .. }) && is_toc(b));
    Some(match existing {
        Some(list) => {
            let end = text[..list.range.end].trim_end_matches('\n').len();
            Edit {
                range: list.range.start..end,
                replacement: toc.clone(),
                selection: list.range.start..list.range.start + toc.len(),
            }
        }
        None => {
            let mut e = insert_block(text, sel, &toc, toc.len());
            let start = e.selection.start - toc.len();
            e.selection = start..start + toc.len();
            e
        }
    })
}

/// A list whose every link points into the document.
fn is_toc(list: &Block) -> bool {
    let mut links = 0;
    let mut foreign = false;
    super::doc::walk_blocks(std::slice::from_ref(list), &mut |b| {
        walk_inlines(&b.inlines, &mut |i| {
            if let InlineKind::Link { url, .. } = &i.kind {
                links += 1;
                foreign |= !url.starts_with('#');
            }
        });
    });
    links > 0 && !foreign
}

pub fn insert_rule(text: &str, sel: Range<usize>) -> Edit {
    insert_block(text, sel, "---", 3)
}

pub fn insert_table(text: &str, sel: Range<usize>, rows: usize, cols: usize) -> Edit {
    let table = super::table::new_table(rows, cols);
    let mut e = insert_block(text, sel, &table, 2);
    let start = e.selection.start;
    e.selection = start..start + tf("Spalte {}", &["1"]).len();
    e
}

pub fn insert_link(sel: Range<usize>, url: &str, label: &str) -> Edit {
    let label = if label.trim().is_empty() { url } else { label };
    let replacement = format!("[{}]({})", escape_label(label), link_destination(url));
    let end = sel.start + replacement.len();
    Edit {
        range: sel,
        replacement,
        selection: end..end,
    }
}

pub fn insert_image(sel: Range<usize>, alt: &str, path: &str) -> Edit {
    let replacement = format!("![{}]({})", escape_label(alt), link_destination(path));
    let end = sel.start + replacement.len();
    Edit {
        range: sel,
        replacement,
        selection: end..end,
    }
}

fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]")
}

pub fn link_destination(url: &str) -> String {
    if url.contains([' ', '(', ')']) {
        format!("<{}>", url.replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

pub fn insert_formula(text: &str, sel: Range<usize>) -> Edit {
    let selected = &text[sel.clone()];
    if !selected.is_empty() && !selected.contains('\n') {
        let replacement = format!("${}$", selected.trim());
        let start = sel.start + 1;
        let end = sel.start + replacement.len() - 1;
        return Edit {
            range: sel,
            replacement,
            selection: start..end,
        };
    }
    if selected.trim().is_empty() {
        return insert_block(text, sel, "$$\n\n$$", 3);
    }
    let body = selected.trim();
    let replacement = format!("$$\n{body}\n$$");
    let start = sel.start + 3;
    Edit {
        range: sel,
        replacement,
        selection: start..start + body.len(),
    }
}

pub fn insert_diagram(text: &str, sel: Range<usize>, template: &str) -> Edit {
    let block = format!("```mermaid\n{template}\n```");
    insert_block(text, sel, &block, "```mermaid\n".len())
}

pub fn insert_footnote(text: &str, sel: Range<usize>) -> Edit {
    let mut n = 1;
    while text.contains(&format!("[^{n}]")) {
        n += 1;
    }
    let at = sel.end;
    let reference = format!("[^{n}]");
    let (head, tail) = text.split_at(at);
    let tail_trimmed = tail.trim_end_matches('\n');
    let sep = if format!("{head}{tail_trimmed}").is_empty() {
        ""
    } else {
        "\n\n"
    };
    let definition = format!("{sep}[^{n}]: ");
    let replacement = format!("{reference}{tail_trimmed}{definition}\n");
    let caret = at + reference.len() + tail_trimmed.len() + definition.len();
    Edit {
        range: at..text.len(),
        replacement,
        selection: caret..caret,
    }
}

pub fn remove_formatting(text: &str, doc: &Doc, sel: Range<usize>) -> Edit {
    let span = line_span(text, &sel);
    let mut removals: Vec<Range<usize>> = Vec::new();
    super::doc::walk_blocks(&doc.blocks, &mut |b| {
        if !b.is_leaf() || b.range.start >= span.end.max(span.start + 1) || b.range.end <= span.start {
            return;
        }
        walk_inlines(&b.inlines, &mut |i| {
            let touches = i.range.start < sel.end.max(sel.start + 1) && i.range.end > sel.start;
            let formatting = matches!(i.kind, InlineKind::Code(_))
                || i.is_container() && !matches!(i.kind, InlineKind::Link { .. } | InlineKind::Image { .. });
            if touches && formatting {
                let (open, close) = delimiter_ranges(text, i);
                removals.push(open);
                removals.push(close);
            }
        });
    });
    removals.sort_by_key(|r| r.start);
    let mut out = String::new();
    let mut pos = span.start;
    for r in removals.iter().filter(|r| r.start >= span.start && r.end <= span.end) {
        if r.start < pos {
            continue;
        }
        out.push_str(&text[pos..r.start]);
        pos = r.end;
    }
    out.push_str(&text[pos..span.end]);
    let result = set_heading(&out, 0..out.len(), 0).replacement;
    let end = span.start + result.len();
    Edit {
        range: span.clone(),
        replacement: result,
        selection: span.start..end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::doc::parse;

    fn run(text: &str, sel: Range<usize>, f: impl Fn(&str, &Doc, Range<usize>) -> Edit) -> (String, String) {
        let d = parse(text);
        let e = f(text, &d, sel);
        let mut s = text.to_string();
        e.apply_to(&mut s);
        let selected = s[e.selection.clone()].to_string();
        (s, selected)
    }

    #[test]
    fn bold_wraps_selection_and_unwraps_again() {
        let (s, sel) = run("ein Wort hier", 4..8, |t, d, r| {
            toggle_inline(t, d, r, InlineStyle::Bold)
        });
        assert_eq!(s, "ein **Wort** hier");
        assert_eq!(sel, "Wort");
        let (s2, sel2) = run(&s, 6..10, |t, d, r| toggle_inline(t, d, r, InlineStyle::Bold));
        assert_eq!(s2, "ein Wort hier");
        assert_eq!(sel2, "Wort");
    }

    #[test]
    fn bold_on_cursor_uses_word_and_trims_spaces() {
        let (s, _) = run("ein Wort hier", 5..5, |t, d, r| {
            toggle_inline(t, d, r, InlineStyle::Bold)
        });
        assert_eq!(s, "ein **Wort** hier");
        let (s2, _) = run("ein Wort hier", 3..9, |t, d, r| {
            toggle_inline(t, d, r, InlineStyle::Italic)
        });
        assert_eq!(s2, "ein *Wort* hier");
    }

    #[test]
    fn code_and_underline() {
        let (s, _) = run("a b`c d", 2..5, |t, d, r| toggle_inline(t, d, r, InlineStyle::Code));
        assert_eq!(s, "a ``b`c`` d");
        let (s2, _) = run("x und y", 2..5, |t, d, r| {
            toggle_inline(t, d, r, InlineStyle::Underline)
        });
        assert_eq!(s2, "x <ins>und</ins> y");
        let (s3, _) = run(&s2, 8..10, |t, d, r| toggle_inline(t, d, r, InlineStyle::Underline));
        assert_eq!(s3, "x und y");
        let (s4, _) = run("x <u>und</u> y", 6..8, |t, d, r| {
            toggle_inline(t, d, r, InlineStyle::Underline)
        });
        assert_eq!(s4, "x und y");
    }

    #[test]
    fn headings_lists_quotes() {
        let (s, _) = run("Titel\nText", 0..0, |t, _, r| set_heading(t, r, 2));
        assert_eq!(s, "## Titel\nText");
        let (s, _) = run("## Titel", 0..0, |t, _, r| set_heading(t, r, 0));
        assert_eq!(s, "Titel");
        let (s, _) = run("a\nb", 0..3, |t, _, r| toggle_list(t, r, ListStyle::Numbered));
        assert_eq!(s, "1. a\n2. b");
        let (s, _) = run(&s, 0..s.len(), |t, _, r| toggle_list(t, r, ListStyle::Task));
        assert_eq!(s, "- [ ] a\n- [ ] b");
        let (s, _) = run(&s, 0..s.len(), |t, _, r| toggle_list(t, r, ListStyle::Task));
        assert_eq!(s, "a\nb");
        let (s, _) = run("a\n\nb", 0..4, |t, _, r| toggle_quote(t, r));
        assert_eq!(s, "> a\n>\n> b");
        let (s, _) = run(&s, 0..s.len(), |t, _, r| toggle_quote(t, r));
        assert_eq!(s, "a\n\nb");
    }

    #[test]
    fn blocks_get_blank_lines() {
        let (s, _) = run("Absatz", 3..3, |t, _, r| insert_rule(t, r));
        assert_eq!(s, "Absatz\n\n---\n");
        let (s, sel) = run("A\n\nB\n", 0..0, |t, _, r| insert_table(t, r, 2, 2));
        assert!(s.starts_with("A\n\n| Spalte 1 | Spalte 2 |"));
        assert!(s.ends_with("|\n\nB\n"));
        assert_eq!(sel, "Spalte 1");
    }

    #[test]
    fn footnote_appends_definition() {
        let (s, _) = run("Text[^1]\n\n[^1]: a\n", 4..4, |t, _, r| insert_footnote(t, r));
        assert_eq!(s, "Text[^2][^1]\n\n[^1]: a\n\n[^2]: \n");
    }

    #[test]
    fn remove_formatting_strips_markers_and_heading() {
        let (s, _) = run("## **a** *b* `c`", 0..16, remove_formatting);
        assert_eq!(s, "a b c");
    }

    fn type_pending(text: &str, at: usize, style: InlineStyle, typed: &str) -> String {
        let d = parse(text);
        let Toggle::Pending(Some(p)) = toggle(text, &d, at..at, style, None) else {
            panic!("no pending style at {at} in {text:?}");
        };
        let mut s = text.to_string();
        s.insert_str(at, typed);
        let (edit, _) = typed_at_pending(text, &s, &p).expect("typing at the pending caret");
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
            toggle(t, &parse(t), 6..6, InlineStyle::Bold, None),
            Toggle::Edit(_)
        ));
    }

    #[test]
    fn only_text_of_one_line_typed_at_the_pending_caret_is_moved() {
        let base = "ein Wort\n";
        let p = Pending {
            at: 8,
            styles: vec![InlineStyle::Bold],
        };
        assert!(typed_at_pending(base, "ein Wortx\n", &p).is_some());
        assert!(typed_at_pending(base, base, &p).is_none());
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
        let a = active_at(&parse(&s), s.find('x').unwrap());
        assert!(a.bold && a.italic);
        let s = type_pending("ein Wort\n", 8, InlineStyle::Bold, "x");
        assert!(active_at(&parse(&s), s.find('x').unwrap()).bold);
        assert!(!active_at(&parse(&s), 5).bold);
    }

    #[test]
    fn pending_style_survives_whitespace_and_toggles_off_again() {
        let t = "ein Wort\n";
        let d = parse(t);
        let Toggle::Pending(Some(p)) = toggle(t, &d, 8..8, InlineStyle::Bold, None) else {
            panic!("no pending style");
        };
        let placed = place(&typing_plan(t, &d, &p).unwrap(), " ");
        assert!(placed.keep_pending);
        assert_eq!(placed.at, 8);
        assert!(matches!(
            toggle(t, &d, 8..8, InlineStyle::Bold, Some(&p)),
            Toggle::Pending(None)
        ));
    }

    #[test]
    fn caret_between_words_inside_bold_toggles_the_span() {
        let t = "**foo bar**\n";
        let Toggle::Edit(e) = toggle(t, &parse(t), 6..6, InlineStyle::Bold, None) else {
            panic!("expected an edit");
        };
        let mut s = t.to_string();
        e.apply_to(&mut s);
        assert_eq!(s, "foo bar\n");
    }

    #[test]
    fn highlight_wraps_in_mark() {
        let (s, sel) = run("ein Wort", 4..8, |t, d, r| {
            toggle_inline(t, d, r, InlineStyle::Highlight)
        });
        assert_eq!(s, "ein <mark>Wort</mark>");
        assert_eq!(sel, "Wort");
        assert!(active_at(&parse(&s), 12).highlight);
    }

    #[test]
    fn code_block_language_is_inserted_or_changed() {
        let (s, _) = run("a\n", 0..0, |t, d, r| code_block_language(t, d, r, "python"));
        assert!(s.contains("```python\n"), "{s:?}");
        let t = "```js\nlet x\n```\n";
        let (s, sel) = run(t, 6..9, |t, d, r| code_block_language(t, d, r, "rust"));
        assert_eq!(s, "```rust\nlet x\n```\n");
        assert_eq!(sel, "let");
        assert_eq!(active_at(&parse(&s), 9).code_lang.as_deref(), Some("rust"));
    }

    #[test]
    fn alerts_are_inserted_wrapped_and_retyped() {
        let (s, _) = run("Absatz\n", 2..2, |t, d, r| insert_alert(t, d, r, AlertKind::Tip));
        assert_eq!(s, "> [!TIP]\n> Absatz\n");
        let (s, sel) = run("A\n\n", 2..2, |t, d, r| insert_alert(t, d, r, AlertKind::Tip));
        assert_eq!(s, "A\n\n> [!TIP]\n> Text\n\n");
        assert_eq!(sel, "Text");
        let (s, _) = run("> [!NOTE]\n> Inhalt\n", 12..12, |t, d, r| {
            insert_alert(t, d, r, AlertKind::Note)
        });
        assert_eq!(s, "Inhalt\n");
        let (s, _) = run("a\nb\n", 0..3, |t, d, r| insert_alert(t, d, r, AlertKind::Note));
        assert_eq!(s, "> [!NOTE]\n> a\n> b\n");
        let (s, sel) = run("> [!NOTE]\n> Inhalt\n", 12..18, |t, d, r| {
            insert_alert(t, d, r, AlertKind::Warning)
        });
        assert_eq!(s, "> [!WARNING]\n> Inhalt\n");
        assert_eq!(sel, "Inhalt");
    }

    #[test]
    fn table_of_contents_links_headings_and_updates_in_place() {
        let t = "# Titel\n\n## Erste Ebene\n\n### Größe & Maß\n\n## Erste Ebene\n";
        let d = parse(t);
        let e = insert_toc(t, &d, 8..8).unwrap();
        let mut s = t.to_string();
        e.apply_to(&mut s);
        assert!(
            s.contains(
                "- [Erste Ebene](#erste-ebene)\n  - [Größe & Maß](#größe--maß)\n- [Erste Ebene](#erste-ebene-1)"
            ),
            "{s}"
        );
        let s2 = s.replace("## Erste Ebene\n\n### Größe", "## Neu\n\n### Größe");
        let at = s2.find("- [").unwrap() + 2;
        let e = insert_toc(&s2, &parse(&s2), at..at).unwrap();
        let mut s3 = s2.clone();
        e.apply_to(&mut s3);
        assert_eq!(s3.matches("](#").count(), 3);
        assert!(s3.contains("- [Neu](#neu)"), "{s3}");
    }

    #[test]
    fn active_state() {
        let t = "## **fett** x\n";
        let d = parse(t);
        let a = active_at(&d, 6);
        assert!(a.bold);
        assert_eq!(a.heading, 2);
        let a = active_at(&d, 12);
        assert!(!a.bold);
    }
}
