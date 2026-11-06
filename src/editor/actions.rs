//! Every editor command in one place. The menu bar, the toolbar and the context menu build their items from here,
//! and menu validation reads here when a command applies and whether it shows a checkmark.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSImage, NSMenuItem};
use objc2_foundation::NSNumber;

use crate::i18n::{n, t, tf};
use crate::markdown::code;
use crate::markdown::commands::{Active, ListStyle};
use crate::markdown::doc::{AlertKind, Align};
use crate::markdown::text::Byte;
use crate::util::ns;

/// Every command once, as `Variant = selector, when it applies { what menus and toolbar show }`, handed to the macro
/// `then` after `pass`. Here the rows become `Action` and its specs; `controller.rs` gives each the method by which
/// AppKit finds the command's target.
macro_rules! commands {
    ($then:ident $(, $pass:tt)?) => {
        $then! {
            $($pass)?
            Bold = mdBold, OutsideCode { title: n("Fett"), tip: n("Markierten Text fett formatieren"), key: "cmd+b", symbol: "bold" };
            Italic = mdItalic, OutsideCode { title: n("Kursiv"), tip: n("Markierten Text kursiv formatieren"), key: "cmd+i", symbol: "italic" };
            Underline = mdUnderline, OutsideCode { title: n("Unterstrichen"), tip: n("Markierten Text unterstreichen"), key: "cmd+u", symbol: "underline" };
            Strikethrough = mdStrikethrough, OutsideCode { title: n("Durchgestrichen"), tip: n("Markierten Text durchstreichen"), key: "shift+cmd+x", symbol: "strikethrough" };
            Highlight = mdHighlight, OutsideCode { title: n("Textmarker"), tip: n("Markierten Text farbig hervorheben"), symbol: "highlighter" };
            InlineCode = mdInlineCode, OutsideCode { title: n("Code"), tip: n("Markierten Text als Code formatieren"), key: "alt+shift+cmd+c", symbol: "chevron.left.forwardslash.chevron.right" };
            Superscript = mdSuperscript, InText { title: n("Hochgestellt"), tip: n("Markierten Text hochstellen"), symbol: "textformat.superscript" };
            Subscript = mdSubscript, InText { title: n("Tiefgestellt"), tip: n("Markierten Text tiefstellen"), symbol: "textformat.subscript" };
            Heading = mdHeading, InText { title: n("Überschrift"), tip: n("Absatz als Überschrift oder Fließtext formatieren"), symbol: "h.square" };
            BulletList = mdBulletList, InText { title: n("Aufzählung"), symbol: "list.bullet" };
            NumberedList = mdNumberedList, InText { title: n("Nummerierung"), symbol: "list.number" };
            TaskList = mdTaskList, InText { title: n("Aufgabenliste"), symbol: "checklist" };
            Quote = mdQuote, InText { title: n("Zitat"), tip: n("Absätze als Zitat formatieren"), symbol: "text.quote" };
            CodeBlock = mdCodeBlock, InText { title: n("Codeblock"), tip: n("Codeblock in einer Programmiersprache einfügen"), symbol: "curlybraces" };
            Alert = mdInsertAlert, InText { title: n("Hinweisbox"), tip: n("Absatz als Hinweis-, Tipp- oder Warnbox formatieren"), symbol: "info.bubble" };
            RemoveFormatting = mdRemoveFormatting, InText { title: n("Formatierung entfernen"), tip: n("Formatierung der Auswahl entfernen"), symbol: "eraser" };
            EditAtom = mdEditAtom, OnFormulaOrDiagram { title: n("Formel oder Diagramm bearbeiten …") };
            Link = mdInsertLink, InText { title: n("Link …"), label: n("Link"), tip: n("Link einfügen"), key: "cmd+k", symbol: "link" };
            Image = mdInsertImage, InText { title: n("Bild …"), label: n("Bild"), tip: n("Bild aus einer Datei einfügen"), symbol: "photo" };
            Table = mdInsertTable, InText { title: n("Tabelle …"), label: n("Tabelle"), tip: n("Tabelle einfügen"), symbol: "tablecells" };
            Formula = mdInsertFormula, InText { title: n("Formel"), tip: n("LaTeX-Formel einfügen"), symbol: "function" };
            Diagram = mdInsertDiagram, InText { title: n("Diagramm"), tip: n("Mermaid-Diagramm einfügen"), symbol: "flowchart" };
            Toc = mdInsertToc, InText { title: n("Inhaltsverzeichnis"), tip: n("Inhaltsverzeichnis einfügen oder aktualisieren"), symbol: "list.bullet.rectangle.portrait" };
            Footnote = mdInsertFootnote, InText { title: n("Fußnote"), tip: n("Fußnote einfügen"), symbol: "text.append" };
            Rule = mdInsertRule, InText { title: n("Trennlinie"), tip: n("Trennlinie einfügen"), symbol: "minus" };
            RowAbove = mdTableRowAbove, InTable { title: n("Zeile darüber einfügen") };
            RowBelow = mdTableRowBelow, InTable { title: n("Zeile darunter einfügen") };
            ColumnLeft = mdTableColumnLeft, InTable { title: n("Spalte links einfügen") };
            ColumnRight = mdTableColumnRight, InTable { title: n("Spalte rechts einfügen") };
            DeleteRow = mdTableDeleteRow, InTable { title: n("Zeile löschen") };
            DeleteColumn = mdTableDeleteColumn, InTable { title: n("Spalte löschen") };
            TableAlign = mdTableAlign, InTable { title: n("Ausrichtung") };
            TableFormat = mdTableFormat, InTable { title: n("Tabelle bündig ausrichten") };
            TogglePreview = mdTogglePreview, Always { title: n("Vorschau ausblenden"), label: n("Vorschau"), tip: n("Vorschau ein- oder ausblenden"), key: "alt+cmd+p", symbol: "rectangle.split.2x1" };
            ToggleSidebar = mdToggleSidebar, Always { title: n("Seitenleiste einblenden"), label: n("Seitenleiste"), tip: n("Seitenleiste ein- oder ausblenden"), symbol: "sidebar.left" };
            ToggleOutline = mdToggleOutline, Always { title: n("Gliederung einblenden"), key: "ctrl+cmd+s", symbol: "sidebar.left" };
            Typography = mdTypography, Always { title: n("Typografische Satzzeichen") };
            ZoomIn = mdZoomIn, Always { title: n("Vergrößern"), key: "cmd++" };
            ZoomOut = mdZoomOut, Always { title: n("Verkleinern"), key: "cmd+-" };
            ZoomReset = mdZoomReset, Always { title: n("Originalgröße"), key: "cmd+0" };
            NewTab = mdNewTab, Always { title: n("Neuer Tab"), label: n("Neues Dokument"), tip: n("Neues Dokument in einem Tab öffnen"), symbol: "plus" };
            LoadRemoteImages = mdLoadRemoteImages, Always { title: n("Bilder laden") };
            ExportHtml = mdExportHTML, Always { title: n("Als HTML exportieren …") };
            ExportWord = mdExportWord, Always { title: n("Als Word-Dokument exportieren …") };
            PasteMarkdown = mdPasteMarkdown, InText { title: n("Als Markdown einsetzen"), tip: n("Formatierten Text als Markdown einsetzen"), key: "ctrl+cmd+v", symbol: "doc.on.clipboard" };
            EditMetadata = mdEditMetadata, Always { title: n("Metadaten bearbeiten …"), label: n("Metadaten"), tip: n("Metadaten des Dokuments in einem Formular bearbeiten"), symbol: "tag" };
            ExportOptions = mdExportOptions, Always { title: n("Exportoptionen …") };
            ToggleFiles = mdToggleFiles, Always { title: n("Dateien einblenden"), key: "ctrl+cmd+e", symbol: "folder" };
            SearchFolder = mdSearchFolder, Always { title: n("In allen Notizen suchen …"), key: "shift+cmd+f", symbol: "magnifyingglass" };
            ToggleWordCount = mdToggleWordCount, Always { title: n("Wortanzahl ausblenden") };
            ToggleChanges = mdToggleChanges, Always { title: n("Änderungen seit dem letzten Commit") };
            EditAtomAt = mdEditAtomAt, Always {};
            AtomSnippet = mdAtomSnippet, Always {};
            AtomMode = mdAtomMode, Always {};
            ToolbarSegment = mdToolbarSegment, Always {};
        }
    };
}
pub(super) use commands;

