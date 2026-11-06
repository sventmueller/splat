use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSBeep};
use objc2_app_kit::{
    NSFontWeightRegular, NSImage, NSImageSymbolConfiguration, NSLayoutAttribute, NSMenu, NSMenuItem, NSMenuToolbarItem,
    NSSegmentSwitchTracking, NSSegmentedControl, NSSharingServicePickerToolbarItem,
    NSSharingServicePickerToolbarItemDelegate, NSTitlebarAccessoryViewController, NSToolbar, NSToolbarDisplayMode,
    NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem, NSToolbarItemIdentifier, NSToolbarItemVisibilityPriorityLow,
    NSToolbarSpaceItemIdentifier, NSUserInterfaceItemIdentification, NSView,
};
use objc2_foundation::{NSArray, NSCopying, NSObjectProtocol, NSPoint, NSSize, NSString};

#[cfg(feature = "selftest")]
use super::EditorController;
use super::actions::{self, Action};
use crate::i18n::{n, t};
use crate::util::constant;
use crate::util::{guarded, ns};

const CHARACTER: &[&str] = &["bold", "italic", "underline", "strike", "highlight", "script"];
const PARAGRAPH: &[&str] = &["heading", "list", "quote", "codeblock", "alert"];
const INSERT: &[&str] = &["link", "image", "table", "formula", "diagram", "more"];
const TRAILING: &[&str] = &["preview", "share", "new"];
/// Navigational items, which AppKit places before the window's title.
const LEADING: &[&str] = &["sidebar"];
/// Each group is one segmented control: macOS 26 gives every menu button of its own a separate glass background,
/// which breaks a row of mixed buttons into many small islands.
const GROUPS: &[(&str, &str, &[&str])] = &[
    ("group.character", n("Zeichen"), CHARACTER),
    ("group.paragraph", n("Absatz"), PARAGRAPH),
    ("group.insert", n("Einfügen"), INSERT),
];
const EXTRA: &[&str] = &[
    "code",
    "superscript",
    "subscript",
    "toc",
    "footnote",
    "rule",
    "clear",
    "export",
    "print",
];

const PLAIN_WIDTH: f64 = 32.0;
const MENU_WIDTH: f64 = 44.0;

/// Added to the 6 pt AppKit leaves beside a label, so that the last one stands as far from the window's edge as the
/// close button; symbols share a capsule that AppKit already sets further in.
const TEXT_MODE_MARGIN: f64 = 12.0;

fn edge_margin(mode: NSToolbarDisplayMode) -> f64 {
    if mode == NSToolbarDisplayMode::LabelOnly {
        TEXT_MODE_MARGIN
    } else {
        0.0
    }
}

define_class!(
    #[unsafe(super(NSToolbar))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDEditorToolbar"]
    #[ivars = Retained<NSView>]
    pub struct EditorToolbar;

    unsafe impl NSObjectProtocol for EditorToolbar {}

    impl EditorToolbar {
        /// AppKit comes through here for the saved mode, the toolbar's menu and the other windows of the identifier.
        #[unsafe(method(setDisplayMode:))]
        fn set_display_mode(&self, mode: NSToolbarDisplayMode) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let () = unsafe { msg_send![super(self), setDisplayMode: mode] };
            guarded((), || self.ivars().setFrameSize(NSSize::new(edge_margin(mode), 1.0)));
        }
    }
);

impl EditorToolbar {
    pub fn new(mtm: MainThreadMarker, identifier: &str) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NSView::new(mtm));
        // SAFETY: NSToolbar's designated initializer, called once on the newly allocated object.
        unsafe { msg_send![super(this), initWithIdentifier: &*ns(identifier)] }
    }

    /// A trailing accessory indents the toolbar's items by its width; its `hidden` has no effect at this position.
    pub fn margin_accessory(&self) -> Retained<NSTitlebarAccessoryViewController> {
        let accessory = NSTitlebarAccessoryViewController::new(self.mtm());
        accessory.setView(self.ivars());
        accessory.setLayoutAttribute(NSLayoutAttribute::Trailing);
        accessory
    }
}

