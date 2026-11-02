//! The main click-through: one document edited by every means the window offers, step by step.

use objc2::{DefinedClass, sel};
use objc2_app_kit::{NSApplication, NSTextInputClient};
use objc2_foundation::{NSRange, NSString};

use super::*;
use crate::util::{ns, storage};

pub(super) fn first_edits() -> Vec<Step> {
    vec![
        settle(|c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            let heading_end = "Markdown-Kurzreferenz".encode_utf16().count();
            e.preview().setSelectedRange(NSRange::new(heading_end, 0));
            type_in_preview(e, " X");
            let s = source(e);
            check(
                "typing in preview edits heading",
                s.starts_with("# Markdown-Kurzreferenz X\n"),
                &head(&s, 40),
            );
        }),
        step(0.15, |c| {
            super::super::dialogs::link(&c.e.window().unwrap(), "Beispieltext", |_, _| {})
        }),
        step(0.8, |c| {
            let window = c.e.window().unwrap();
            screen_image(&window, &c.out.join("14-link-sheet.png"));
            let Some(sheet) = window.attachedSheet() else {
                check("link dialog fields span the sheet", false, "no sheet");
                return;
            };
            let mut fields = Vec::new();
            if let Some(content) = sheet.contentView() {
                editable_fields(&content, &mut fields);
            }
            let widths: Vec<f64> = fields.iter().map(|f| f.frame().size.width).collect();
            check(
                "link dialog fields span the sheet",
                widths.len() == 2 && widths.iter().all(|w| *w >= 300.0),
                &format!("{widths:?}"),
            );
            window.endSheet_returnCode(&sheet, objc2_app_kit::NSAlertSecondButtonReturn);
            window.makeFirstResponder(Some(c.e.preview()));
        }),
        step(0.15, |c| unsafe {
            c.e.preview().doCommandBySelector(sel!(insertNewline:))
        }),
    ]
}

pub(super) fn toolbar_and_undo() -> Vec<Step> {
    vec![
        step(0.15, |c| {
            type_in_preview(&c.e, "Neuer Absatz");
            let s = source(&c.e);
            check(
                "return creates a paragraph",
                s.starts_with("# Markdown-Kurzreferenz X\n\nNeuer Absatz"),
                &head(&s, 60),
            );
        }),
        step(0.15, |c| {
            let e = &c.e;
            let p = storage(e.preview()).string().to_string();
            let start = p.encode_utf16().count() - p[p.find("Neuer Absatz").unwrap()..].encode_utf16().count();
            e.preview().setSelectedRange(NSRange::new(start, 5));
            let window = e.window().unwrap();
            let app = NSApplication::sharedApplication(e.mtm());
            // Without a key window (app started from a terminal) NSApp has no responder chain to walk, so this
            // walks the preview's own chain: text view, views, window, window controller.
            let _ = window.isKeyWindow() && unsafe { app.sendAction_to_from(sel!(mdBold:), None, None) }
                || unsafe { e.preview().tryToPerform_with(sel!(mdBold:), None) };
            let s = source(e);
            check(
                "bold from toolbar action in preview",
                s.contains("\n\n**Neuer** Absatz"),
                &format!("{:?}", head(&s, 70)),
            );
        }),
        step(0.15, |c| {
            if let Some(undo) = c.e.undo_manager() {
                undo.undo();
            }
            let s = source(&c.e);
            check(
                "undo removes bold only",
                s.contains("X\n\nNeuer Absatz") && !s.contains("**Neuer**"),
                &format!("{:?}", head(&s, 70)),
            );
        }),
        // Like selecting with the mouse: the other view's caret is already where the selection starts, so
        // nothing else redraws the spot.
        step(0.15, |c| {
            let e = &c.e;
            let (preview_at, _) = neuer_absatz(e);
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            e.preview().setSelectedRange(NSRange::new(preview_at, 0));
        }),
    ]
}

pub(super) fn selection_steps() -> Vec<Step> {
    vec![
        step(0.3, |c| {
            let e = &c.e;
            let (preview_at, source_at) = neuer_absatz(e);
            e.preview().setSelectedRange(NSRange::new(preview_at, 5));
            check(
                "a preview selection shows in the source in the selection colour",
                e.ivars().marks.in_source() == Some(NSRange::new(source_at, 5))
                    && e.source().selectedRange().length == 0,
                &format!(
                    "{:?}, real {:?}",
                    e.ivars().marks.in_source(),
                    e.source().selectedRange()
                ),
            );
        }),
        step(0.4, |c| {
            let e = &c.e;
            let image = c.out.join("12-selection-from-preview.png");
            screen_image(&e.window().unwrap(), &image);
            let (preview_at, source_at) = neuer_absatz(e);
            same_selection_colour(
                "the source shows the preview selection on screen",
                &image,
                (e.preview(), preview_at),
                (e.source(), source_at),
            );
            e.window().unwrap().makeFirstResponder(Some(e.source()));
            e.source().setSelectedRange(NSRange::new(source_at, 0));
        }),
        step(0.3, |c| {
            let e = &c.e;
            let (preview_at, source_at) = neuer_absatz(e);
            e.source().setSelectedRange(NSRange::new(source_at, 5));
            check(
                "a source selection shows in the preview",
                e.ivars().marks.in_preview() == Some(NSRange::new(preview_at, 5))
                    && e.ivars().marks.in_source().is_none(),
                &format!("{:?}", e.ivars().marks.in_preview()),
            );
        }),
        step(0.4, |c| {
            let e = &c.e;
            let image = c.out.join("12-selection-from-source.png");
            screen_image(&e.window().unwrap(), &image);
            let (preview_at, source_at) = neuer_absatz(e);
            same_selection_colour(
                "the preview shows the source selection on screen",
                &image,
                (e.source(), source_at),
                (e.preview(), preview_at),
            );
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            e.preview().setSelectedRange(NSRange::new(preview_at, 5));
        }),
    ]
}

