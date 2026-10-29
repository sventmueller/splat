//! Every editor command in one place. The menu bar, the toolbar and the context menu build their items from here,
//! and menu validation reads here when a command applies and whether it shows a checkmark.

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSImage, NSMenuItem};

use crate::i18n::{n, t, tf};
use crate::markdown::code;
use crate::markdown::commands::{Active, ListStyle};
use crate::markdown::doc::AlertKind;
use crate::util::ns;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Highlight,
    InlineCode,
    Superscript,
    Subscript,
    Heading,
    BulletList,
    NumberedList,
    TaskList,
    Quote,
    CodeBlock,
    Alert,
    RemoveFormatting,
    EditAtom,
    Link,
    Image,
    Table,
    Formula,
    Diagram,
    Toc,
    Footnote,
    Rule,
    RowAbove,
    RowBelow,
    ColumnLeft,
    ColumnRight,
    DeleteRow,
    DeleteColumn,
    TableAlign,
    TableFormat,
    TogglePreview,
    ToggleOutline,
    Typography,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    NewTab,
    LoadRemoteImages,
    ExportHtml,
    ExportWord,
    /// Targets of controls in the editor window rather than menu items of their own.
    EditAtomAt,
    AtomSnippet,
    AtomMode,
    ToolbarSegment,
}

/// When a command can be chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applies {
    Always,
    /// The source or the preview has the focus.
    InText,
    /// Inline formatting: in text, but not inside a code block.
    OutsideCode,
    InTable,
    OnFormulaOrDiagram,
}

pub struct Spec {
    pub selector: Sel,
    /// Menu title, untranslated.
    pub title: &'static str,
    /// Shorter toolbar label, untranslated; empty when it is the title.
    pub label: &'static str,
    pub tip: &'static str,
    pub key: &'static str,
    /// SF Symbol for the toolbar and for menu items that show one.
    pub symbol: &'static str,
    pub applies: Applies,
}

impl Action {
    pub const ALL: [Action; 47] = {
        use Action::*;
        [
            Bold,
            Italic,
            Underline,
            Strikethrough,
            Highlight,
            InlineCode,
            Superscript,
            Subscript,
            Heading,
            BulletList,
            NumberedList,
            TaskList,
            Quote,
            CodeBlock,
            Alert,
            RemoveFormatting,
            EditAtom,
            Link,
            Image,
            Table,
            Formula,
            Diagram,
            Toc,
            Footnote,
            Rule,
            RowAbove,
            RowBelow,
            ColumnLeft,
            ColumnRight,
            DeleteRow,
            DeleteColumn,
            TableAlign,
            TableFormat,
            TogglePreview,
            ToggleOutline,
            Typography,
            ZoomIn,
            ZoomOut,
            ZoomReset,
            NewTab,
            LoadRemoteImages,
            ExportHtml,
            ExportWord,
            EditAtomAt,
            AtomSnippet,
            AtomMode,
            ToolbarSegment,
        ]
    };