/// The identifier of a segment of a group control.
fn member_of(group: &str, segment: usize) -> Option<&'static str> {
    GROUPS.iter().find(|g| g.0 == group)?.2.get(segment).copied()
}

/// The toolbar item behind a segment of a group control, for its action, menu, label and image.
pub fn segment_item(
    mtm: MainThreadMarker,
    group: &str,
    segment: usize,
    share: &ProtocolObject<dyn NSSharingServicePickerToolbarItemDelegate>,
    target: &AnyObject,
) -> Option<Retained<NSToolbarItem>> {
    make(mtm, member_of(group, segment)?, share, target)
}

/// Where the menu of a segment opens: below its left edge, in the control's coordinates.
pub fn segment_menu_origin(control: &NSSegmentedControl, segment: isize) -> NSPoint {
    let x: f64 = (0..segment).map(|i| control.widthForSegment(i)).sum();
    below_control(x, control.isFlipped(), control.bounds().size.height)
}

fn below_control(x: f64, flipped: bool, height: f64) -> NSPoint {
    NSPoint::new(x, if flipped { height + 4.0 } else { -4.0 })
}

fn segmented_group(
    mtm: MainThreadMarker,
    id: &str,
    label: &str,
    members: &[&str],
    share: &ProtocolObject<dyn NSSharingServicePickerToolbarItemDelegate>,
    target: &AnyObject,
) -> Retained<NSToolbarItem> {
    let items: Vec<Retained<NSToolbarItem>> = members.iter().filter_map(|m| make(mtm, m, share, target)).collect();
    let control = NSSegmentedControl::new(mtm);
    control.setSegmentCount(crate::util::to_isize(items.len()));
    control.setTrackingMode(NSSegmentSwitchTracking::Momentary);
    let overflow = NSMenu::new(mtm);
    for (n, item) in items.iter().enumerate() {
        let n = crate::util::to_isize(n);
        let menu = item.downcast_ref::<NSMenuToolbarItem>().map(|m| m.menu());
        let image = item
            .image()
            .and_then(|i| i.imageWithSymbolConfiguration(&segment_symbol_size()).or(Some(i)));
        control.setImage_forSegment(image.as_deref(), n);
        if menu.is_some() {
            control.setShowsMenuIndicator_forSegment(true, n);
        }
        control.setWidth_forSegment(if menu.is_some() { MENU_WIDTH } else { PLAIN_WIDTH }, n);
        control.setToolTip_forSegment(item.toolTip().as_deref(), n);
        let entry = crate::util::item(
            mtm,
            &item.label().to_string(),
            if menu.is_some() { None } else { item.action() },
            "",
        );
        if let Some(menu) = menu {
            entry.setSubmenu(Some(&menu.copy()));
        }
        overflow.addItem(&entry);
    }
    control.setIdentifier(Some(&ns(id)));
    // SAFETY: The target is the window's controller, which owns the toolbar; the action takes the sender.
    unsafe {
        control.setTarget(Some(target));
        control.setAction(Some(sel!(mdToolbarSegment:)));
    }
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &ns(id));
    item.setView(Some(&control));
    item.setLabel(&ns(label));
    item.setPaletteLabel(&ns(label));
    let form = crate::util::item(mtm, label, None, "");
    form.setSubmenu(Some(&overflow));
    item.setMenuFormRepresentation(Some(&form));
    if gives_way_first(id) {
        item.setVisibilityPriority(NSToolbarItemVisibilityPriorityLow);
    }
    item
}

/// Segmented controls do not size symbols like toolbar buttons do, so all segments get the same explicit size.
fn segment_symbol_size() -> Retained<NSImageSymbolConfiguration> {
    NSImageSymbolConfiguration::configurationWithPointSize_weight(15.0, constant!(NSFontWeightRegular))
}

fn ids(names: &[&str]) -> Vec<Retained<NSString>> {
    names.iter().map(|n| ns(n)).collect()
}

fn default_names<'a>(space: &'a str, flexible: &'a str) -> Vec<&'a str> {
    let mut all = LEADING.to_vec();
    all.extend([GROUPS[0].0, space, GROUPS[1].0, space, GROUPS[2].0, flexible]);
    all.extend(TRAILING);
    all
}