macro_rules! actions_of {
    ($($action:ident = $selector:ident, $applies:ident { $($field:ident: $value:expr),* };)*) => {
        /// An editor command.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Action {
            $($action,)*
        }

        impl Action {
            /// Every command, in the order of the table.
            pub const ALL: &[Action] = &[$(Action::$action,)*];

            /// What the menus, the toolbar and validation know of the command.
            pub fn spec(self) -> Spec {
                match self {
                    $(Action::$action => Spec {
                        $($field: Some($value),)*
                        ..Spec::blank(sel!($selector:), Applies::$applies)
                    },)*
                }
            }
        }
    };
}

commands!(actions_of);

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

/// What the menus, the toolbar and validation know of a command; what it lacks is `None`.
pub struct Spec {
    pub selector: Sel,
    /// Menu title, untranslated; the targets of controls have none, as no menu shows them.
    pub title: Option<&'static str>,
    /// Shorter toolbar label, untranslated, where the title is too long.
    pub label: Option<&'static str>,
    /// Tooltip, untranslated: it starts with a verb and has at most 75 characters.
    pub tip: Option<&'static str>,
    /// Key equivalent, as `crate::util::item` reads it.
    pub key: Option<&'static str>,
    /// SF Symbol for the toolbar and for menu items that show one.
    pub symbol: Option<&'static str>,
    pub applies: Applies,
}

