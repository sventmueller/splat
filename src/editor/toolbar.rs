use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSApplication, NSBeep};
use objc2_app_kit::{
    NSFontWeightRegular, NSImage, NSImageSymbolConfiguration, NSMenu, NSMenuItem, NSMenuToolbarItem,
    NSSegmentSwitchTracking, NSSegmentedControl, NSSharingServicePickerToolbarItem,
    NSSharingServicePickerToolbarItemDelegate, NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem,
    NSToolbarItemIdentifier, NSToolbarItemVisibilityPriorityLow, NSToolbarSpaceItemIdentifier,
    NSUserInterfaceItemIdentification,
};
use objc2_foundation::{NSArray, NSCopying, NSPoint, NSString};

use super::EditorController;
use super::actions::{self, Action};
use crate::i18n::{n, t};
use crate::util::constant;
use crate::util::ns;

const CHARACTER: &[&str] = &["bold", "italic", "underline", "strike", "highlight", "code", "script"];
const PARAGRAPH: &[&str] = &["heading", "list", "quote", "codeblock", "alert"];
const INSERT: &[&str] = &["link", "image", "table", "formula", "diagram", "more"];
const TRAILING: &[&str] = &["preview", "share", "new"];
/// Each group is one segmented control: macOS 26 gives every menu button of its own a separate glass background,
/// which breaks a row of mixed buttons into many small islands.
const GROUPS: &[(&str, &str, &[&str])] = &[
    ("group.character", n("Zeichen"), CHARACTER),
    ("group.paragraph", n("Absatz"), PARAGRAPH),
    ("group.insert", n("Einfügen"), INSERT),
];
const EXTRA: &[&str] = &[
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

/// The toolbar item behind a segment of a group control, for its action, menu, label and image.
pub fn segment_item(
    mtm: MainThreadMarker,
    group: &str,
    segment: usize,
    share: &ProtocolObject<dyn NSSharingServicePickerToolbarItemDelegate>,
    target: &AnyObject,
) -> Option<Retained<NSToolbarItem>> {
    let (_, _, members) = GROUPS.iter().find(|g| g.0 == group)?;
    make(mtm, members.get(segment)?, share, target)
}

/// Where the menu of a segment opens: below its left edge, in the control's coordinates.
pub fn segment_menu_origin(control: &NSSegmentedControl, segment: isize) -> NSPoint {
    let x: f64 = (0..segment).map(|i| control.widthForSegment(i)).sum();
    let y = if control.isFlipped() {
        control.bounds().size.height + 4.0
    } else {
        -4.0
    };
    NSPoint::new(x, y)
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
    control.setSegmentCount(items.len() as isize);
    control.setTrackingMode(NSSegmentSwitchTracking::Momentary);
    let overflow = NSMenu::new(mtm);
    for (n, item) in items.iter().enumerate() {
        let n = n as isize;
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
    if id == GROUPS[2].0 {
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

pub fn default_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    let space = || ns(&constant!(NSToolbarSpaceItemIdentifier).to_string());
    let mut all = vec![ns(GROUPS[0].0), space(), ns(GROUPS[1].0), space(), ns(GROUPS[2].0)];
    all.push(ns(&constant!(NSToolbarFlexibleSpaceItemIdentifier).to_string()));
    all.extend(ids(TRAILING));
    NSArray::from_retained_slice(&all)
}

pub fn allowed_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    let mut all: Vec<Retained<NSString>> = GROUPS.iter().map(|g| ns(g.0)).collect();
    all.extend(ids(CHARACTER));
    all.extend(ids(PARAGRAPH));
    all.extend(ids(INSERT));
    all.extend(ids(TRAILING));
    all.extend(ids(EXTRA));
    all.push(ns(&constant!(NSToolbarSpaceItemIdentifier).to_string()));
    all.push(ns(&constant!(NSToolbarFlexibleSpaceItemIdentifier).to_string()));
    NSArray::from_retained_slice(&all)
}

fn symbol(name: &str, description: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), Some(&ns(description)))
}

