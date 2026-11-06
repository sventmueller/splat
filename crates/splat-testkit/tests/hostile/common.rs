//! What the documents built to hurt share: the check of an edit and inputs of many numbered lines.

use splat_markdown::commands::Edit;
use splat_markdown::text::Byte;

/// An edit a command returned has to apply to the text it was made for.
pub(super) fn applies(text: &str, edit: Edit) {
    let mut out = text.to_string();
    assert!(
        edit.range.end <= Byte::new(text.len())
            && text.is_char_boundary(edit.range.start.get())
            && text.is_char_boundary(edit.range.end.get()),
        "an edit outside the text: {:?} of {} bytes",
        edit.range,
        text.len()
    );
    edit.apply_to(&mut out);
    assert!(
        edit.selection.end <= Byte::new(out.len())
            && out.is_char_boundary(edit.selection.start.get())
            && out.is_char_boundary(edit.selection.end.get()),
        "a selection outside the result: {:?} of {} bytes",
        edit.selection,
        out.len()
    );
}

pub(super) fn numbered(count: usize, line: impl Fn(usize) -> String) -> String {
    (0..count).map(line).collect()
}