impl Spec {
    fn blank(selector: Sel, applies: Applies) -> Spec {
        Spec {
            selector,
            title: None,
            label: None,
            tip: None,
            key: None,
            symbol: None,
            applies,
        }
    }
}

/// The starter diagrams of the menus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Diagram {
    Flowchart,
    Sequence,
    Gantt,
}

/// Which variant of a command an item of its submenu stands for. The item's tag is the variant's place in
/// `Action::choices`, which builds the items and reads the tag back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    /// The command has no variants.
    Plain,
    /// Body text for 0, else a heading of this level.
    Heading(u8),
    /// The info string of a code block, empty for none.
    CodeLanguage(&'static str),
    /// The kind of a new alert box.
    Alert(AlertKind),
    /// The starter of a new diagram.
    Diagram(Diagram),
    /// The alignment of a table column.
    Align(Align),
}

impl Action {
    /// The command AppKit sends `selector` for.
    pub fn from_selector(selector: Sel) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.spec().selector == selector)
    }

    /// The variants of the command in the order of their tags; none for a command without variants.
    pub fn choices(self) -> Vec<Choice> {
        match self {
            Action::Heading => (0..=6).map(Choice::Heading).collect(),
            Action::CodeBlock => std::iter::once("")
                .chain(code::MENU.iter().map(|(lang, _)| *lang))
                .map(Choice::CodeLanguage)
                .collect(),
            Action::Alert => AlertKind::ALL.map(Choice::Alert).to_vec(),
            Action::Diagram => [Diagram::Flowchart, Diagram::Sequence, Diagram::Gantt]
                .map(Choice::Diagram)
                .to_vec(),
            Action::TableAlign => [Align::None, Align::Left, Align::Center, Align::Right]
                .map(Choice::Align)
                .to_vec(),
            _ => Vec::new(),
        }
    }

    /// The variant an item's `tag` stands for; a tag no item has stands for the first.
    pub fn choice(self, tag: isize) -> Choice {
        let choices = self.choices();
        usize::try_from(tag)
            .ok()
            .and_then(|at| choices.get(at))
            .or(choices.first())
            .copied()
            .unwrap_or(Choice::Plain)
    }

    /// The tag of the item for `choice`.
    fn tag_of(self, choice: Choice) -> isize {
        let at = self.choices().iter().position(|c| *c == choice).unwrap_or(0);
        crate::util::to_isize(at)
    }

    /// Whether the menu item for this command and `choice` shows a checkmark at the formatting in `active`; `None`
    /// for commands without one.
    pub fn checked(self, active: &Active, choice: Choice) -> Option<bool> {
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
            Heading => choice == Choice::Heading(active.heading),
            BulletList => active.list == Some(ListStyle::Bullet),
            NumberedList => active.list == Some(ListStyle::Numbered),
            TaskList => active.list == Some(ListStyle::Task),
            Quote => active.quote,
            CodeBlock => matches!(choice, Choice::CodeLanguage(lang) if active.code_lang.as_deref() == Some(lang)),
            Alert => matches!(choice, Choice::Alert(kind) if active.alert == Some(kind)),
            _ => return None,
        })
    }
}

