//! A large change and a document too large for the preview.

use objc2_foundation::NSRange;

use super::super::{Step, append, applied, check, settle, source, when};
use super::{remember, undone};
use crate::util::{insert_text, ns, storage};

pub(super) fn large_change_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            remember(c);
            let text: String = (1..=1000)
                .map(|n| format!("\n\nAbsatz {n} des großen Nachtrags."))
                .collect();
            append(&c.e, &text);
        }),
        settle(|c| {
            let pending = c.e.model().read(|st| st.blocks().pending());
            check("a large change renders the part around the view first", pending, "");
            let end = storage(c.e.preview()).length();
            c.e.preview().scrollRangeToVisible(NSRange::new(end, 0));
        }),
        when(
            "the blocks scrolled into view to render",
            |c| c.e.model().read(|st| st.blocks().placeholders.last().copied()) == Some(false),
            |c| {
                let last = c.e.model().read(|st| st.blocks().placeholders.last().copied());
                check(
                    "blocks scrolled into view are rendered",
                    last == Some(false),
                    &format!("{last:?}"),
                );
            },
        ),
        undone("the large change is undone in one step".into()),
    ]
}

pub(super) fn huge_document_steps() -> Vec<Step> {
    const NOTICE: &str = "Dieses Dokument ist zu groß für die Vorschau. Der Text lässt sich weiter bearbeiten.";
    vec![
        applied(|c| {
            remember(c);
            let huge = "| Zeile |\n".repeat(3 << 20);
            append(&c.e, &huge);
        }),
        settle(|c| {
            let preview = storage(c.e.preview()).string().to_string();
            check(
                "a huge document says why it has no preview",
                preview == NOTICE,
                &preview,
            );
            let before = source(&c.e).len();
            c.e.preview().setSelectedRange(NSRange::new(0, 0));
            insert_text(c.e.preview(), &ns("x"), NSRange::new(0, 0));
            check(
                "the preview of a huge document takes no input",
                !c.e.preview().isEditable()
                    && !c.e.preview_takes_input()
                    && source(&c.e).len() == before
                    && storage(c.e.preview()).string().to_string() == NOTICE,
                "",
            );
        }),
        undone("the huge text is undone in one step".into()),
        settle(|c| {
            let preview = storage(c.e.preview()).string().to_string();
            check(
                "the preview comes back with the smaller text",
                preview.contains("Prüffußnote") && !preview.contains(NOTICE),
                "",
            );
            check(
                "the preview takes input again with the smaller text",
                c.e.preview().isEditable() && c.e.preview_takes_input(),
                "",
            );
        }),
    ]
}
