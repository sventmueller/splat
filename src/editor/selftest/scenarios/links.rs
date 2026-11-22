//! Clicks on links in the preview.

use std::cell::Cell;

use objc2::MainThreadOnly;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSEventModifierFlags, NSPasteboard, NSPasteboardTypeString, NSTextFinderAction,
    NSTextFinderBarContainer,
};
use objc2_foundation::NSRange;

use super::super::{Ctx, Step, applied, at_least, check, click_preview, pause, preview_link, when};
use super::{SHEET, documents, end_sheet, sheet};
use crate::util::{constant, ns, storage};

thread_local! {
    /// How many documents were open before a click on a link to a missing file.
    static MISSING_LINK_DOCUMENTS: Cell<usize> = const { Cell::new(0) };
}

/// Clicks the preview link to `target`, scrolled into view first; whether there was one.
fn click_link(c: &Ctx, target: &str, flags: NSEventModifierFlags) -> bool {
    let Some(at) = preview_link(&c.e, target) else {
        return false;
    };
    c.e.preview().scrollRangeToVisible(NSRange::new(at, 1));
    click_preview(&c.e, at, flags);
    true
}

/// A plain click on the preview link to `target`, handed to the controller as AppKit hands it over: on a window in the
/// background, a real plain click would only bring it to the front.
fn plain_click(c: &Ctx, target: &str) -> Option<usize> {
    let at = preview_link(&c.e, target)?;
    c.e.preview().scrollRangeToVisible(NSRange::new(at, 1));
    let link: &AnyObject = &ns(target);
    c.e.links().link_clicked(c.e.preview(), link, at);
    Some(at)
}

fn cmd_click(c: &Ctx, target: &str) -> bool {
    click_link(c, target, NSEventModifierFlags::Command)
}

/// Whether the preview shows the test section's footnote.
fn footnote_in_view(c: &Ctx) -> bool {
    let preview = c.e.preview();
    let text = storage(preview).string().to_string();
    text.find("Die Prüffußnote").is_some_and(|at| {
        let rect = crate::editor::scroll::char_rect(preview, text[..at].encode_utf16().count());
        let shown = preview.visibleRect();
        rect.origin.y >= shown.origin.y && rect.origin.y <= shown.origin.y + shown.size.height
    })
}

/// What the shared find pasteboard asks for and whether the find bar shows.
fn find_bar(c: &Ctx) -> (Option<String>, bool) {
    let board = NSPasteboard::pasteboardWithName(constant!(objc2_app_kit::NSPasteboardNameFind));
    let searched = board
        .stringForType(constant!(NSPasteboardTypeString))
        .map(|s| s.to_string());
    (
        searched,
        c.e.source().enclosingScrollView().is_some_and(|s| s.isFindBarVisible()),
    )
}

#[expect(clippy::too_many_lines, reason = "a list of steps, each of which stands for itself")]
pub(super) fn link_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let clicked = plain_click(c, "splat-footnote:pruef");
            check("the footnote reference is a link", clicked.is_some(), "");
        }),
        when("the footnote to come into view", footnote_in_view, |c| {
            check(
                "a click on a footnote reference shows the footnote",
                footnote_in_view(c),
                "",
            );
            plain_click(c, "https://example.com/pruefstand");
        }),
        applied(|c| {
            let at = preview_link(&c.e, "https://example.com/pruefstand");
            let caret = c.e.preview().selectedRange().location;
            check(
                "a plain click on a web link only places the caret",
                at == Some(caret),
                &format!("{at:?} vs {caret}"),
            );
            check("tags are links", cmd_click(c, "splat-tag:#pruefstand"), "");
        }),
        // AppKit reports a click on a link once a double click can no longer follow.
        when(
            "the ⌘-click on the tag to open the find bar",
            |c| {
                let (searched, bar) = find_bar(c);
                bar && searched.as_deref() == Some("#pruefstand")
            },
            |c| {
                let (searched, bar) = find_bar(c);
                check(
                    "⌘-click on a tag searches for it in the source",
                    bar && searched.as_deref() == Some("#pruefstand"),
                    &format!("{searched:?}, find bar {bar}"),
                );
                let item = objc2_app_kit::NSMenuItem::new(c.e.mtm());
                item.setTag(NSTextFinderAction::HideFindInterface.0);
                // SAFETY: The sender is a menu item whose tag names the action, as the method expects.
                unsafe { c.e.source().performTextFinderAction(Some(&item)) };
                let count = documents(c.e.mtm()).len();
                let opened = cmd_click(c, "andere.md");
                check("links to other documents are links", opened && count > 0, "");
            },
        ),
        when(
            "the linked document to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("andere"))
            },
            |c| {
                let other = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string().starts_with("andere"));
                check(
                    "⌘-click on a link to a Markdown file opens it in Splat",
                    other.as_ref().is_some_and(|d| d.text() == "# Andere\n"),
                    "",
                );
                if let Some(d) = other {
                    d.close();
                }
            },
        ),
        at_least(
            0.5,
            "the linked document to close",
            |c| {
                !documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("andere"))
            },
            |c| {
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
                let count = documents(c.e.mtm()).len();
                cmd_click(c, "fehlt.md");
                MISSING_LINK_DOCUMENTS.set(count);
            },
        ),
        pause(
            0.7,
            "a click that opens nothing gives no signal when it has arrived",
            |c| {
                check(
                    "⌘-click on a missing file opens nothing",
                    documents(c.e.mtm()).len() == MISSING_LINK_DOCUMENTS.get() && sheet(c).is_none(),
                    "",
                );
                check(
                    "links to other apps are links",
                    cmd_click(c, "x-apple.systempreferences:com.apple.Settings"),
                    "",
                );
            },
        ),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("a link to another app asks first", sheet(c).is_some(), "no alert");
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
        applied(|c| {
            if let Some(window) = c.e.window() {
                window.makeKeyAndOrderFront(None);
            }
            check("wiki links are links", cmd_click(c, "splat-wiki:andere"), "");
        }),
        when(
            "the note the wiki link names to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("andere"))
            },
            |c| {
                let other = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string().starts_with("andere"));
                check(
                    "⌘-click on a wiki link opens the note it names in Splat",
                    other.as_ref().is_some_and(|d| d.text() == "# Andere\n"),
                    "",
                );
                if let Some(d) = other {
                    d.close();
                }
            },
        ),
        when(
            "the note to close",
            |c| {
                !documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("andere"))
            },
            |_| {},
        ),
    ]
}