pub(super) fn preview_edit_steps() -> Vec<Step> {
    vec![
        step(0.15, |c| {
            type_in_preview(&c.e, "*");
            let s = source(&c.e);
            check(
                "typed markdown characters are escaped",
                s.contains("\\*"),
                &head(&s, 80),
            );
        }),
        step(0.15, |c| {
            let e = &c.e;
            let p = storage(e.preview()).string().to_string();
            let Some(cell) = p.find("Ergebnis").map(|b| p[..b].encode_utf16().count()) else {
                check("tab moves to next table cell", false, "table not found");
                return;
            };
            e.preview().setSelectedRange(NSRange::new(cell, 0));
            unsafe { e.preview().doCommandBySelector(sel!(insertTab:)) };
            let sel = e.preview().selectedRange();
            let text = storage(e.preview()).string().to_string();
            let selected: String = text
                .encode_utf16()
                .skip(sel.location)
                .take(sel.length)
                .map(|u| char::from_u32(u as u32).unwrap_or('?'))
                .collect();
            check(
                "tab moves to next table cell",
                selected == "Schreibweise",
                &format!("selected {selected:?}"),
            );
        }),
        step(0.15, |c| {
            let e = &c.e;
            let s = source(e);
            let task = s.find("- [ ] Offene").unwrap_or(0);
            let p = storage(e.preview()).string().to_string();
            let Some(b) = p.find("☐\tOffene") else {
                check("checkbox click toggles task", false, "checkbox not rendered");
                return;
            };
            let idx = p[..b].encode_utf16().count();
            let link: Retained<NSString> = ns("splat-task");
            e.link_clicked(e.preview(), &obj(link), idx);
            let s2 = source(e);
            check(
                "checkbox click toggles task",
                s2[task..].starts_with("- [x] Offene"),
                &head(&s2[task..], 14),
            );
        }),
        settle(|c| {
            let e = &c.e;
            let p = storage(e.preview()).string().to_string();
            let unfold = |marker: &str, name: &str| {
                let Some(at) = p.find(marker) else {
                    check(name, false, &format!("{marker} not rendered"));
                    return;
                };
                let link: Retained<NSString> = ns("splat-fold");
                e.link_clicked(e.preview(), &obj(link), p[..at].encode_utf16().count());
            };
            unfold("▸", "a click on ▸ unfolds the callout");
            let s = source(e);
            check(
                "a click on ▸ unfolds the callout",
                s.contains("> [!tip]+") && !s.contains("> [!tip]-"),
                "",
            );
        }),
        settle(|c| {
            let e = &c.e;
            let p = storage(e.preview()).string().to_string();
            let shown = p.contains("Ein Klick auf ▸ klappt die Box auf.");
            check("the unfolded callout shows its content", shown, "");
            let Some(at) = p.find("▸ HTML") else {
                check("a click opens the HTML details", false, "details not rendered");
                return;
            };
            let link: Retained<NSString> = ns("splat-fold");
            e.link_clicked(e.preview(), &obj(link), p[..at].encode_utf16().count());
            check(
                "a click opens the HTML details",
                source(e).contains("<details open>"),
                "",
            );
        }),
        settle(|c| {
            let p = storage(c.e.preview()).string().to_string();
            check(
                "opened details show their Markdown",
                p.contains("HTML wie in GitHub-READMEs wird dargestellt"),
                "",
            );
            check(
                "extended syntax renders",
                p.contains("Textmarker") && !p.contains("==Textmarker==") && p.contains("🚀") && !p.contains("%%"),
                "",
            );
        }),
        step(0.15, |c| {
            let e = &c.e;
            let window = e.window().unwrap();
            snapshot(&window, &c.out.join("02-edited.png"));
            window.makeFirstResponder(Some(e.source()));
            e.set_zoom(1.0);
            snapshot(&window, &c.out.join("03-source-focus.png"));
            export_pdf(e, &c.out.join("export.pdf"));
        }),
    ]
}