    #[rustfmt::skip]
    pub fn spec(self) -> Spec {
        use Action::*;
        use Applies::*;
        let (selector, title, label, tip, key, symbol, applies) = match self {
            Bold => (sel!(mdBold:), n("Fett"), "", n("Markierten Text fett formatieren"), "cmd+b", "bold", OutsideCode),
            Italic => (sel!(mdItalic:), n("Kursiv"), "", n("Markierten Text kursiv formatieren"), "cmd+i", "italic", OutsideCode),
            Underline => (sel!(mdUnderline:), n("Unterstrichen"), "", n("Markierten Text unterstreichen"), "cmd+u", "underline", OutsideCode),
            Strikethrough => (sel!(mdStrikethrough:), n("Durchgestrichen"), "", n("Markierten Text durchstreichen"), "shift+cmd+x", "strikethrough", OutsideCode),
            Highlight => (sel!(mdHighlight:), n("Textmarker"), "", n("Markierten Text farbig hervorheben"), "", "highlighter", OutsideCode),
            InlineCode => (sel!(mdInlineCode:), n("Code"), "", n("Markierten Text als Code formatieren"), "", "chevron.left.forwardslash.chevron.right", OutsideCode),
            Superscript => (sel!(mdSuperscript:), n("Hochgestellt"), "", n("Markierten Text hochstellen"), "", "textformat.superscript", InText),
            Subscript => (sel!(mdSubscript:), n("Tiefgestellt"), "", n("Markierten Text tiefstellen"), "", "textformat.subscript", InText),
            Heading => (sel!(mdHeading:), n("Überschrift"), "", n("Absatz als Überschrift oder Fließtext formatieren"), "", "h.square", InText),
            BulletList => (sel!(mdBulletList:), n("Aufzählung"), "", "", "", "list.bullet", InText),
            NumberedList => (sel!(mdNumberedList:), n("Nummerierung"), "", "", "", "list.number", InText),
            TaskList => (sel!(mdTaskList:), n("Aufgabenliste"), "", "", "", "checklist", InText),
            Quote => (sel!(mdQuote:), n("Zitat"), "", n("Absätze als Zitat formatieren"), "", "text.quote", InText),
            CodeBlock => (sel!(mdCodeBlock:), n("Codeblock"), "", n("Codeblock in einer Programmiersprache einfügen"), "", "curlybraces", InText),
            Alert => (sel!(mdInsertAlert:), n("Hinweisbox"), "", n("Absatz als Hinweis-, Tipp- oder Warnbox formatieren"), "", "info.bubble", InText),
            RemoveFormatting => (sel!(mdRemoveFormatting:), n("Formatierung entfernen"), "", n("Formatierung der Auswahl entfernen"), "", "eraser", InText),
            EditAtom => (sel!(mdEditAtom:), n("Formel oder Diagramm bearbeiten …"), "", "", "", "", OnFormulaOrDiagram),
            Link => (sel!(mdInsertLink:), n("Link …"), n("Link"), n("Link einfügen"), "cmd+k", "link", InText),
            Image => (sel!(mdInsertImage:), n("Bild …"), n("Bild"), n("Bild aus einer Datei einfügen"), "", "photo", InText),
            Table => (sel!(mdInsertTable:), n("Tabelle …"), n("Tabelle"), n("Tabelle einfügen"), "", "tablecells", InText),
            Formula => (sel!(mdInsertFormula:), n("Formel"), "", n("LaTeX-Formel einfügen"), "", "function", InText),
            Diagram => (sel!(mdInsertDiagram:), n("Diagramm"), "", n("Mermaid-Diagramm einfügen"), "", "flowchart", InText),
            Toc => (sel!(mdInsertToc:), n("Inhaltsverzeichnis"), "", n("Inhaltsverzeichnis einfügen oder aktualisieren"), "", "list.bullet.rectangle.portrait", InText),
            Footnote => (sel!(mdInsertFootnote:), n("Fußnote"), "", n("Fußnote einfügen"), "", "text.append", InText),
            Rule => (sel!(mdInsertRule:), n("Trennlinie"), "", n("Trennlinie einfügen"), "", "minus", InText),
            RowAbove => (sel!(mdTableRowAbove:), n("Zeile darüber einfügen"), "", "", "", "", InTable),
            RowBelow => (sel!(mdTableRowBelow:), n("Zeile darunter einfügen"), "", "", "", "", InTable),
            ColumnLeft => (sel!(mdTableColumnLeft:), n("Spalte links einfügen"), "", "", "", "", InTable),
            ColumnRight => (sel!(mdTableColumnRight:), n("Spalte rechts einfügen"), "", "", "", "", InTable),
            DeleteRow => (sel!(mdTableDeleteRow:), n("Zeile löschen"), "", "", "", "", InTable),
            DeleteColumn => (sel!(mdTableDeleteColumn:), n("Spalte löschen"), "", "", "", "", InTable),
            TableAlign => (sel!(mdTableAlign:), n("Ausrichtung"), "", "", "", "", InTable),
            TableFormat => (sel!(mdTableFormat:), n("Tabelle bündig ausrichten"), "", "", "", "", InTable),
            TogglePreview => (sel!(mdTogglePreview:), n("Vorschau ausblenden"), n("Vorschau"), n("Vorschau ein- oder ausblenden"), "alt+cmd+p", "rectangle.split.2x1", Always),
            ToggleOutline => (sel!(mdToggleOutline:), n("Gliederung einblenden"), "", "", "ctrl+cmd+s", "sidebar.left", Always),
            Typography => (sel!(mdTypography:), n("Typografische Satzzeichen"), "", "", "", "", Always),
            ZoomIn => (sel!(mdZoomIn:), n("Vergrößern"), "", "", "cmd++", "", Always),
            ZoomOut => (sel!(mdZoomOut:), n("Verkleinern"), "", "", "cmd+-", "", Always),
            ZoomReset => (sel!(mdZoomReset:), n("Originalgröße"), "", "", "cmd+0", "", Always),
            NewTab => (sel!(mdNewTab:), n("Neuer Tab"), n("Neues Dokument"), n("Neues Dokument in einem Tab öffnen"), "", "plus", Always),
            LoadRemoteImages => (sel!(mdLoadRemoteImages:), n("Bilder laden"), "", "", "", "", Always),
            ExportHtml => (sel!(mdExportHTML:), n("Als HTML exportieren …"), "", "", "", "", Always),
            ExportWord => (sel!(mdExportWord:), n("Als Word-Dokument exportieren …"), "", "", "", "", Always),
            EditAtomAt => (sel!(mdEditAtomAt:), "", "", "", "", "", Always),
            AtomSnippet => (sel!(mdAtomSnippet:), "", "", "", "", "", Always),
            AtomMode => (sel!(mdAtomMode:), "", "", "", "", "", Always),
            ToolbarSegment => (sel!(mdToolbarSegment:), "", "", "", "", "", Always),
        };
        Spec { selector, title, label, tip, key, symbol, applies }
    }