fn allowed_names<'a>(space: &'a str, flexible: &'a str) -> Vec<&'a str> {
    let mut all: Vec<&str> = GROUPS.iter().map(|g| g.0).collect();
    for names in [LEADING, CHARACTER, PARAGRAPH, INSERT, TRAILING, EXTRA] {
        all.extend(names);
    }
    all.extend([space, flexible]);
    all
}

pub fn default_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    let (space, flexible) = spaces();
    NSArray::from_retained_slice(&ids(&default_names(&space, &flexible)))
}

pub fn allowed_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    let (space, flexible) = spaces();
    NSArray::from_retained_slice(&ids(&allowed_names(&space, &flexible)))
}

fn spaces() -> (String, String) {
    (
        constant!(NSToolbarSpaceItemIdentifier).to_string(),
        constant!(NSToolbarFlexibleSpaceItemIdentifier).to_string(),
    )
}

fn symbol(name: &str, description: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), Some(&ns(description)))
}

/// The toolbar button of a command, with the label, symbol and tooltip of the registry.
fn action_button(mtm: MainThreadMarker, id: &str, action: Action) -> Retained<NSToolbarItem> {
    let spec = action.spec();
    let label = spec.label.or(spec.title).map_or("", t);
    button(
        mtm,
        id,
        label,
        spec.symbol.unwrap_or_default(),
        spec.selector,
        spec.tip.map_or("", t),
    )
}

fn button(
    mtm: MainThreadMarker,
    id: &str,
    label: &str,
    image: &str,
    action: Sel,
    tip: &str,
) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &ns(id));
    item.setLabel(&ns(label));
    item.setPaletteLabel(&ns(label));
    item.setToolTip(Some(&ns(tip)));
    item.setImage(symbol(image, label).as_deref());
    // SAFETY: Toolbar actions are command selectors that take the sender as their only argument.
    unsafe { item.setAction(Some(action)) };
    item.setBordered(true);
    item
}

fn menu_button(
    mtm: MainThreadMarker,
    id: &str,
    label: &str,
    image: &str,
    tip: &str,
    menu: Retained<NSMenu>,
) -> Retained<NSToolbarItem> {
    let item = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), &ns(id));
    item.setLabel(&ns(label));
    item.setPaletteLabel(&ns(label));
    item.setToolTip(Some(&ns(tip)));
    item.setImage(symbol(image, label).as_deref());
    item.setMenu(&menu);
    item.setShowsIndicator(true);
    Retained::into_super(item)
}

fn menu(mtm: MainThreadMarker, items: Vec<Retained<objc2_app_kit::NSMenuItem>>) -> Retained<NSMenu> {
    let m = NSMenu::new(mtm);
    for i in &items {
        m.addItem(i);
    }
    m
}

/// What a toolbar identifier stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Group {
        label: &'static str,
        members: &'static [&'static str],
    },
    /// A button that runs a command of the registry.
    Command(Action),
    /// A menu of the variants of a command of the registry.
    Variants(Action),
    /// A menu of commands, with a separator for each `None`.
    Commands {
        label: &'static str,
        symbol: &'static str,
        tip: &'static str,
        items: &'static [Option<Action>],
    },
    Print,
    Export,
    Share,
}