pub(super) fn formula_editor_steps() -> Vec<Step> {
    vec![
        step(0.5, |c| {
            let e = &c.e;
            let p = storage(e.preview()).string().to_string();
            let attachments = p.matches('\u{FFFC}').count();
            check(
                "formulas and diagram rendered as images",
                attachments >= 3,
                &format!("{attachments} attachments"),
            );
            if let Some(b) = p.find("Formeln und Diagramme") {
                let idx = p[..b].encode_utf16().count();
                e.preview().scrollRangeToVisible(NSRange::new(idx, 0));
                let window = e.window().unwrap();
                window.makeFirstResponder(Some(e.preview()));
                snapshot(&window, &c.out.join("04-formulas.png"));
            }
        }),
        step(0.15, |c| {
            let e = &c.e;
            let Some(view) = open_formula_editor(e) else {
                check("formula popover opens", false, "no popover");
                return;
            };
            check(
                "formula popover shows TeX",
                view.string().to_string() == "E = mc^2",
                &view.string().to_string(),
            );
            view.setString(&ns("E = mc^3"));
            e.atom_text_changed();
            check("popover edits apply live", source(e).contains("$E = mc^3$"), "");
            close_popover(e);
        }),
        closed(|c| check("closing the popover commits", source(&c.e).contains("$E = mc^3$"), "")),
        step(0.15, |c| {
            if let Some(undo) = c.e.undo_manager() {
                undo.undo();
            }
            let s = source(&c.e);
            let at = s.find("$E = mc").unwrap_or(0);
            check(
                "formula edit is one undo step",
                s.contains("$E = mc^2$"),
                &format!(
                    "{:?}",
                    &s[s.floor_char_boundary(at.saturating_sub(40))..s.ceil_char_boundary((at + 40).min(s.len()))]
                ),
            );
        }),
        step(0.15, |c| {
            let e = &c.e;
            let Some(view) = open_formula_editor(e) else {
                check("formula popover reopens", false, "no popover");
                return;
            };
            view.setString(&ns("x"));
            e.atom_text_changed();
            e.atom_command(sel!(cancelOperation:));
        }),
        closed(|c| {
            check(
                "escape discards the popover edit",
                source(&c.e).contains("$E = mc^2$"),
                "",
            )
        }),
        step(0.15, |c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            let len = storage(e.preview()).length();
            e.preview().setSelectedRange(NSRange::new(len, 0));
            perform(e, sel!(mdInsertFormula:), 0);
            let open = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.popover.isShown());
            check(
                "inserting a formula opens its editor",
                open == Some(true),
                &format!("{open:?}"),
            );
            if let Some(view) = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.view.clone()) {
                view.setString(&ns("a^2 + b^2"));
            }
            e.atom_text_changed();
            close_popover(e);
        }),
        closed(|c| {
            let e = &c.e;
            check(
                "the new formula is kept when its editor closes",
                source(e).contains("$$\na^2 + b^2\n$$"),
                &tail(&source(e), 60),
            );
            let before = source(e);
            perform(e, sel!(mdInsertFormula:), 0);
            check(
                "an empty formula block is inserted",
                source(e).len() > before.len(),
                &tail(&source(e), 60),
            );
            // Like clicking into the source: it takes the focus and the editor closes.
            e.window().unwrap().makeFirstResponder(Some(e.source()));
            close_popover(e);
        }),
        closed(|c| {
            let e = &c.e;
            let s = source(e);
            let responder = e.window().unwrap().firstResponder();
            let responder: Option<&objc2::runtime::AnyObject> = responder.as_deref().map(|r| r.as_ref());
            check(
                "a click into the source keeps the new formula and the focus",
                s.trim_end().ends_with("$$\n\n$$") && e.is_source(responder),
                &tail(&s, 60),
            );
            set_source_caret(e, s.len());
            perform(e, sel!(mdInsertFormula:), 0);
            e.cancel_atom_edit();
        }),
        closed(|c| {
            let e = &c.e;
            let s = source(e);
            check(
                "escape removes a new empty formula again",
                s.trim_end().ends_with("$$\n\n$$") && s.matches("$$\n\n$$").count() == 1,
                &tail(&s, 60),
            );
            // Back to the document before the formulas: insert, edit, insert, insert, remove.
            if let Some(undo) = e.undo_manager() {
                for _ in 0..5 {
                    undo.undo();
                }
            }
        }),
        step(0.15, |c| check_context_menu(&c.e)),
        step(0.05, |c| check_app_menu(c.e.mtm())),
        step(0.5, |c| check_default_probe(c.e.mtm())),
    ]
}