    pub fn from_selector(selector: Sel) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.spec().selector == selector)
    }

    /// Whether the menu item for this command and `tag` shows a checkmark at the formatting in `active`; `None`
    /// for commands without one.
    pub fn checked(self, active: &Active, tag: isize) -> Option<bool> {
        use Action::*;
        Some(match self {
            Bold => active.bold,
            Italic => active.italic,
            Underline => active.underline,
            Strikethrough => active.strikethrough,
            Highlight => active.highlight,
            InlineCode => active.code,
            Superscript => active.superscript,
            Subscript => active.subscript,
            Heading => active.heading as isize == tag,
            BulletList => active.list == Some(ListStyle::Bullet),
            NumberedList => active.list == Some(ListStyle::Numbered),
            TaskList => active.list == Some(ListStyle::Task),
            Quote => active.quote,
            CodeBlock => active.code_lang.as_deref() == Some(code_language(tag)),
            Alert => active.alert.is_some() && active.alert == alert_kind(tag),
            _ => return None,
        })
    }
}

/// Info string for the tag of a code block menu item, see `code_block_items`.
pub fn code_language(tag: isize) -> &'static str {
    usize::try_from(tag - 1)
        .ok()
        .and_then(|i| code::MENU.get(i))
        .map_or("", |(lang, _)| lang)
}

pub fn alert_kind(tag: isize) -> Option<AlertKind> {
    usize::try_from(tag).ok().and_then(|i| AlertKind::ALL.get(i)).copied()
}

/// The menu item of a command, with its shortcut.
pub fn item(mtm: MainThreadMarker, action: Action) -> Retained<NSMenuItem> {
    let spec = action.spec();
    crate::util::item(mtm, t(spec.title), Some(spec.selector), spec.key)
}

/// The menu item of a command with its symbol in front.
pub fn item_with_symbol(mtm: MainThreadMarker, action: Action) -> Retained<NSMenuItem> {
    with_symbol(item(mtm, action), action.spec().symbol)
}

/// The menu item of a command for a context menu, without shortcut.
pub fn context_item(mtm: MainThreadMarker, action: Action) -> Retained<NSMenuItem> {
    let spec = action.spec();
    crate::util::item(mtm, t(spec.title), Some(spec.selector), "")
}

/// A menu item running a command with a parameter in its tag.
pub fn tagged(mtm: MainThreadMarker, action: Action, title: &str, key: &str, tag: isize) -> Retained<NSMenuItem> {
    crate::util::tagged(mtm, title, action.spec().selector, key, tag)
}

pub fn with_symbol(item: Retained<NSMenuItem>, symbol: &str) -> Retained<NSMenuItem> {
    if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), None) {
        item.setImage(Some(&image));
    }
    item
}

/// Body text and the six heading levels; the tag is the level, 0 for body text.
pub fn heading_items(mtm: MainThreadMarker, shortcuts: bool) -> Vec<Retained<NSMenuItem>> {
    let key = |level: isize| {
        if shortcuts {
            format!("alt+cmd+{level}")
        } else {
            String::new()
        }
    };
    let mut items = vec![tagged(mtm, Action::Heading, t("Fließtext"), &key(0), 0)];
    for level in 1..=6 {
        let title = tf("Überschrift {}", &[&level.to_string()]);
        items.push(tagged(mtm, Action::Heading, &title, &key(level), level));
    }
    items
}

/// Code block entries; the tag is 0 for no language, else the position in `code::MENU` plus one.
pub fn code_block_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    let mut items = vec![
        tagged(mtm, Action::CodeBlock, t("Ohne Sprache"), "", 0),
        NSMenuItem::separatorItem(mtm),
    ];
    for (n, (_, title)) in code::MENU.iter().enumerate() {
        items.push(tagged(mtm, Action::CodeBlock, title, "", n as isize + 1));
    }
    items
}

/// Alert entries; the tag is the position in `AlertKind::ALL`.
pub fn alert_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    AlertKind::ALL
        .iter()
        .enumerate()
        .map(|(n, kind)| tagged(mtm, Action::Alert, t(kind.label()), "", n as isize))
        .collect()
}