fn entry(id: &str) -> Option<Entry> {
    if let Some((_, label, members)) = GROUPS.iter().find(|g| g.0 == id) {
        return Some(Entry::Group { label, members });
    }
    Some(match id {
        "bold" => Entry::Command(Action::Bold),
        "italic" => Entry::Command(Action::Italic),
        "underline" => Entry::Command(Action::Underline),
        "strike" => Entry::Command(Action::Strikethrough),
        "code" => Entry::Command(Action::InlineCode),
        "superscript" => Entry::Command(Action::Superscript),
        "subscript" => Entry::Command(Action::Subscript),
        "highlight" => Entry::Command(Action::Highlight),
        "quote" => Entry::Command(Action::Quote),
        "toc" => Entry::Command(Action::Toc),
        "link" => Entry::Command(Action::Link),
        "image" => Entry::Command(Action::Image),
        "table" => Entry::Command(Action::Table),
        "formula" => Entry::Command(Action::Formula),
        "footnote" => Entry::Command(Action::Footnote),
        "rule" => Entry::Command(Action::Rule),
        "clear" => Entry::Command(Action::RemoveFormatting),
        "preview" => Entry::Command(Action::TogglePreview),
        "sidebar" => Entry::Command(Action::ToggleSidebar),
        "new" => Entry::Command(Action::NewTab),
        "heading" => Entry::Variants(Action::Heading),
        "codeblock" => Entry::Variants(Action::CodeBlock),
        "alert" => Entry::Variants(Action::Alert),
        "diagram" => Entry::Variants(Action::Diagram),
        "list" => Entry::Commands {
            label: n("Liste"),
            symbol: "list.bullet",
            tip: n("Absätze in eine Liste umwandeln"),
            items: &[
                Some(Action::BulletList),
                Some(Action::NumberedList),
                Some(Action::TaskList),
            ],
        },
        "script" => Entry::Commands {
            label: n("Hoch- und Tiefstellen"),
            symbol: "textformat.superscript",
            tip: n("Text hoch- oder tiefstellen, Formatierung entfernen"),
            items: &[
                Some(Action::Superscript),
                Some(Action::Subscript),
                None,
                Some(Action::RemoveFormatting),
            ],
        },
        "more" => Entry::Commands {
            label: n("Weitere Elemente"),
            symbol: "ellipsis.circle",
            tip: n("Inhaltsverzeichnis, Fußnote oder Trennlinie einfügen"),
            items: &[Some(Action::Toc), Some(Action::Footnote), Some(Action::Rule)],
        },
        "print" => Entry::Print,
        "export" => Entry::Export,
        "share" => Entry::Share,
        _ => return None,
    })
}

/// Items that leave the toolbar first when the window gets narrow.
fn gives_way_first(id: &str) -> bool {
    id == GROUPS[2].0 || INSERT.contains(&id)
}

fn variants(mtm: MainThreadMarker, action: Action) -> Vec<Retained<NSMenuItem>> {
    match action {
        Action::Heading => actions::heading_items(mtm, false),
        // Inline code has no segment of its own, so that the default toolbar fits a window of the default width.
        Action::CodeBlock => {
            let mut items = vec![actions::item(mtm, Action::InlineCode), NSMenuItem::separatorItem(mtm)];
            items.extend(actions::code_block_items(mtm));
            items
        }
        Action::Alert => actions::alert_items(mtm),
        Action::Diagram => actions::diagram_items(mtm),
        _ => Vec::new(),
    }
}

pub fn make(
    mtm: MainThreadMarker,
    id: &str,
    share: &ProtocolObject<dyn NSSharingServicePickerToolbarItemDelegate>,
    target: &AnyObject,
) -> Option<Retained<NSToolbarItem>> {
    let item = match entry(id)? {
        Entry::Group { label, members } => return Some(segmented_group(mtm, id, t(label), members, share, target)),
        Entry::Command(action) => action_button(mtm, id, action),
        Entry::Variants(action) => {
            let spec = action.spec();
            menu_button(
                mtm,
                id,
                spec.title.map_or("", t),
                spec.symbol.unwrap_or_default(),
                spec.tip.map_or("", t),
                menu(mtm, variants(mtm, action)),
            )
        }
        Entry::Commands {
            label,
            symbol,
            tip,
            items,
        } => {
            let items = items
                .iter()
                .map(|action| action.map_or_else(|| NSMenuItem::separatorItem(mtm), |a| actions::item(mtm, a)))
                .collect();
            menu_button(mtm, id, t(label), symbol, t(tip), menu(mtm, items))
        }
        Entry::Print => button(
            mtm,
            id,
            t("Drucken"),
            "printer",
            sel!(printDocument:),
            t("Dokument drucken"),
        ),
        Entry::Export => button(
            mtm,
            id,
            t("PDF exportieren"),
            "arrow.down.document",
            sel!(saveDocumentToPDF:),
            t("Dokument als PDF exportieren"),
        ),
        Entry::Share => {
            let item = NSSharingServicePickerToolbarItem::initWithItemIdentifier(
                NSSharingServicePickerToolbarItem::alloc(mtm),
                &ns(id),
            );
            item.setLabel(&ns(t("Teilen")));
            item.setPaletteLabel(&ns(t("Teilen")));
            item.setToolTip(Some(&ns(t("Markdown-Datei teilen, etwa per Mail oder Nachrichten"))));
            item.setDelegate(Some(share));
            Retained::into_super(item)
        }
    };
    if gives_way_first(id) {
        item.setVisibilityPriority(NSToolbarItemVisibilityPriorityLow);
    }
    item.setNavigational(LEADING.contains(&id));
    Some(item)
}