pub(super) fn scrolling_dark_mode_and_remote_steps() -> Vec<Step> {
    vec![
        step(0.15, |c| {
            let e = &c.e;
            let src = source(e);
            let target = src.find("## Tabellen").unwrap();
            let map = crate::markdown::text::Utf16Map::new(&src);
            let y = super::super::scroll::char_rect(e.source(), map.to_u16(target).0)
                .origin
                .y;
            let clip = e.source().enclosingScrollView().unwrap().contentView();
            clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
            e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
        }),
        step(0.5, |c| {
            let e = &c.e;
            let p = storage(e.preview()).string().to_string();
            let top = e.preview().visibleRect().origin.y;
            let tables = p.find("Tabellen\n").map(|b| p[..b].encode_utf16().count()).unwrap_or(0);
            let ty = super::super::scroll::char_rect(e.preview(), tables).origin.y;
            let bottom = e.preview().bounds().size.height - e.preview().visibleRect().size.height;
            check(
                "scrolling the source scrolls the preview",
                (top - ty).abs() < 120.0 || (ty > top && top >= bottom - 2.0),
                &format!("preview top {top:.0}, heading at {ty:.0}"),
            );
            check(
                "source view stays on TextKit 2",
                e.source().textLayoutManager().is_some(),
                "",
            );
            check_link_dialog(e.mtm(), &c.out.join("13-link-dialog.png"));
            let app = NSApplication::sharedApplication(e.mtm());
            app.setAppearance(
                objc2_app_kit::NSAppearance::appearanceNamed(unsafe { objc2_app_kit::NSAppearanceNameDarkAqua })
                    .as_deref(),
            );
        }),
        settle(|c| {
            let e = &c.e;
            snapshot(&e.window().unwrap(), &c.out.join("05-dark.png"));
            let p = storage(e.preview()).string().to_string();
            check(
                "dark mode keeps rendered formulas",
                p.matches('\u{FFFC}').count() >= 3,
                "",
            );
            append(e, "\n\n![Badge](https://bilder.splat-test.example/badge.svg)\n");
            check(
                "images from the web wait for permission",
                e.ivars().remote_banner.get().is_some_and(|b| !b.isHidden()),
                "",
            );
            e.allow_remote_images();
        }),
        settle(|c| {
            let store = storage(c.e.preview());
            let p = store.string().to_string();
            let index = p
                .encode_utf16()
                .enumerate()
                .filter(|(_, u)| *u == 0xFFFC)
                .map(|(i, _)| i)
                .last();
            let width = index
                .and_then(|i| {
                    let value = unsafe {
                        store.attribute_atIndex_effectiveRange(
                            objc2_app_kit::NSAttachmentAttributeName,
                            i,
                            std::ptr::null_mut(),
                        )
                    };
                    value.and_then(|v| v.downcast::<objc2_app_kit::NSTextAttachment>().ok())
                })
                .and_then(|a| a.image())
                .map(|img| img.size().width)
                .unwrap_or(0.0);
            check("remote image loads", width > 40.0, &format!("attachment width {width}"));
            append(&c.e, "\n| a | b | c |\n|---|---|---|\n|   |   |   |");
        }),
    ]
}

pub(super) fn tables_and_spaces_steps() -> Vec<Step> {
    vec![
        step(0.15, |c| {
            let store = storage(c.e.preview());
            let p = store.string().to_string();
            let cells = (0..store.length())
                .filter(|&i| p.encode_utf16().nth(i) == Some(u16::from(b'\n')))
                .filter(|&i| {
                    let style = unsafe {
                        store.attribute_atIndex_effectiveRange(
                            objc2_app_kit::NSParagraphStyleAttributeName,
                            i,
                            std::ptr::null_mut(),
                        )
                    };
                    style
                        .and_then(|v| v.downcast::<objc2_app_kit::NSParagraphStyle>().ok())
                        .is_some_and(|ps| {
                            ps.textBlocks().iter().any(|b| {
                                b.downcast::<objc2_app_kit::NSTextTableBlock>()
                                    .is_ok_and(|t| t.table().numberOfColumns() == 3 && t.startingRow() == 1)
                            })
                        })
                })
                .count();
            let tail: String = p.chars().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
            check(
                "empty last table row keeps all cells",
                cells >= 3,
                &format!("{cells} cells, tail {tail:?}"),
            );
            append(&c.e, "\n\nLeer Test\n\nAm Ende Wort\n");
        }),
        step(0.3, |c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            let p = storage(e.preview()).string().to_string();
            let b = p.rfind("Leer Test").unwrap();
            let end = p[..b].encode_utf16().count() + "Leer Test".encode_utf16().count();
            e.preview().setSelectedRange(NSRange::new(end, 0));
            type_in_preview(e, " ");
        }),
        step(0.15, |c| {
            type_in_preview(&c.e, "z");
            let s = source(&c.e);
            check(
                "a space typed at a paragraph end stays before the next letter (preview)",
                s.contains("Leer Test z\n"),
                &tail(&s, 40),
            );
        }),
    ]
}

