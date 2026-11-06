//! Exporting through the save panel, and an export that fails.

use std::path::Path;

use objc2_app_kit::{NSAlertFirstButtonReturn, NSModalResponseCancel, NSSavePanel};

use super::super::{Ctx, Step, applied, at_least, check, perform, when};
use super::{SHEET, check_html_export, check_word_export, doc_dir, end_sheet, sheet};
use crate::editor::actions::Action;

/// The save panel of an export, which proposes `name`; it runs in a process of its own and is cancelled here.
fn proposes(c: &Ctx, name: &str) -> bool {
    let proposed = sheet(c)
        .and_then(|s| s.downcast::<NSSavePanel>().ok())
        .map(|p| p.nameFieldStringValue().to_string());
    end_sheet(c, NSModalResponseCancel);
    proposed.is_some_and(|p| p == name)
}

pub(super) fn export_steps() -> Vec<Step> {
    use crate::editor::export::ExportKind;
    const SAVE_PANEL: &str = "the save panel to show";
    vec![
        applied(|c| perform(&c.e, Action::ExportHtml.spec().selector, 0)),
        // The panel fills in its name from its own process, a moment after its sheet is there.
        at_least(
            0.8,
            SAVE_PANEL,
            |c| sheet(c).is_some(),
            |c| {
                check(
                    "HTML export proposes the document's name",
                    proposes(c, "QuickReference.html"),
                    "",
                );
                check_html_export(&c.e, &doc_dir(c));
                perform(&c.e, Action::ExportWord.spec().selector, 0);
            },
        ),
        at_least(
            0.8,
            SAVE_PANEL,
            |c| sheet(c).is_some(),
            |c| {
                check(
                    "Word export proposes the document's name",
                    proposes(c, "QuickReference.docx"),
                    "",
                );
                check_word_export(&c.e, &doc_dir(c));
                let failed =
                    c.e.export_source()
                        .write(ExportKind::Html, "Export", Path::new("/System/Export.html"));
                if let Err(reason) = &failed {
                    c.e.report_export_failure(reason);
                }
                check("an export to a place without write access fails", failed.is_err(), "");
            },
        ),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("a failed export says so", sheet(c).is_some(), "no alert");
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
    ]
}