/// A segment of a toolbar group was clicked: it opens its menu or runs its command, unless `applies` refuses it. `share`
/// and `target` are what `make` takes.
pub(super) fn click_segment(
    mtm: MainThreadMarker,
    sender: Option<&AnyObject>,
    share: &ProtocolObject<dyn NSSharingServicePickerToolbarItemDelegate>,
    target: &AnyObject,
    applies: impl FnOnce(Sel) -> bool,
) {
    let Some(control) = sender.and_then(|s| s.downcast_ref::<NSSegmentedControl>()) else {
        return;
    };
    let segment = control.selectedSegment();
    let group = NSUserInterfaceItemIdentification::identifier(control);
    let Some(item) = group
        .filter(|_| segment >= 0)
        .and_then(|g| segment_item(mtm, &g.to_string(), crate::util::to_usize(segment), share, target))
    else {
        return;
    };
    if let Some(menu_item) = item.downcast_ref::<NSMenuToolbarItem>() {
        let at = segment_menu_origin(control, segment);
        menu_item
            .menu()
            .popUpMenuPositioningItem_atLocation_inView(None, at, Some(control));
    } else if let Some(action) = item.action() {
        // Segments are not validated like toolbar buttons, so a command that does not apply here is refused.
        if !applies(action) {
            NSBeep();
            return;
        }
        // SAFETY: The action comes from a toolbar item and takes the sender as its only argument.
        let _ = unsafe { NSApplication::sharedApplication(mtm).sendAction_to_from(action, None, Some(control)) };
    }
}