pub(super) fn pending_format_steps() -> Vec<Step> {
    vec![
        step(0.3, |c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            let p = storage(e.preview()).string().to_string();
            let Some(b) = p.rfind("Am Ende Wort") else {
                check(
                    "bold at a word end waits for typing (preview)",
                    false,
                    "paragraph not rendered",
                );
                return;
            };
            let end = p[..b].encode_utf16().count() + "Am Ende Wort".encode_utf16().count();
            e.preview().setSelectedRange(NSRange::new(end, 0));
            let _ = unsafe { e.preview().tryToPerform_with(sel!(mdBold:), None) };
            check(
                "bold at a word end waits for typing (preview)",
                source(e).contains("Am Ende Wort\n") && e.active().bold,
                &tail(&source(e), 30),
            );
            type_in_preview(e, " ");
        }),
        step(0.15, |c| type_in_preview(&c.e, "n")),
        step(0.15, |c| {
            type_in_preview(&c.e, "eu");
            let s = source(&c.e);
            check(
                "bold at a word end formats the next word (preview)",
                s.contains("Am Ende Wort **neu**"),
                &tail(&s, 30),
            );
        }),
        step(0.15, |c| {
            let _ = unsafe { c.e.preview().tryToPerform_with(sel!(mdBold:), None) };
            type_in_preview(&c.e, "!");
            let s = source(&c.e);
            check(
                "bold again at the end of bold text stops it (preview)",
                s.contains("Am Ende Wort **neu**!"),
                &tail(&s, 30),
            );
            append(&c.e, "\nQuelle Wort\n");
        }),
        step(0.3, |c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.source()));
            let s = source(e);
            let end = s.rfind("Quelle Wort").unwrap() + "Quelle Wort".len();
            let map = crate::markdown::text::Utf16Map::new(&s);
            e.source().setSelectedRange(NSRange::new(map.to_u16(end).0, 0));
            let _ = unsafe { e.source().tryToPerform_with(sel!(mdBold:), None) };
            type_in_source(e, " ");
        }),
        step(0.15, |c| type_in_source(&c.e, "x")),
        step(0.15, |c| {
            type_in_source(&c.e, "y");
            let s = source(&c.e);
            check(
                "bold at a word end formats the next word (source)",
                s.contains("Quelle Wort **xy**"),
                &tail(&s, 30),
            );
        }),
        step(0.15, |c| {
            if let Some(undo) = c.e.undo_manager() {
                undo.undo();
            }
        }),
        step(0.15, |c| {
            if let Some(undo) = c.e.undo_manager() {
                undo.undo();
            }
            let s = source(&c.e);
            let line = s[s.rfind("Quelle Wort").unwrap()..]
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            check(
                "undo takes back typed text together with its markers",
                !line.contains('*') && !line.contains('x'),
                &line,
            );
            append(&c.e, "\nEingabe\n");
        }),
    ]
}

pub(super) fn input_method_steps() -> Vec<Step> {
    vec![
        step(0.3, |c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            let p = storage(e.preview()).string().to_string();
            let Some(b) = p.rfind("Eingabe") else {
                check(
                    "an input method composes in the preview",
                    false,
                    "paragraph not rendered",
                );
                return;
            };
            let end = p[..b].encode_utf16().count() + "Eingabe".encode_utf16().count();
            e.preview().setSelectedRange(NSRange::new(end, 0));
            compose(e.preview(), "にほん");
        }),
        step(0.15, |c| compose(c.e.preview(), "にほんご")),
        step(0.15, |c| {
            let composing = c.e.preview().hasMarkedText();
            let before = source(&c.e);
            type_in_preview(&c.e, "日本語");
            let s = source(&c.e);
            check(
                "an input method composes in the preview",
                composing && !before.contains('ほ'),
                &format!("marked text {composing}, {}", tail(&before, 30)),
            );
            check(
                "an input method commits its word once (preview)",
                s.contains("\nEingabe日本語\n") && !s.contains('ほ'),
                &tail(&s, 30),
            );
        }),
        step(0.3, |c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.source()));
            let s = source(e);
            let end = s.rfind("Eingabe日本語").map_or(s.len(), |b| b + "Eingabe日本語".len());
            e.source()
                .setSelectedRange(NSRange::new(Utf16Map::new(&s).to_u16(end).0, 0));
            compose(e.source(), "ni");
        }),
        step(0.15, |c| compose(c.e.source(), "nihao")),
        step(0.15, |c| {
            check(
                "an input method composes in the source",
                c.e.source().hasMarkedText(),
                &tail(&source(&c.e), 30),
            );
            type_in_source(&c.e, "你好");
        }),
        step(0.4, |c| {
            let s = source(&c.e);
            let p = storage(c.e.preview()).string().to_string();
            check(
                "an input method commits its word once (source)",
                s.contains("\nEingabe日本語你好\n") && !s.contains("nihao") && p.contains("Eingabe日本語你好"),
                &tail(&s, 30),
            );
        }),
    ]
}

pub(super) fn toolbar_steps() -> Vec<Step> {
    vec![
        step(0.15, |c| {
            let e = &c.e;
            let ids: Vec<String> = e
                .window()
                .and_then(|w| w.toolbar())
                .map(|t| t.items().iter().map(|i| i.itemIdentifier().to_string()).collect())
                .unwrap_or_default();
            check(
                "toolbar offers new tab, share and more formats",
                ["group.character", "group.paragraph", "group.insert", "share", "new"]
                    .iter()
                    .all(|id| ids.iter().any(|i| i == id)),
                &format!("{ids:?}"),
            );
            window_image(&e.window().unwrap(), &c.out.join("06-toolbar.png"));
            let unshared = e.share_file(false).and_then(|u| u.path()).map(|p| p.to_string());
            let unshared_len = unshared
                .as_deref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len());
            check(
                "the share file holds no text before sharing",
                unshared_len == Some(0),
                &format!("{unshared:?}: {unshared_len:?} bytes"),
            );
            let path = e.share_file(true).and_then(|u| u.path()).map(|p| p.to_string());
            let content = path.as_deref().and_then(|p| std::fs::read_to_string(p).ok());
            check(
                "share hands over the current text as a .md file",
                path.as_deref().is_some_and(|p| p.ends_with(".md")) && content == Some(source(e)),
                &format!("{path:?}"),
            );
            let count = || e.window().and_then(|w| w.tabbedWindows()).map_or(1, |t| t.count());
            let before = count();
            let _ = unsafe { e.source().tryToPerform_with(sel!(mdNewTab:), None) };
            let after = count();
            check(
                "plus opens a new document as a tab",
                after == before + 1,
                &format!("{before} -> {after} tabs"),
            );
        }),
        step(0.5, |c| {
            let window = c.e.window().unwrap();
            window_image(&window, &c.out.join("07-tabs.png"));
            if let Some(tabs) = window.tabbedWindows() {
                for w in tabs.iter().filter(|w| !std::ptr::eq(&**w, &*window)) {
                    w.performClose(None);
                }
            }
        }),
    ]
}

