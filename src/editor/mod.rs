//! The editor window: Markdown source and preview side by side. `controller` holds the one object AppKit knows, and the
//! components below do the work, each with the state it owns; README.md describes the rules that keep the delegate
//! callbacks from stepping on each other.

pub mod actions;
mod atom;
mod attributes;
mod controller;
pub mod decoder;
mod dialogs;
mod editing;
mod env;
mod export;
mod images;
mod js;
mod links;
mod mathfield;
mod model;
mod navigation;
mod outline;
mod panes;
mod picture;
mod progressive;
mod remote;
mod render;
mod render_queue;
mod safe_image;
mod scroll;
mod selection;
#[cfg(feature = "selftest")]
pub mod selftest;
mod shadow;
mod state;
mod style;
mod sync;
mod toolbar;
mod update;
mod views;
mod webkit;

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSMenuItem, NSToolbarItem};
use objc2_foundation::NSRange;

use crate::util::ns;
use actions::Action;
use controller::{layers, window};

pub use crate::util::panic_is_contained;
pub use controller::EditorController;
pub use images::{look_at_images, watch_memory};
pub use shadow::{discard_copies, discard_share_files, keep_copies};

use state::{Invalidate, State};

thread_local! {
    static OPEN_EDITORS: RefCell<Vec<Weak<EditorController>>> = const { RefCell::new(Vec::new()) };
}

/// Lays out every open preview again, after a setting they all follow changed.
fn relayout_all() {
    let editors: Vec<Retained<EditorController>> =
        OPEN_EDITORS.with(|e| e.borrow().iter().filter_map(Weak::load).collect());
    for editor in editors {
        editor.invalidate(Invalidate::Layout);
    }
}

/// Where `emergency_save` leaves the texts, for `offer_recovered` to point to after the next start.
pub(crate) fn recovery_dir() -> Option<PathBuf> {
    // Self-test builds keep their copies with their other output, away from the user's.
    #[cfg(feature = "selftest")]
    {
        let out = std::env::var_os("SPLAT_SELFTEST_OUT").map_or_else(std::env::temp_dir, PathBuf::from);
        Some(out.join("Recovery"))
    }
    #[cfg(not(feature = "selftest"))]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/Splat/Recovery"))
    }
}

/// Writes the text of every open document to a recovery folder; runs from the panic hook right before the app
/// ends, so it reads only the text views and no state that might be mid-change. Returns the written files.
pub fn emergency_save() -> Vec<PathBuf> {
    if MainThreadMarker::new().is_none() {
        return vec![];
    }
    let Some(dir) = recovery_dir() else {
        return vec![];
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return vec![];
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // The panic this runs from may have struck while the list was borrowed.
    let editors: Vec<Retained<EditorController>> = OPEN_EDITORS.with(|e| {
        e.try_borrow()
            .map(|e| e.iter().filter_map(Weak::load).collect())
            .unwrap_or_default()
    });
    let mut written = Vec::new();
    let mut failed = false;
    for (n, editor) in editors.iter().enumerate() {
        let Some(text) = editor.text_of_built_view() else {
            continue;
        };
        let document = editor.document();
        // What is saved anyway need not lie around a second time.
        if document.as_ref().is_some_and(|d| !d.isDocumentEdited()) {
            continue;
        }
        let name = crate::util::file_name_of(
            &document.map_or_else(|| "Dokument".to_string(), |d| d.displayName().to_string()),
        );
        let path = dir.join(format!("{name}-{stamp}-{n}.md"));
        if std::fs::write(&path, text).is_ok() {
            written.push(path);
        } else {
            failed = true;
        }
    }
    // These are newer than the copies, and the next start would offer both; if one is missing, the copies are all that
    // is left of its text.
    if !failed {
        discard_copies();
    }
    written
}

/// After a crash, says once where `emergency_save` put the texts; nothing else would show the folder.
pub fn offer_recovered(mtm: MainThreadMarker) {
    const SHOWN_UNTIL: &str = "RecoveredTextsShownUntil";
    let Some(dir) = recovery_dir() else { return };
    let newest = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .filter_map(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs_f64())
        .fold(0.0, f64::max);
    let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
    if newest <= defaults.doubleForKey(&ns(SHOWN_UNTIL)) {
        return;
    }
    defaults.setDouble_forKey(newest, &ns(SHOWN_UNTIL));
    let alert = objc2_app_kit::NSAlert::new(mtm);
    alert.setMessageText(&ns(crate::i18n::t("Splat wurde unerwartet beendet")));
    alert.setInformativeText(&ns(crate::i18n::t(
        "Der Text der geöffneten Dokumente wurde in einem eigenen Ordner gesichert.",
    )));
    alert.addButtonWithTitle(&ns(crate::i18n::t("Im Finder zeigen")));
    alert.addButtonWithTitle(&ns(crate::i18n::t("OK")));
    if alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn
        && let Some(path) = dir.to_str()
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&objc2_foundation::NSURL::fileURLWithPath(&ns(path)));
    }
}