/// The toolbar button of a command, with the label, symbol and tooltip of the registry.
fn action_button(mtm: MainThreadMarker, id: &str, action: Action) -> Retained<NSToolbarItem> {
    let spec = action.spec();
    let label = if spec.label.is_empty() { spec.title } else { spec.label };
    button(mtm, id, t(label), spec.symbol, spec.selector, t(spec.tip))
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

pub fn make(
    mtm: MainThreadMarker,
    id: &str,
    share: &ProtocolObject<dyn NSSharingServicePickerToolbarItemDelegate>,
    target: &AnyObject,
) -> Option<Retained<NSToolbarItem>> {
    if let Some((_, label, members)) = GROUPS.iter().find(|g| g.0 == id) {
        return Some(segmented_group(mtm, id, t(label), members, share, target));
    }
    let item = |action| actions::item(mtm, action);
    let item = match id {
        "bold" => action_button(mtm, id, Action::Bold),
        "italic" => action_button(mtm, id, Action::Italic),
        "underline" => action_button(mtm, id, Action::Underline),
        "strike" => action_button(mtm, id, Action::Strikethrough),
        "code" => action_button(mtm, id, Action::InlineCode),
        "superscript" => action_button(mtm, id, Action::Superscript),
        "subscript" => action_button(mtm, id, Action::Subscript),
        "highlight" => action_button(mtm, id, Action::Highlight),
        "quote" => action_button(mtm, id, Action::Quote),
        "toc" => action_button(mtm, id, Action::Toc),
        "link" => action_button(mtm, id, Action::Link),
        "image" => action_button(mtm, id, Action::Image),
        "table" => action_button(mtm, id, Action::Table),
        "formula" => action_button(mtm, id, Action::Formula),
        "footnote" => action_button(mtm, id, Action::Footnote),
        "rule" => action_button(mtm, id, Action::Rule),
        "clear" => action_button(mtm, id, Action::RemoveFormatting),
        "preview" => action_button(mtm, id, Action::TogglePreview),
        "new" => action_button(mtm, id, Action::NewTab),
        "heading" => {
            let spec = Action::Heading.spec();
            menu_button(
                mtm,
                id,
                t(spec.title),
                spec.symbol,
                t(spec.tip),
                menu(mtm, actions::heading_items(mtm, false)),
            )
        }
        "codeblock" => {
            let spec = Action::CodeBlock.spec();
            menu_button(
                mtm,
                id,
                t(spec.title),
                spec.symbol,
                t(spec.tip),
                menu(mtm, actions::code_block_items(mtm)),
            )
        }
        "alert" => {
            let spec = Action::Alert.spec();
            menu_button(
                mtm,
                id,
                t(spec.title),
                spec.symbol,
                t(spec.tip),
                menu(mtm, actions::alert_items(mtm)),
            )
        }
        "diagram" => {
            let spec = Action::Diagram.spec();
            menu_button(
                mtm,
                id,
                t(spec.title),
                spec.symbol,
                t(spec.tip),
                menu(mtm, actions::diagram_items(mtm)),
            )
        }
        "list" => menu_button(
            mtm,
            id,
            t("Liste"),
            "list.bullet",
            t("Absätze in eine Liste umwandeln"),
            menu(
                mtm,
                vec![
                    item(Action::BulletList),
                    item(Action::NumberedList),
                    item(Action::TaskList),
                ],
            ),
        ),
        "script" => menu_button(
            mtm,
            id,
            t("Hoch- und Tiefstellen"),
            "textformat.superscript",
            t("Text hoch- oder tiefstellen, Formatierung entfernen"),
            menu(
                mtm,
                vec![
                    item(Action::Superscript),
                    item(Action::Subscript),
                    NSMenuItem::separatorItem(mtm),
                    item(Action::RemoveFormatting),
                ],
            ),
        ),
        "more" => menu_button(
            mtm,
            id,
            t("Weitere Elemente"),
            "ellipsis.circle",
            t("Inhaltsverzeichnis, Fußnote oder Trennlinie einfügen"),
            menu(mtm, vec![item(Action::Toc), item(Action::Footnote), item(Action::Rule)]),
        ),
        "print" => button(
            mtm,
            id,
            t("Drucken"),
            "printer",
            sel!(printDocument:),
            t("Dokument drucken"),
        ),
        "export" => button(
            mtm,
            id,
            t("PDF exportieren"),
            "arrow.down.document",
            sel!(saveDocumentToPDF:),
            t("Dokument als PDF exportieren"),
        ),
        "share" => {
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
        _ => return None,
    };
    if INSERT.contains(&id) {
        item.setVisibilityPriority(NSToolbarItemVisibilityPriorityLow);
    }
    Some(item)
}

impl EditorController {
    /// A segment of a toolbar group was clicked: it opens its menu or runs its command.
    pub(super) fn toolbar_segment(&self, sender: Option<&AnyObject>) {
        let Some(control) = sender.and_then(|s| s.downcast_ref::<NSSegmentedControl>()) else {
            return;
        };
        let segment = control.selectedSegment();
        let group = NSUserInterfaceItemIdentification::identifier(control);
        let Some(item) = group.filter(|_| segment >= 0).and_then(|g| {
            segment_item(
                self.mtm(),
                &g.to_string(),
                segment as usize,
                ProtocolObject::from_ref(self),
                self,
            )
        }) else {
            return;
        };
        if let Some(menu_item) = item.downcast_ref::<NSMenuToolbarItem>() {
            let at = segment_menu_origin(control, segment);
            menu_item
                .menu()
                .popUpMenuPositioningItem_atLocation_inView(None, at, Some(control));
        } else if let Some(action) = item.action() {
            // Segments are not validated like toolbar buttons, so a command that does not apply here is refused.
            if !self.validate(Some(action), 0, None) {
                NSBeep();
                return;
            }
            // SAFETY: The action comes from a toolbar item and takes the sender as its only argument.
            let _ =
                unsafe { NSApplication::sharedApplication(self.mtm()).sendAction_to_from(action, None, Some(control)) };
        }
    }
}