/// The menu item of a command, with its shortcut.
pub fn item(mtm: MainThreadMarker, action: Action) -> Retained<NSMenuItem> {
    let spec = action.spec();
    crate::util::item(
        mtm,
        spec.title.map_or("", t),
        Some(spec.selector),
        spec.key.unwrap_or_default(),
    )
}

/// The menu item of a command with its symbol in front.
pub fn item_with_symbol(mtm: MainThreadMarker, action: Action) -> Retained<NSMenuItem> {
    with_symbol(item(mtm, action), action.spec().symbol)
}

/// The menu item of a command for a context menu, without shortcut.
pub fn context_item(mtm: MainThreadMarker, action: Action) -> Retained<NSMenuItem> {
    let spec = action.spec();
    crate::util::item(mtm, spec.title.map_or("", t), Some(spec.selector), "")
}

/// A menu item running a variant of a command.
fn choice_item(mtm: MainThreadMarker, action: Action, title: &str, key: &str, choice: Choice) -> Retained<NSMenuItem> {
    crate::util::tagged(mtm, title, action.spec().selector, key, action.tag_of(choice))
}

/// A menu item running a command on what starts at `at` in the source.
pub fn item_at(mtm: MainThreadMarker, action: Action, title: &str, at: Byte) -> Retained<NSMenuItem> {
    let item = crate::util::item(mtm, title, Some(action.spec().selector), "");
    let at = NSNumber::new_usize(at.get());
    // SAFETY: Any object can be represented; the item retains it.
    unsafe { item.setRepresentedObject(Some(&at)) };
    item
}

/// Where in the source the menu item of `item_at` that sent a command points.
pub fn at_of(sender: Option<&AnyObject>) -> Option<Byte> {
    let item = sender?.downcast_ref::<NSMenuItem>()?;
    let at = item.representedObject()?.downcast::<NSNumber>().ok()?;
    Some(Byte::new(at.as_usize()))
}

/// `item` with `symbol` in front, if there is one and the system knows it.
pub fn with_symbol(item: Retained<NSMenuItem>, symbol: Option<&str>) -> Retained<NSMenuItem> {
    if let Some(image) =
        symbol.and_then(|symbol| NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), None))
    {
        item.setImage(Some(&image));
    }
    item
}

/// Body text and the six heading levels.
pub fn heading_items(mtm: MainThreadMarker, shortcuts: bool) -> Vec<Retained<NSMenuItem>> {
    let key = |level: u8| {
        if shortcuts {
            format!("alt+cmd+{level}")
        } else {
            String::new()
        }
    };
    let mut items = vec![choice_item(
        mtm,
        Action::Heading,
        t("Fließtext"),
        &key(0),
        Choice::Heading(0),
    )];
    for level in 1..=6 {
        let title = tf("Überschrift {}", &[&level.to_string()]);
        items.push(choice_item(
            mtm,
            Action::Heading,
            &title,
            &key(level),
            Choice::Heading(level),
        ));
    }
    items
}

/// Code block entries, the first for no language.
pub fn code_block_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    let mut items = vec![
        choice_item(mtm, Action::CodeBlock, t("Ohne Sprache"), "", Choice::CodeLanguage("")),
        NSMenuItem::separatorItem(mtm),
    ];
    for (lang, title) in code::MENU {
        items.push(choice_item(
            mtm,
            Action::CodeBlock,
            title,
            "",
            Choice::CodeLanguage(lang),
        ));
    }
    items
}

/// The kinds of alert box.
pub fn alert_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    AlertKind::ALL
        .iter()
        .map(|kind| choice_item(mtm, Action::Alert, t(kind.label()), "", Choice::Alert(*kind)))
        .collect()
}

/// The starter diagrams.
pub fn diagram_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    [
        (n("Flussdiagramm"), Diagram::Flowchart),
        (n("Sequenzdiagramm"), Diagram::Sequence),
        (n("Gantt-Diagramm"), Diagram::Gantt),
    ]
    .iter()
    .map(|(title, diagram)| choice_item(mtm, Action::Diagram, t(title), "", Choice::Diagram(*diagram)))
    .collect()
}

/// Column alignments.
pub fn align_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    let align =
        |title: &str, key: &str, align: Align| choice_item(mtm, Action::TableAlign, title, key, Choice::Align(align));
    vec![
        align(t("Links"), "cmd+{", Align::Left),
        align(t("Zentriert"), "cmd+|", Align::Center),
        align(t("Rechts"), "cmd+}", Align::Right),
        align(t("Standard"), "", Align::None),
    ]
}

