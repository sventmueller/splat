//! The table, link and image dialogs.

use objc2_app_kit::{NSAlertFirstButtonReturn, NSModalResponseCancel};

use super::super::{Step, applied, at_least, check, editable_fields, perform, select_in_source, source, when};
use super::{SHEET, before, end_sheet, remember, sheet, sizes, undone};
use crate::editor::actions::Action;
use crate::util::ns;

pub(super) fn dialog_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            perform(&c.e, Action::Table.spec().selector, 0);
        }),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("the table dialog opens", sheet(c).is_some(), "no sheet");
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
        when(
            "the new table to reach the text",
            |c| table_rows(&source(&c.e)) > table_rows(&before(c)),
            |c| {
                check(
                    "the table dialog inserts a table of three by three",
                    table_rows(&source(&c.e)) == table_rows(&before(c)) + 4,
                    &sizes(c),
                );
            },
        ),
        undone("inserting a table is undone in one step".into()),
        applied(|c| {
            select_in_source(&c.e, "zwei");
            remember(c);
            perform(&c.e, Action::Link.spec().selector, 0);
        }),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                let mut fields = Vec::new();
                if let Some(view) = sheet(c).and_then(|s| s.contentView()) {
                    editable_fields(&view, &mut fields);
                }
                check("the link dialog asks for address and text", fields.len() == 2, "");
                if let Some(address) = fields.first() {
                    address.setStringValue(&ns("https://splat.example"));
                }
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
        when(
            "the link to reach the text",
            |c| source(&c.e).contains("[zwei](https://splat.example)"),
            |c| {
                check(
                    "the link dialog links the selection",
                    source(&c.e).contains("[zwei](https://splat.example)"),
                    &sizes(c),
                );
            },
        ),
        undone("inserting a link is undone in one step".into()),
        applied(|c| {
            remember(c);
            perform(&c.e, Action::Image.spec().selector, 0);
        }),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("the image dialog opens", sheet(c).is_some(), "no open panel");
                end_sheet(c, NSModalResponseCancel);
            },
        ),
        at_least(
            0.5,
            "the image dialog to close",
            |c| sheet(c).is_none(),
            |c| {
                check(
                    "a cancelled image dialog leaves the text",
                    source(&c.e) == before(c),
                    &sizes(c),
                );
            },
        ),
    ]
}

fn table_rows(text: &str) -> usize {
    text.lines().filter(|l| l.starts_with('|')).count()
}