/// The tool tip of the preview link to `target`, which says when the link leads nowhere.
fn link_tip(c: &Ctx, target: &str) -> Option<String> {
    let at = preview_link(&c.e, target)?;
    let text = storage(c.e.preview());
    // SAFETY: An index inside the text, and no range asked for.
    let value = unsafe {
        text.attribute_atIndex_effectiveRange(objc2_app_kit::NSToolTipAttributeName, at, std::ptr::null_mut())
    };
    Some(
        value
            .and_then(|v| v.downcast::<objc2_foundation::NSString>().ok())
            .map(|s| s.to_string())
            .unwrap_or_default(),
    )
}

fn marked(c: &Ctx, target: &str) -> Option<bool> {
    link_tip(c, target).map(|tip| tip.contains(crate::i18n::t("Das Ziel dieses Links fehlt")))
}

fn arrives_later(c: &Ctx) -> std::path::PathBuf {
    c.e.doc_dir().unwrap_or_else(|| c.out.clone()).join("kommt.md")
}

const DEAD_LINKS: &str = "# Ziele\n\n## Da\n\n[Andere](andere.md) [Fehlt](fehlt.md) [Kommt](kommt.md) \
    [[andere]] [[Nirgends]] [Da](#da) [Weg](#weg) <a id=\"html-ziel\"></a>[Html](#html-ziel) \
    [Netz](https://example.com/x) [Einstellungen](x-apple.systempreferences:com.apple.Settings)\n";

pub(super) fn dead_link_steps() -> Vec<Step> {
    let wanted: [(&str, bool); 9] = [
        ("andere.md", false),
        ("fehlt.md", true),
        ("kommt.md", true),
        ("splat-wiki:andere", false),
        ("splat-wiki:Nirgends", true),
        ("#da", false),
        ("#weg", true),
        ("#html-ziel", false),
        ("https://example.com/x", false),
    ];
    vec![
        applied(|c| {
            let _ = std::fs::remove_file(arrives_later(c));
            c.e.replace_all_text(DEAD_LINKS);
            c.e.updater().update_now();
        }),
        when(
            "the links that lead nowhere to be marked",
            move |c| wanted.iter().all(|(target, dead)| marked(c, target) == Some(*dead)),
            move |c| {
                for (target, dead) in wanted {
                    let name = if dead {
                        format!("a link to {target}, which is not there, is marked")
                    } else {
                        format!("a link to {target}, which is there, is not marked")
                    };
                    check(
                        &name,
                        marked(c, target) == Some(dead),
                        &format!("{:?}", link_tip(c, target)),
                    );
                }
                check(
                    "a link to another app is not marked",
                    marked(c, "x-apple.systempreferences:com.apple.Settings") == Some(false),
                    "",
                );
                let at = preview_link(&c.e, "fehlt.md").unwrap_or_default();
                let text = storage(c.e.preview());
                // SAFETY: An index inside the text, and no range asked for.
                let color = unsafe {
                    text.attribute_atIndex_effectiveRange(
                        objc2_app_kit::NSForegroundColorAttributeName,
                        at,
                        std::ptr::null_mut(),
                    )
                };
                check(
                    "a link that leads nowhere is red",
                    color
                        .and_then(|v| v.downcast::<objc2_app_kit::NSColor>().ok())
                        .is_some_and(|v| v == objc2_app_kit::NSColor::systemRedColor()),
                    "",
                );
                std::fs::write(arrives_later(c), "# Kommt\n").expect("a test file");
            },
        ),
        when(
            "a file that appeared to lose the mark of the link to it",
            |c| marked(c, "kommt.md") == Some(false),
            |c| {
                check(
                    "a link loses its mark once its file appears",
                    marked(c, "kommt.md") == Some(false),
                    "",
                );
                check(
                    "the other links keep their marks",
                    marked(c, "fehlt.md") == Some(true) && marked(c, "andere.md") == Some(false),
                    "",
                );
                let _ = std::fs::remove_file(arrives_later(c));
                c.e.replace_all_text(&DEAD_LINKS.replace("](#weg)", "](#da)"));
                c.e.updater().update_now();
            },
        ),
        when(
            "a link whose heading the text now has to lose its mark",
            |c| marked(c, "#weg").is_none() && marked(c, "#da") == Some(false),
            |c| {
                check(
                    "an edit that names an existing heading takes the mark away",
                    marked(c, "#weg").is_none() && marked(c, "#da") == Some(false),
                    "",
                );
            },
        ),
    ]
}