pub(super) fn command_and_link_steps() -> Vec<Step> {
    vec![
        step(0.5, |c| {
            let e = &c.e;
            e.window().unwrap().makeKeyAndOrderFront(None);
            append(e, "\n\n## Neue Formate\n\nEin wichtiges Wort hier.\n");
            select_in_source(e, "wichtiges");
            perform(e, sel!(mdHighlight:), 0);
            check(
                "highlight wraps the selection in <mark>",
                source(e).contains("Ein <mark>wichtiges</mark> Wort"),
                &tail(&source(e), 60),
            );
            select_in_source(e, "Wort hier");
            perform(e, sel!(mdInsertAlert:), 3);
            check(
                "alert box wraps the paragraph",
                source(e).contains("> [!WARNING]\n> Ein <mark>"),
                &tail(&source(e), 80),
            );
        }),
        step(0.3, |c| {
            let e = &c.e;
            let p = storage(e.preview()).string().to_string();
            check(
                "alert box renders its German title",
                p.contains("\u{FFFC} Warnung\n"),
                &format!("{:?}", tail(&p, 80)),
            );
            let end = source(e).len();
            set_source_caret(e, end);
            perform(e, sel!(mdCodeBlock:), 13);
            check(
                "code block with a language",
                source(e).contains("```python\n"),
                &tail(&source(e), 40),
            );
            type_in_source(e, "def f(x):  # Kommentar\n    return 42");
            let end = source(e).len();
            set_source_caret(e, end);
            perform(e, sel!(mdInsertToc:), 0);
            let s = source(e);
            check(
                "table of contents links the headings",
                s.contains("- [Neue Formate](#neue-formate)") && s.contains("](#tabellen)"),
                &tail(&s, 120),
            );
            e.reveal_heading("tabellen");
            let at = Utf16Map::new(&s).to_byte(crate::markdown::text::U16(e.source().selectedRange().location));
            check(
                "anchor link jumps to its heading",
                s[at..].starts_with("## Tabellen"),
                &head(&s[at..], 20),
            );
        }),
        step(0.3, |c| {
            let e = &c.e;
            set_source_caret(e, 0);
            let Some(at) = preview_link(e, "#tabellen") else {
                check(
                    "⌘-click on a link to a heading jumps there",
                    false,
                    "no link to #tabellen in the preview",
                );
                return;
            };
            e.preview().scrollRangeToVisible(NSRange::new(at, 1));
            click_preview(e, at, objc2_app_kit::NSEventModifierFlags::Command);
        }),
        // AppKit reports a click on a link once a double click can no longer follow.
        step(1.0, |c| {
            let s = source(&c.e);
            let at = Utf16Map::new(&s).to_byte(crate::markdown::text::U16(c.e.source().selectedRange().location));
            check(
                "⌘-click on a link to a heading jumps there",
                s[at..].starts_with("## Tabellen"),
                &head(&s[at..], 20),
            );
        }),
    ]
}