/// The text of a starter diagram.
pub fn diagram_template(diagram: Diagram) -> String {
    match diagram {
        Diagram::Flowchart => format!(
            "flowchart TD\n    A[{}] --> B{{{}}}\n    B -->|{}| C[{}]\n    B -->|{}| D[{}]",
            t("Start"),
            t("Entscheidung"),
            t("Ja"),
            t("Weiter"),
            t("Nein"),
            t("Ende")
        ),
        Diagram::Sequence => format!(
            "sequenceDiagram\n    Alice->>Bob: {}\n    Bob-->>Alice: {}",
            t("Anfrage"),
            t("Antwort")
        ),
        Diagram::Gantt => format!(
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

    /// The controller's methods come from the same table as the registry; this holds that AppKit finds one for every
    /// command and no command method outside it.
    #[test]
    fn every_command_has_a_method_and_every_method_a_command() {
        let class = super::super::EditorController::class();
        for &action in Action::ALL {
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
            code_lang: Some(code::MENU[0].0.to_string()),
            alert: Some(AlertKind::ALL[1]),
            ..Active::default()
        };
        let plain = Choice::Plain;
        assert_eq!(Action::Bold.checked(&active, plain), Some(true));
        assert_eq!(Action::Italic.checked(&active, plain), Some(false));
        assert_eq!(Action::Heading.checked(&active, Action::Heading.choice(2)), Some(true));
        assert_eq!(Action::Heading.checked(&active, Action::Heading.choice(1)), Some(false));
        assert_eq!(Action::TaskList.checked(&active, plain), Some(true));
        assert_eq!(Action::BulletList.checked(&active, plain), Some(false));
        assert_eq!(
            Action::CodeBlock.checked(&active, Action::CodeBlock.choice(1)),
            Some(true)
        );
        assert_eq!(Action::Alert.checked(&active, Action::Alert.choice(1)), Some(true));
        assert_eq!(Action::Alert.checked(&active, Action::Alert.choice(0)), Some(false));
        assert_eq!(Action::Rule.checked(&active, plain), None);
    }

    #[test]
    fn a_tag_reads_back_as_the_variant_its_item_was_made_for() {
        for &action in Action::ALL {
            for choice in action.choices() {
                assert_eq!(action.choice(action.tag_of(choice)), choice, "{action:?}");
            }
        }
        assert_eq!(Action::Heading.choice(4), Choice::Heading(4));
        assert_eq!(Action::CodeBlock.choice(0), Choice::CodeLanguage(""));
        assert_eq!(Action::CodeBlock.choice(1), Choice::CodeLanguage(code::MENU[0].0));
        assert_eq!(Action::TableAlign.choice(2), Choice::Align(Align::Center));
        assert_eq!(
            Action::Diagram.choice(9),
            Choice::Diagram(Diagram::Flowchart),
            "a tag no item has"
        );
        assert_eq!(Action::Alert.choice(-1), Choice::Alert(AlertKind::ALL[0]));
        assert_eq!(Action::Bold.choice(3), Choice::Plain);
    }

    #[test]
    fn diagram_templates_are_the_three_mermaid_starters() {
        for (diagram, kind) in [
            (Diagram::Flowchart, "flowchart"),
            (Diagram::Sequence, "sequenceDiagram"),
            (Diagram::Gantt, "gantt"),
        ] {
            assert!(diagram_template(diagram).starts_with(kind), "{diagram:?}");
        }
    }

    #[test]
    fn every_shortcut_names_a_key_and_nothing_given_is_empty() {
        for &action in Action::ALL {
            let spec = action.spec();
            if let Some(key) = spec.key {
                assert!(!crate::util::split_shortcut(key).1.is_empty(), "{action:?}");
            }
            for given in [spec.title, spec.label, spec.tip, spec.key, spec.symbol]
                .into_iter()
                .flatten()
            {
                assert!(!given.is_empty(), "{action:?} gives an empty text instead of none");
            }
        }
    }

    #[test]
    fn menu_commands_have_titles() {
        for &action in Action::ALL {
            let spec = action.spec();
            let internal = matches!(
                action,
                Action::EditAtomAt | Action::AtomSnippet | Action::AtomMode | Action::ToolbarSegment
            );
            assert_eq!(spec.title.is_none(), internal, "{action:?}");
        }
    }
}
