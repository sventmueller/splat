use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSApplication, NSMenu};

use crate::editor::actions::{self, Action, item_with_symbol, with_symbol};
use crate::i18n::t;
use crate::util::{item, ns, separator, submenu, tagged};

pub fn install(app: &NSApplication, mtm: MainThreadMarker) {
    let main = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(""));

    let (services_item, services) = submenu(mtm, t("Dienste"), vec![]);
    let (app_item, _) = submenu(
        mtm,
        "Splat",
        vec![
            item(mtm, t("Über Splat"), Some(sel!(orderFrontStandardAboutPanel:)), ""),
            separator(mtm),
            services_item,
            separator(mtm),
            item(mtm, t("Splat ausblenden"), Some(sel!(hide:)), "cmd+h"),
            item(
                mtm,
                t("Andere ausblenden"),
                Some(sel!(hideOtherApplications:)),
                "alt+cmd+h",
            ),
            item(mtm, t("Alle einblenden"), Some(sel!(unhideAllApplications:)), ""),
            separator(mtm),
            item(mtm, t("Splat beenden"), Some(sel!(terminate:)), "cmd+q"),
        ],
    );
    app.setServicesMenu(Some(&services));

    let (revert_item, _) = submenu(
        mtm,
        t("Zurücksetzen auf"),
        vec![
            item(
                mtm,
                t("Zuletzt gesicherte Version"),
                Some(sel!(revertDocumentToSaved:)),
                "",
            ),
            item(
                mtm,
                t("Alle Versionen durchsuchen …"),
                Some(sel!(browseDocumentVersions:)),
                "",
            ),
        ],
    );
    let (file_item, _) = submenu(
        mtm,
        t("Ablage"),
        vec![
            item(mtm, t("Neu"), Some(sel!(newDocument:)), "cmd+n"),
            actions::item(mtm, Action::NewTab),
            // NSDocumentController adds the localized "Open Recent" submenu after this item itself.
            item(mtm, t("Öffnen …"), Some(sel!(openDocument:)), "cmd+o"),
            separator(mtm),
            item(mtm, t("Schließen"), Some(sel!(performClose:)), "cmd+w"),
            item(mtm, t("Sichern"), Some(sel!(saveDocument:)), "cmd+s"),
            item(mtm, t("Duplizieren"), Some(sel!(duplicateDocument:)), "shift+cmd+s"),
            item(mtm, t("Umbenennen …"), Some(sel!(renameDocument:)), ""),
            item(mtm, t("Bewegen …"), Some(sel!(moveDocument:)), ""),
            revert_item,
            separator(mtm),
            objc2_app_kit::NSDocumentController::sharedDocumentController(mtm).standardShareMenuItem(),
            item(mtm, t("Als PDF exportieren …"), Some(sel!(saveDocumentToPDF:)), ""),
            actions::item(mtm, Action::ExportHtml),
            actions::item(mtm, Action::ExportWord),
            separator(mtm),
            item(mtm, t("Papierformat …"), Some(sel!(runPageLayout:)), "shift+cmd+p"),
            item(mtm, t("Drucken …"), Some(sel!(printDocument:)), "cmd+p"),
        ],
    );

    let find = |title: &str, tag: isize, key: &str| tagged(mtm, title, sel!(performTextFinderAction:), key, tag);
    let (find_item, _) = submenu(
        mtm,
        t("Suchen"),
        vec![
            find(t("Suchen …"), 1, "cmd+f"),
            find(t("Suchen und ersetzen …"), 12, "alt+cmd+f"),
            find(t("Weitersuchen"), 2, "cmd+g"),
            find(t("Rückwärts suchen"), 3, "shift+cmd+g"),
            find(t("Auswahl suchen"), 7, "cmd+e"),
            item(
                mtm,
                t("Auswahl anzeigen"),
                Some(sel!(centerSelectionInVisibleArea:)),
                "cmd+j",
            ),
        ],
    );
    let (spelling_item, _) = submenu(
        mtm,
        t("Rechtschreibung und Grammatik"),
        vec![
            item(
                mtm,
                t("Rechtschreibung und Grammatik einblenden"),
                Some(sel!(showGuessPanel:)),
                "cmd+:",
            ),
            item(mtm, t("Dokument jetzt prüfen"), Some(sel!(checkSpelling:)), "cmd+;"),
            separator(mtm),
            item(
                mtm,
                t("Rechtschreibung während der Texteingabe prüfen"),
                Some(sel!(toggleContinuousSpellChecking:)),
                "",
            ),
            item(
                mtm,
                t("Grammatik mit Rechtschreibung prüfen"),
                Some(sel!(toggleGrammarChecking:)),
                "",
            ),
            item(
                mtm,
                t("Rechtschreibung automatisch korrigieren"),
                Some(sel!(toggleAutomaticSpellingCorrection:)),
                "",
            ),
        ],
    );
    let (substitutions_item, _) = submenu(
        mtm,
        t("Ersetzungen"),
        vec![
            item(
                mtm,
                t("Ersetzungen einblenden"),
                Some(sel!(orderFrontSubstitutionsPanel:)),
                "",
            ),
            separator(mtm),
            item(
                mtm,
                t("Intelligentes Kopieren/Einsetzen"),
                Some(sel!(toggleSmartInsertDelete:)),
                "",
            ),
            item(
                mtm,
                t("Intelligente Anführungszeichen"),
                Some(sel!(toggleAutomaticQuoteSubstitution:)),
                "",
            ),
            item(
                mtm,
                t("Intelligente Bindestriche"),
                Some(sel!(toggleAutomaticDashSubstitution:)),
                "",
            ),
            item(
                mtm,
                t("Intelligente Links"),
                Some(sel!(toggleAutomaticLinkDetection:)),
                "",
            ),
            item(mtm, t("Textersetzung"), Some(sel!(toggleAutomaticTextReplacement:)), ""),
        ],
    );
    let (transform_item, _) = submenu(
        mtm,
        t("Transformationen"),
        vec![
            item(mtm, t("In Großbuchstaben"), Some(sel!(uppercaseWord:)), ""),
            item(mtm, t("In Kleinbuchstaben"), Some(sel!(lowercaseWord:)), ""),
            item(mtm, t("Großschreibung"), Some(sel!(capitalizeWord:)), ""),
        ],
    );
    let (speech_item, _) = submenu(
        mtm,
        t("Sprachausgabe"),
        vec![
            item(mtm, t("Sprachausgabe starten"), Some(sel!(startSpeaking:)), ""),
            item(mtm, t("Sprachausgabe stoppen"), Some(sel!(stopSpeaking:)), ""),
        ],
    );
    let (edit_item, _) = submenu(
        mtm,
        t("Bearbeiten"),
        vec![
            item(mtm, t("Widerrufen"), Some(sel!(undo:)), "cmd+z"),
            item(mtm, t("Wiederholen"), Some(sel!(redo:)), "shift+cmd+z"),
            separator(mtm),
            item(mtm, t("Ausschneiden"), Some(sel!(cut:)), "cmd+x"),
            item(mtm, t("Kopieren"), Some(sel!(copy:)), "cmd+c"),
            item(mtm, t("Einsetzen"), Some(sel!(paste:)), "cmd+v"),
            item(
                mtm,
                t("Einsetzen und Stil anpassen"),
                Some(sel!(pasteAsPlainText:)),
                "alt+shift+cmd+v",
            ),
            item(mtm, t("Löschen"), Some(sel!(delete:)), ""),
            item(mtm, t("Alles auswählen"), Some(sel!(selectAll:)), "cmd+a"),
            separator(mtm),
            find_item,
            spelling_item,
            substitutions_item,
            transform_item,
            speech_item,
        ],
    );

    let (font_item, _) = submenu(
        mtm,
        t("Schrift"),
        [
            Action::Bold,
            Action::Italic,
            Action::Underline,
            Action::Strikethrough,
            Action::Highlight,
            Action::InlineCode,
            Action::Superscript,
            Action::Subscript,
        ]
        .map(|a| item_with_symbol(mtm, a))
        .to_vec(),
    );
    let (heading_item, _) = submenu(mtm, t("Überschrift"), actions::heading_items(mtm, true));
    let (list_item, _) = submenu(
        mtm,
        t("Liste"),
        [Action::BulletList, Action::NumberedList, Action::TaskList]
            .map(|a| item_with_symbol(mtm, a))
            .to_vec(),
    );
    let (format_item, _) = submenu(
        mtm,
        t("Format"),
        vec![
            font_item,
            heading_item,
            list_item,
            separator(mtm),
            actions::item(mtm, Action::Quote),
            submenu(mtm, t("Codeblock"), actions::code_block_items(mtm)).0,
            submenu(mtm, t("Hinweisbox"), actions::alert_items(mtm)).0,
            separator(mtm),
            actions::item(mtm, Action::EditAtom),
            separator(mtm),
            actions::item(mtm, Action::RemoveFormatting),
        ],
    );

    let (view_item, _) = submenu(
        mtm,
        t("Darstellung"),
        vec![
            actions::item(mtm, Action::TogglePreview),
            actions::item(mtm, Action::ToggleOutline),
            separator(mtm),
            actions::item(mtm, Action::Typography),
            separator(mtm),
            actions::item(mtm, Action::ZoomIn),
            actions::item(mtm, Action::ZoomOut),
            actions::item(mtm, Action::ZoomReset),
            separator(mtm),
            item(
                mtm,
                t("Symbolleiste ausblenden"),
                Some(sel!(toggleToolbarShown:)),
                "alt+cmd+t",
            ),
            item(
                mtm,
                t("Symbolleiste anpassen …"),
                Some(sel!(runToolbarCustomizationPalette:)),
                "",
            ),
            separator(mtm),
            item(mtm, t("Tableiste einblenden"), Some(sel!(toggleTabBar:)), ""),
            item(
                mtm,
                t("Alle Tabs einblenden"),
                Some(sel!(toggleTabOverview:)),
                "shift+cmd+\\",
            ),
            separator(mtm),
            item(mtm, t("Vollbildmodus"), Some(sel!(toggleFullScreen:)), "ctrl+cmd+f"),
        ],
    );

    let (diagram_item, _) = submenu(mtm, t("Diagramm"), actions::diagram_items(mtm));
    let (insert_item, _) = submenu(
        mtm,
        t("Einfügen"),
        vec![
            item_with_symbol(mtm, Action::Link),
            item_with_symbol(mtm, Action::Image),
            item_with_symbol(mtm, Action::Table),
            item_with_symbol(mtm, Action::Formula),
            with_symbol(diagram_item, Action::Diagram.spec().symbol),
            item_with_symbol(mtm, Action::Toc),
            item_with_symbol(mtm, Action::Footnote),
            item_with_symbol(mtm, Action::Rule),
        ],
    );

    let (align_item, _) = submenu(mtm, t("Ausrichtung"), actions::align_items(mtm));
    let (table_item, _) = submenu(
        mtm,
        t("Tabelle"),
        vec![
            actions::item(mtm, Action::RowAbove),
            actions::item(mtm, Action::RowBelow),
            actions::item(mtm, Action::ColumnLeft),
            actions::item(mtm, Action::ColumnRight),
            separator(mtm),
            actions::item(mtm, Action::DeleteRow),
            actions::item(mtm, Action::DeleteColumn),
            separator(mtm),
            align_item,
            actions::item(mtm, Action::TableFormat),
        ],
    );

    let (window_item, window_menu) = submenu(
        mtm,
        t("Fenster"),
        vec![
            item(mtm, t("Im Dock ablegen"), Some(sel!(performMiniaturize:)), "cmd+m"),
            item(mtm, t("Zoomen"), Some(sel!(performZoom:)), ""),
            separator(mtm),
            item(
                mtm,
                t("Vorherigen Tab zeigen"),
                Some(sel!(selectPreviousTab:)),
                "ctrl+shift+\t",
            ),
            item(mtm, t("Nächsten Tab zeigen"), Some(sel!(selectNextTab:)), "ctrl+\t"),
            item(
                mtm,
                t("Tab in neues Fenster bewegen"),
                Some(sel!(moveTabToNewWindow:)),
                "",
            ),
            item(mtm, t("Alle Fenster zusammenführen"), Some(sel!(mergeAllWindows:)), ""),
            separator(mtm),
            item(mtm, t("Alle nach vorne bringen"), Some(sel!(arrangeInFront:)), ""),
        ],
    );
    let (help_item, help_menu) = submenu(
        mtm,
        t("Hilfe"),
        vec![item(mtm, t("Markdown-Kurzreferenz"), Some(sel!(mdQuickReference:)), "")],
    );

    for i in [
        app_item,
        file_item,
        edit_item,
        format_item,
        view_item,
        insert_item,
        table_item,
        window_item,
        help_item,
    ] {
        main.addItem(&i);
    }
    app.setMainMenu(Some(&main));
    app.setWindowsMenu(Some(&window_menu));
    app.setHelpMenu(Some(&help_menu));
}
