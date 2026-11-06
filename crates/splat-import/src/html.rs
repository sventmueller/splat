//! What browsers, Word, Google Docs, Confluence, Excel and Notes put on the pasteboard as HTML. One pass over the text, without
//! a tree, so that what is written to hurt takes no more than linear time and no stack.

mod builder;
mod data;
mod style;
mod table;

#[cfg(test)]
mod tests;

use splat_markdown::markup::{self, Tag};

use crate::rich::Rich;
use builder::Builder;

/// The content of an HTML fragment or page.
pub fn from_html(html: &str) -> Rich {
    let mut builder = Builder::default();
    let mut at = 0;
    while at < html.len() {
        at = step(html, at, &mut builder);
    }
    builder.finish()
}

/// Takes in what stands at `at`, a char boundary, and returns the offset to read on from.
fn step(html: &str, at: usize, builder: &mut Builder) -> usize {
    let rest = &html[at..];
    if let Some(comment) = rest.strip_prefix("<!--") {
        let end = comment.find("-->");
        builder.conditional(comment.get(..end.unwrap_or(0)).unwrap_or(""));
        return end.map_or(html.len(), |end| at + 4 + end + 3);
    }
    if rest.starts_with("<!") || rest.starts_with("<?") {
        let end = rest.find('>');
        builder.conditional(rest.get(2..end.unwrap_or(0)).unwrap_or(""));
        return end.map_or(html.len(), |end| at + end + 1);
    }
    if rest.starts_with('<')
        && let Some((tag, end)) = markup::read_tag(html, at)
    {
        match tag {
            Tag::Close { name } => builder.close(&name),
            Tag::Open { name, self_closing, .. } if is_raw(&name) && !self_closing => {
                let (content_end, behind) = closing_tag(html, end, &name);
                if name == "style" {
                    builder.style_sheet(&html[end..content_end]);
                }
                return behind;
            }
            Tag::Open {
                name,
                attrs,
                self_closing,
            } => builder.open(name, &attrs, self_closing),
        }
        return end;
    }
    let first = rest.chars().next().map_or(1, char::len_utf8);
    let next = html[at + first..]
        .find('<')
        .map_or(html.len(), |found| at + first + found);
    builder.text(&html[at..next]);
    next
}

/// Elements dropped with all they hold: what they hold is code, data or the controls of a form.
fn is_raw(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "title"
            | "textarea"
            | "iframe"
            | "noscript"
            | "noembed"
            | "noframes"
            | "object"
            | "applet"
            | "template"
            | "svg"
            | "select"
            | "button"
            | "canvas"
            | "audio"
            | "video"
            | "xml"
    )
}

/// Where the first closing tag of `name` from `from` on starts and the offset behind it, or the end twice if there is none,
/// as a browser reads it.
fn closing_tag(html: &str, from: usize, name: &str) -> (usize, usize) {
    let mut at = from;
    while let Some(found) = html[at..].find("</") {
        let start = at + found + 2;
        let candidate = html.as_bytes().get(start..start + name.len());
        if candidate.is_some_and(|bytes| bytes.eq_ignore_ascii_case(name.as_bytes())) {
            let behind = html[start..].find('>').map_or(html.len(), |end| start + end + 1);
            return (start - 2, behind);
        }
        at = start;
    }
    (html.len(), html.len())
}