/// `click_segment` for the toolbar of `controller`, as a click on the segment `sender` runs it.
#[cfg(feature = "selftest")]
pub(super) fn segment_clicked(controller: &EditorController, sender: Option<&AnyObject>) {
    click_segment(
        controller.mtm(),
        sender,
        ProtocolObject::from_ref(controller),
        controller,
        |action| controller.command_applies(action),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_entries() -> Vec<&'static str> {
        allowed_names("space", "flexible")
            .into_iter()
            .filter(|id| !["space", "flexible"].contains(id))
            .collect()
    }

    #[test]
    fn the_default_toolbar_is_the_sidebar_button_the_three_groups_and_what_stands_behind_them() {
        assert_eq!(
            default_names("space", "flexible"),
            [
                "sidebar",
                "group.character",
                "space",
                "group.paragraph",
                "space",
                "group.insert",
                "flexible",
                "preview",
                "share",
                "new",
            ]
        );
    }

    #[test]
    fn whatever_the_toolbar_shows_or_offers_can_be_made_once() {
        let allowed = allowed_names("space", "flexible");
        let unique: std::collections::HashSet<_> = allowed.iter().collect();
        assert_eq!(unique.len(), allowed.len(), "offered twice");
        for id in default_names("space", "flexible") {
            assert!(
                allowed.contains(&id),
                "{id} stands in the toolbar but cannot be added again"
            );
        }
        for id in all_entries() {
            assert!(entry(id).is_some(), "{id} is offered but not made");
        }
        for (group, _, members) in GROUPS {
            assert!(entry(group).is_some(), "{group}");
            for member in *members {
                assert!(entry(member).is_some(), "{member} of {group}");
            }
        }
        assert_eq!(entry("nothing"), None);
    }

    #[test]
    fn every_button_runs_the_command_it_is_named_after() {
        for (id, selector) in [
            ("bold", sel!(mdBold:)),
            ("italic", sel!(mdItalic:)),
            ("underline", sel!(mdUnderline:)),
            ("strike", sel!(mdStrikethrough:)),
            ("code", sel!(mdInlineCode:)),
            ("superscript", sel!(mdSuperscript:)),
            ("subscript", sel!(mdSubscript:)),
            ("highlight", sel!(mdHighlight:)),
            ("quote", sel!(mdQuote:)),
            ("toc", sel!(mdInsertToc:)),
            ("link", sel!(mdInsertLink:)),
            ("image", sel!(mdInsertImage:)),
            ("table", sel!(mdInsertTable:)),
            ("formula", sel!(mdInsertFormula:)),
            ("footnote", sel!(mdInsertFootnote:)),
            ("rule", sel!(mdInsertRule:)),
            ("clear", sel!(mdRemoveFormatting:)),
            ("preview", sel!(mdTogglePreview:)),
            ("sidebar", sel!(mdToggleSidebar:)),
            ("new", sel!(mdNewTab:)),
        ] {
            let Some(Entry::Command(action)) = entry(id) else {
                panic!("{id} is no button");
            };
            assert_eq!(action.spec().selector, selector, "{id}");
        }
        for (id, action) in [
            ("heading", Action::Heading),
            ("codeblock", Action::CodeBlock),
            ("alert", Action::Alert),
            ("diagram", Action::Diagram),
        ] {
            assert_eq!(entry(id), Some(Entry::Variants(action)));
        }
    }

    #[test]
    fn every_command_in_the_toolbar_has_a_symbol_and_a_name_for_its_button() {
        for id in all_entries() {
            let action = match entry(id) {
                Some(Entry::Command(action) | Entry::Variants(action)) => action,
                _ => continue,
            };
            let spec = action.spec();
            assert!(spec.symbol.is_some(), "{id} would show an empty button");
            assert!(spec.title.is_some(), "{id} would have no label");
            assert!(spec.tip.is_some(), "{id} would have no tooltip");
        }
    }

    #[test]
    fn the_menus_of_their_own_hold_their_commands_in_order() {
        let commands = |id| match entry(id) {
            Some(Entry::Commands { items, .. }) => items,
            other => panic!("{id}: {other:?}"),
        };
        assert_eq!(
            commands("list"),
            [
                Some(Action::BulletList),
                Some(Action::NumberedList),
                Some(Action::TaskList)
            ]
        );
        assert_eq!(
            commands("script"),
            [
                Some(Action::Superscript),
                Some(Action::Subscript),
                None,
                Some(Action::RemoveFormatting),
            ]
        );
        assert_eq!(
            commands("more"),
            [Some(Action::Toc), Some(Action::Footnote), Some(Action::Rule)]
        );
    }

    #[test]
    fn a_segment_stands_for_the_member_at_its_place() {
        assert_eq!(member_of("group.character", 0), Some("bold"));
        assert_eq!(member_of("group.character", 5), Some("script"));
        assert_eq!(member_of("group.character", 6), None);
        assert_eq!(member_of("group.paragraph", 0), Some("heading"));
        assert_eq!(member_of("group.insert", 5), Some("more"));
        assert_eq!(member_of("bold", 0), None);
    }

    #[test]
    fn only_what_inserts_gives_way_first() {
        for id in ["group.insert", "link", "image", "table", "formula", "diagram", "more"] {
            assert!(gives_way_first(id), "{id}");
        }
        for id in [
            "group.character",
            "group.paragraph",
            "bold",
            "heading",
            "preview",
            "share",
            "new",
            "sidebar",
        ] {
            assert!(!gives_way_first(id), "{id}");
        }
    }

    #[test]
    fn only_the_text_mode_keeps_an_extra_margin() {
        assert_eq!(edge_margin(NSToolbarDisplayMode::LabelOnly), TEXT_MODE_MARGIN);
        for mode in [
            NSToolbarDisplayMode::Default,
            NSToolbarDisplayMode::IconAndLabel,
            NSToolbarDisplayMode::IconOnly,
        ] {
            assert_eq!(edge_margin(mode), 0.0, "{mode:?}");
        }
    }

    #[test]
    fn a_menu_opens_below_its_control_whichever_way_the_view_counts() {
        assert_eq!(below_control(32.0, true, 28.0), NSPoint::new(32.0, 32.0));
        assert_eq!(below_control(32.0, false, 28.0), NSPoint::new(32.0, -4.0));
    }
}