pub fn warm_renderer(mtm: MainThreadMarker) {
    js::renderer(mtm);
}

fn tag_of(sender: Option<&AnyObject>) -> isize {
    let Some(s) = sender else { return 0 };
    if let Some(item) = s.downcast_ref::<NSMenuItem>() {
        item.tag()
    } else if let Some(control) = s.downcast_ref::<objc2_app_kit::NSControl>() {
        control.tag()
    } else if let Some(item) = s.downcast_ref::<NSToolbarItem>() {
        item.tag()
    } else {
        0
    }
}

/// Converting through UTF-16 takes a third of the time `to_string` needs, which goes through `UTF8String`.
fn string_of(s: &objc2_foundation::NSString) -> String {
    let len = s.length();
    let mut units = vec![0u16; len];
    let buffer = std::ptr::NonNull::new(units.as_mut_ptr()).expect("vector buffer");
    // SAFETY: The buffer holds exactly `len` units, and the range covers the whole string.
    unsafe { s.getCharacters_range(buffer, NSRange::new(0, len)) };
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// The most methods the class may have outside `define_class!`; what does more belongs to a component.
    const MOST_METHODS: usize = 45;

    fn sources(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(sources(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
        found
    }

    /// Items at the top level of a file: inside `define_class!` the selector methods stand deeper.
    fn methods_on_the_class(text: &str) -> usize {
        let mut inside = false;
        let mut methods = 0;
        for line in text.lines() {
            if inside {
                inside = line != "}";
                let item = line.strip_prefix("    ").filter(|l| !l.starts_with([' ', '/', '#']));
                if item.is_some_and(|l| l.starts_with("fn ") || l.contains(" fn ")) {
                    methods += 1;
                }
            } else {
                inside = line.starts_with("impl EditorController");
            }
        }
        methods
    }

    #[test]
    fn counts_the_methods_of_top_level_blocks_only() {
        let text = r#"
impl EditorController {
    /// fn in a comment
    fn a(&self) {
        fn inner() {}
    }

    pub(super) fn b(
        &self,
    ) {}
}

impl Other {
    fn c() {}
}

define_class!(
    impl EditorController {
        fn d(&self) {}
    }
);
"#;
        assert_eq!(methods_on_the_class(text), 2);
    }

    #[test]
    fn the_controller_only_forwards() {
        let editor = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/editor");
        let methods: usize = sources(&editor)
            .iter()
            .filter(|f| !f.to_string_lossy().contains("selftest"))
            .map(|f| {
                let text = std::fs::read_to_string(f).unwrap();
                methods_on_the_class(text.split("#[cfg(test)]").next().unwrap_or(""))
            })
            .sum();
        assert!(methods > 10, "the count looks at the wrong place: {methods}");
        assert!(
            methods <= MOST_METHODS,
            "{methods} methods on EditorController, at most {MOST_METHODS}"
        );
    }
}
