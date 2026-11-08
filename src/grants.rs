//! Folders the user granted Splat. In the App Sandbox a document that was opened may be read and written, but the
//! pictures, notes and repository around it only once the user picked their folder in an open panel; the grant then holds
//! for the folder and everything below it. Grants are kept as security-scoped bookmarks and taken up again at each start.

use std::path::Path;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel, NSWindow};
use objc2_foundation::{
    NSArray, NSData, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSUserDefaults,
};

use crate::i18n::t;
use crate::util::{guarded, ns};

/// User defaults key of the granted folders, as bookmark data.
const KEY: &str = "FolderGrants";

/// Whether Splat runs in the App Sandbox, where a folder around a document needs a grant.
pub fn sandboxed() -> bool {
    std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}

fn saved() -> Vec<Retained<NSData>> {
    NSUserDefaults::standardUserDefaults()
        .arrayForKey(&ns(KEY))
        .map(|list| list.iter().filter_map(|item| item.downcast::<NSData>().ok()).collect())
        .unwrap_or_default()
}

fn save(bookmarks: &[Retained<NSData>]) {
    let list = NSArray::from_retained_slice(bookmarks);
    // SAFETY: An array of data objects is a property list value, as user defaults require.
    unsafe { NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&list), &ns(KEY)) };
}

fn bookmark(url: &NSURL) -> Option<Retained<NSData>> {
    url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
        NSURLBookmarkCreationOptions::WithSecurityScope,
        None,
        None,
    )
    .ok()
}

/// Takes up the grants of earlier runs, before a document reads around itself; one whose folder has gone drops out.
pub fn restore() {
    if !sandboxed() {
        return;
    }
    let mut kept = Vec::new();
    for data in saved() {
        let mut stale = Bool::NO;
        // SAFETY: Bookmark data this app made, and a valid place for the flag.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope,
                None,
                &raw mut stale,
            )
        };
        let Ok(url) = url else { continue };
        // SAFETY: A URL resolved from a security-scoped bookmark, whose access lasts until the app quits.
        if unsafe { url.startAccessingSecurityScopedResource() } {
            kept.push(if stale.as_bool() {
                bookmark(&url).unwrap_or(data)
            } else {
                data
            });
        }
    }
    save(&kept);
}

/// Keeps the grant of `url`, a folder or a file the user just picked in an open panel, for later runs.
pub fn remember(url: &NSURL) {
    if !sandboxed() {
        return;
    }
    let Some(data) = bookmark(url) else { return };
    // SAFETY: A folder the user picked; its access lasts until the app quits.
    unsafe { url.startAccessingSecurityScopedResource() };
    let path = url.path().map(|p| p.to_string());
    let mut bookmarks: Vec<Retained<NSData>> = saved()
        .into_iter()
        .filter(|old| path.is_none() || resolved_path(old) != path)
        .collect();
    bookmarks.push(data);
    save(&bookmarks);
}

fn resolved_path(data: &NSData) -> Option<String> {
    let mut stale = Bool::NO;
    // SAFETY: Bookmark data this app made, and a valid place for the flag; resolving without the scope starts no access.
    let url = unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            data,
            NSURLBookmarkResolutionOptions::WithoutUI,
            None,
            &raw mut stale,
        )
    };
    Some(url.ok()?.path()?.to_string())
}

/// Whether Splat may read the folder `dir`. It reads the disk, so it belongs on the thread `DISK`.
pub fn reaches(dir: &Path) -> bool {
    !crate::test_hooks::plays_closed(dir) && (!sandboxed() || std::fs::read_dir(dir).is_ok())
}

/// Asks for a folder in an open panel, on `window` or on its own, showing `suggested`; `then` hears whether one was
/// granted.
pub fn ask(mtm: MainThreadMarker, window: Option<&NSWindow>, suggested: &Path, then: impl Fn(bool) + 'static) {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(false);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(false);
    panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&ns(&suggested.to_string_lossy()))));
    panel.setMessage(Some(&ns(t(
        "Splat darf dann Bilder, Notizen und Git-Daten in diesem Ordner und allen Unterordnern lesen. Das gilt dauerhaft.",
    ))));
    panel.setPrompt(Some(&ns(t("Freigeben"))));
    let chosen = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        guarded((), || {
            let url = (response == NSModalResponseOK).then(|| chosen.URL()).flatten();
            if let Some(url) = &url {
                remember(url);
            }
            then(url.is_some());
        });
    });
    match window {
        Some(window) => panel.beginSheetModalForWindow_completionHandler(window, &handler),
        None => panel.beginWithCompletionHandler(&handler),
    }
}

/// The folders and files granted so far, by path.
#[cfg(feature = "selftest")]
pub fn granted() -> Vec<String> {
    saved().iter().filter_map(|data| resolved_path(data)).collect()
}

/// Gives up the grant of `path`, as a self-test leaves no grant of its own behind.
#[cfg(feature = "selftest")]
pub fn forget(path: &Path) {
    let path = path.to_string_lossy();
    let kept: Vec<Retained<NSData>> = saved()
        .into_iter()
        .filter(|data| resolved_path(data).as_deref() != Some(path.as_ref()))
        .collect();
    save(&kept);
}