pub(super) fn building_block_steps() -> Vec<Step> {
    vec![
        step(0.5, |c| {
            let e = &c.e;
            let window = e.window().unwrap();
            window.makeFirstResponder(Some(e.preview()));
            if let Some(b) = storage(e.preview()).string().to_string().find("def f") {
                let idx = storage(e.preview()).string().to_string()[..b].encode_utf16().count();
                e.preview().scrollRangeToVisible(NSRange::new(idx, 0));
            }
            window_image(&window, &c.out.join("08-formats.png"));
            let Some(view) = open_formula_editor(e) else {
                check("formula building blocks", false, "no popover");
                return;
            };
            e.set_atom_visual(false);
            unsafe { view.selectAll(None) };
            e.atom_snippet(0);
            check(
                "fraction button inserts a template with the selection",
                view.string().to_string() == "\\frac{E = mc^2}{□}" && source(e).contains("$\\frac{E = mc^2}{□}$"),
                &view.string().to_string(),
            );
            view.setSelectedRange(NSRange::new(0, 0));
            let jumped = e.atom_tab(false);
            let sel = view.selectedRange();
            check(
                "tab selects the next placeholder",
                jumped && sel.length == 1,
                &format!("{sel:?}"),
            );
            e.set_atom_visual(true);
        }),
        when(
            |c| super::super::mathfield::field(c.e.mtm()).loaded.get(),
            |c| {
                let e = &c.e;
                let field = super::super::mathfield::field(e.mtm());
                check("visual formula editor loads", field.loaded.get(), "");
                field.run(
                    "mf.value = latex; mf.dispatchEvent(new Event('input', { bubbles: true }));",
                    "x^2+\\sqrt{y}",
                );
            },
        ),
        step(0.8, |c| {
            let e = &c.e;
            if let Some(w) = super::super::mathfield::field(e.mtm()).web.window() {
                screen_image(&w, &c.out.join("11-visual-formula.png"));
            }
            check(
                "visual edits reach the document",
                source(e).contains("$x^2+\\sqrt{y}$"),
                &tail(&source(e), 40),
            );
        }),
        step(0.5, |c| c.e.cancel_atom_edit()),
        closed(|c| {
            let e = &c.e;
            check("escape restores the formula", source(e).contains("$E = mc^2$"), "");
            let s = source(e);
            let at = s.find("flowchart LR").unwrap();
            e.window().unwrap().makeFirstResponder(Some(e.source()));
            set_source_caret(e, at + 2);
            e.edit_atom_at_selection();
            let Some(view) = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.view.clone()) else {
                check("diagram building blocks", false, "no popover");
                return;
            };
            let len = view.string().length();
            view.setSelectedRange(NSRange::new(len, 0));
            e.atom_snippet(super::super::atom::diagram_tag(
                crate::markdown::snippets::DiagramAction::Edge,
            ));
            check(
                "connection button continues from the last node",
                view.string().to_string().ends_with("C --> D[Text]"),
                &view.string().to_string(),
            );
        }),
        step(0.3, |c| {
            let e = &c.e;
            if let Some(w) = e.ivars().atoms.edit.borrow().as_ref().and_then(|a| a.view.window()) {
                window_image(&w, &c.out.join("10-diagram-popover.png"));
            }
            let view = e.ivars().atoms.edit.borrow().as_ref().map(|a| a.view.clone());
            if let Some(view) = view {
                view.setString(&ns("flowchart LR\n    A --> "));
                e.atom_text_changed();
            }
        }),
        settle(|c| {
            let e = &c.e;
            let status = e
                .ivars()
                .atoms
                .edit
                .borrow()
                .as_ref()
                .map(|a| a.status.stringValue().to_string())
                .unwrap_or_default();
            check(
                "diagram popover shows the Mermaid error",
                status.starts_with("⚠︎"),
                &status,
            );
            e.cancel_atom_edit();
        }),
    ]
}

pub(super) fn empty_line_and_caret_steps() -> Vec<Step> {
    vec![
        step(0.6, |c| {
            let e = &c.e;
            append(e, "\n\nLeerzeilen Absatz\n");
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            let p = storage(e.preview()).string().to_string();
            let b = p.rfind("Leerzeilen Absatz").unwrap();
            let end = p[..b].encode_utf16().count() + "Leerzeilen Absatz".encode_utf16().count();
            e.preview().setSelectedRange(NSRange::new(end, 0));
            unsafe { e.preview().doCommandBySelector(sel!(insertNewline:)) };
        }),
        step(0.15, |c| unsafe {
            c.e.preview().doCommandBySelector(sel!(insertNewline:))
        }),
        step(0.15, |c| {
            let e = &c.e;
            let s = source(e);
            let p = storage(e.preview()).string().to_string();
            let lines_after = p[p.rfind("Leerzeilen Absatz").unwrap()..].matches('\n').count();
            check(
                "return twice in the preview leaves two empty lines",
                s.ends_with("Leerzeilen Absatz\n\n\n\n\n") && lines_after >= 3,
                &format!("{:?} / {lines_after} line ends", tail(&s, 30)),
            );
            unsafe { e.preview().doCommandBySelector(sel!(deleteBackward:)) };
            type_in_preview(e, "Danach");
            let s = source(e);
            check(
                "backspace removes one empty line, typing fills the other",
                s.ends_with("Leerzeilen Absatz\n\nDanach\n"),
                &format!("{:?}", tail(&s, 40)),
            );
        }),
        step(0.3, |c| {
            let e = &c.e;
            e.window().unwrap().makeFirstResponder(Some(e.preview()));
            let p = storage(e.preview()).string().to_string();
            let at = p[..p.find("Inline-Formel: ").unwrap()].encode_utf16().count() + "Inline-Formel: ".len();
            e.preview().setSelectedRange(NSRange::new(at + 1, 0));
            check(
                "edit command is available right behind a formula",
                e.validate(Some(sel!(mdEditAtom:)), 0, None) && e.ivars().atoms.edit.borrow().is_none(),
                "",
            );
            e.preview().setSelectedRange(NSRange::new(at, 0));
        }),
        step(0.3, |c| {
            let e = &c.e;
            check(
                "caret on a formula opens its editor",
                e.ivars().atoms.edit.borrow().is_some(),
                "",
            );
            e.cancel_atom_edit();
        }),
        step(0.6, |c| {
            let e = &c.e;
            let at = e.preview().selectedRange().location;
            e.preview().setSelectedRange(NSRange::new(at, 0));
        }),
        step(0.3, |c| {
            let e = &c.e;
            check(
                "a closed editor does not reopen while the caret stays",
                e.ivars().atoms.edit.borrow().is_none(),
                "",
            );
        }),
        step(0.3, |c| {
            if let Some(document) = c.e.document() {
                unsafe { document.saveDocument(None) };
            }
        }),
        when(
            |c| c.e.document().is_some_and(|d| !d.isDocumentEdited()),
            |c| {
                let e = &c.e;
                let s = source(e);
                set_source_caret(e, s.find("## Tabellen").unwrap_or(0));
                let path = std::env::var("SPLAT_SELFTEST_FILE").unwrap_or_default();
                let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
                // Written without file coordination, as Git or a shell would.
                let _ = std::fs::write(&path, format!("{on_disk}\nVon außen geändert\n"));
            },
        ),
    ]
}