/// Starter diagrams; the tag picks the template, see `diagram_template`.
pub fn diagram_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    [n("Flussdiagramm"), n("Sequenzdiagramm"), n("Gantt-Diagramm")]
        .iter()
        .enumerate()
        .map(|(tag, title)| tagged(mtm, Action::Diagram, t(title), "", tag as isize))
        .collect()
}

/// Column alignments; the tag is 1 left, 2 centered, 3 right and 0 for none.
pub fn align_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    vec![
        tagged(mtm, Action::TableAlign, t("Links"), "cmd+{", 1),
        tagged(mtm, Action::TableAlign, t("Zentriert"), "cmd+|", 2),
        tagged(mtm, Action::TableAlign, t("Rechts"), "cmd+}", 3),
        tagged(mtm, Action::TableAlign, t("Standard"), "", 0),
    ]
}

/// Starter diagrams of the menus: flowchart, sequence and Gantt.
pub fn diagram_template(tag: isize) -> String {
    match tag {
        0 => format!(
            "flowchart TD\n    A[{}] --> B{{{}}}\n    B -->|{}| C[{}]\n    B -->|{}| D[{}]",
            t("Start"),
            t("Entscheidung"),
            t("Ja"),
            t("Weiter"),
            t("Nein"),
            t("Ende")
        ),
        1 => format!(
            "sequenceDiagram\n    Alice->>Bob: {}\n    Bob-->>Alice: {}",
            t("Anfrage"),
            t("Antwort")
        ),
        _ => format!(
            "gantt\n    title {}\n    dateFormat YYYY-MM-DD\n    section {}\n    {} A :a1, 2026-01-05, 10d\n    {} B :after a1, 5d",
            t("Projektplan"),
            tf("Phase {}", &["1"]),
            t("Aufgabe"),
            t("Aufgabe")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::ClassType;

    /// A selector exists twice: in the registry and as a method of the editor. A typo in either compiles, so the
    /// two are compared here.
    #[test]
    fn every_command_has_a_method_and_every_method_a_command() {
        let class = super::super::EditorController::class();
        for action in Action::ALL {
            let selector = action.spec().selector;
            assert!(
                class.instance_method(selector).is_some(),
                "{action:?} has no method {selector:?}"
            );
            assert_eq!(
                Action::from_selector(selector),
                Some(action),
                "{selector:?} is used twice"
            );
        }
        for method in class.instance_methods().iter() {
            let name = method.name();
            let text = name.name().to_string_lossy();
            if text.starts_with("md") && text.ends_with(':') {
                assert!(
                    Action::from_selector(name).is_some(),
                    "{text} is missing in the registry"
                );
            }
        }
    }

    #[test]
    fn checkmarks_follow_the_formatting_at_the_caret() {
        let active = Active {
            bold: true,
            heading: 2,
            list: Some(ListStyle::Task),
            code_lang: Some(code_language(1).to_string()),
            alert: alert_kind(1),
            ..Active::default()
        };
        assert_eq!(Action::Bold.checked(&active, 0), Some(true));
        assert_eq!(Action::Italic.checked(&active, 0), Some(false));
        assert_eq!(Action::Heading.checked(&active, 2), Some(true));
        assert_eq!(Action::Heading.checked(&active, 1), Some(false));
        assert_eq!(Action::TaskList.checked(&active, 0), Some(true));
        assert_eq!(Action::BulletList.checked(&active, 0), Some(false));
        assert_eq!(Action::CodeBlock.checked(&active, 1), Some(true));
        assert_eq!(Action::Alert.checked(&active, 1), Some(true));
        assert_eq!(Action::Alert.checked(&active, 0), Some(false));
        assert_eq!(Action::Rule.checked(&active, 0), None);
        assert_eq!(alert_kind(-1), None);
        assert_eq!(alert_kind(AlertKind::ALL.len() as isize), None);
    }

    #[test]
    fn diagram_templates_are_the_three_mermaid_starters() {
        for (tag, kind) in [(0, "flowchart"), (1, "sequenceDiagram"), (2, "gantt"), (9, "gantt")] {
            assert!(diagram_template(tag).starts_with(kind), "{tag}");
        }
    }

    #[test]
    fn menu_commands_have_titles() {
        for action in Action::ALL {
            let spec = action.spec();
            let internal = matches!(
                action,
                Action::EditAtomAt | Action::AtomSnippet | Action::AtomMode | Action::ToolbarSegment
            );
            assert_eq!(spec.title.is_empty(), internal, "{action:?}");
        }
    }
}