thread_local! {
    /// Whether the preview and the outline showed before they were toggled.
    static PANES_BEFORE: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((false, false)) };
}

pub(super) fn reload_and_hidden_preview_steps() -> Vec<Step> {
    vec![
        when(
            |c| source(&c.e).contains("Von außen geändert"),
            |c| {
                let e = &c.e;
                let s = source(e);
                let at = Utf16Map::new(&s).to_byte(crate::markdown::text::U16(e.source().selectedRange().location));
                check(
                    "an unedited document reloads when another program changes its file",
                    s.ends_with("Von außen geändert\n"),
                    &tail(&s, 40),
                );
                check(
                    "reloading keeps the caret",
                    s[at..].starts_with("## Tabellen"),
                    &head(&s[at..], 20),
                );
                check(
                    "reloading leaves no undo steps into the old text",
                    e.document().and_then(|d| d.undoManager()).is_some_and(|u| !u.canUndo()),
                    "",
                );
                let Some(view) = open_formula_editor(e) else {
                    check("an open formula edit counts as unsaved", false, "no popover");
                    return;
                };
                view.setString(&ns("x"));
                e.atom_text_changed();
                check(
                    "an open formula edit counts as unsaved",
                    e.document().is_some_and(|d| d.isDocumentEdited()),
                    "",
                );
                e.atom_command(sel!(cancelOperation:));
            },
        ),
        closed(|c| {
            check(
                "a discarded formula edit leaves the document unedited",
                c.e.document().is_some_and(|d| !d.isDocumentEdited()),
                "",
            );
            c.e.toggle_preview();
        }),
        step(0.8, |c| {
            let e = &c.e;
            set_source_caret(e, 0);
            type_in_source(e, "Äußerst übersichtlich: ");
        }),
        step(0.5, |c| c.e.toggle_preview()),
        settle(|c| {
            let e = &c.e;
            let preview = storage(e.preview()).string().to_string();
            check(
                "a preview shown again has the text typed while it was hidden",
                preview.starts_with("Äußerst übersichtlich: "),
                &head(&preview, 40),
            );
            check(
                "scroll positions follow the text typed while the preview was hidden",
                e.scroll_spans().is_some_and(|spans| !spans.is_empty()),
                "",
            );
            // Graphviz lays out this graph for minutes; the renderer must give up and render the next formula.
            let edges: String = (0..6000)
                .map(|i| format!("n{} -> n{};", (i * 7919) % 600, (i * 104_729 + 13) % 600))
                .collect();
            super::super::js::renderer(e.mtm()).request(vec![super::super::js::Key::Diagram {
                source: format!("digraph {{ {edges} }}"),
                kind: crate::markdown::doc::DiagramKind::Graphviz,
                dark: false,
            }]);
        }),
        step(12.0, |c| {
            super::super::js::renderer(c.e.mtm()).request(vec![hang_check_formula()]);
        }),
        when(
            |_| super::super::js::outcome(&hang_check_formula()).is_some(),
            |_| {
                let outcome = super::super::js::outcome(&hang_check_formula());
                check(
                    "formulas render again after a diagram that never finishes",
                    outcome == Some(Ok(())),
                    &format!("{outcome:?}"),
                );
            },
        ),
        step(0.1, |c| {
            let e = &c.e;
            PANES_BEFORE.set((e.preview_visible(), e.outline_visible()));
            e.toggle_preview();
            e.toggle_preview();
            e.toggle_outline();
            e.toggle_outline();
        }),
        step(1.0, |c| {
            let now = (c.e.preview_visible(), c.e.outline_visible());
            check(
                "panes toggled twice in a moment end as they were",
                now == PANES_BEFORE.get(),
                &format!("{:?} -> {now:?}", PANES_BEFORE.get()),
            );
            c.e.toggle_preview();
        }),
        step(0.05, |c| c.e.toggle_preview()),
        step(1.0, |c| {
            let now = c.e.preview_visible();
            check(
                "a pane toggled twice during its animation ends as it was",
                now == PANES_BEFORE.get().0,
                "",
            );
        }),
    ]
}
