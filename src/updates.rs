//! Updates through Sparkle, which only the distributed disk image carries. Sparkle asks on the second launch whether
//! it may check on its own; until then, and in builds without it, Splat makes no request.

use std::cell::OnceCell;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::NSMenuItem;
use objc2_foundation::NSBundle;

use crate::i18n::t;
use crate::util::{item, ns};

thread_local! {
    static CONTROLLER: OnceCell<Option<Retained<AnyObject>>> = const { OnceCell::new() };
}

/// Loads Sparkle from the app's Frameworks folder and starts the updater, which runs the checks the user allowed.
/// Without Sparkle, as in local builds, nothing happens.
pub fn start(_mtm: MainThreadMarker) {
    CONTROLLER.with(|c| {
        c.get_or_init(load);
    });
}

fn load() -> Option<Retained<AnyObject>> {
    let url = NSBundle::mainBundle()
        .privateFrameworksURL()?
        .URLByAppendingPathComponent(&ns("Sparkle.framework"))?;
    let bundle = NSBundle::bundleWithURL(&url)?;
    // SAFETY: loading runs no initializers that need the app. The hardened runtime refuses the framework unless it
    // carries the app's Team ID, which only the Developer ID signature has.
    if !unsafe { bundle.load() } {
        return None;
    }
    let class = AnyClass::get(c"SPUStandardUpdaterController")?;
    // SAFETY: Sparkle's documented initializer; both delegates are optional.
    unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![
            allocated,
            initWithStartingUpdater: true,
            updaterDelegate: None::<&AnyObject>,
            userDriverDelegate: None::<&AnyObject>
        ]
    }
}

fn controller() -> Option<Retained<AnyObject>> {
    CONTROLLER.with(|c| c.get().cloned().flatten())
}

fn updater() -> Option<Retained<AnyObject>> {
    // SAFETY: SPUStandardUpdaterController's `updater` property, never nil once initialized.
    controller().map(|c| unsafe { msg_send![&*c, updater] })
}

/// The menu items for updates, or none without Sparkle. Sparkle enables the first one while no check is running.
pub fn menu_items(mtm: MainThreadMarker) -> Vec<Retained<NSMenuItem>> {
    let Some(controller) = controller() else {
        return Vec::new();
    };
    let check = item(mtm, t("Nach Updates suchen …"), Some(sel!(checkForUpdates:)), "");
    // SAFETY: the controller stays alive in CONTROLLER for the life of the app; menu items hold targets weakly.
    unsafe { check.setTarget(Some(&controller)) };
    vec![
        check,
        item(
            mtm,
            t("Automatisch nach Updates suchen"),
            Some(sel!(mdToggleUpdateChecks:)),
            "",
        ),
        item(
            mtm,
            t("Updates automatisch laden und installieren"),
            Some(sel!(mdToggleUpdateDownloads:)),
            "",
        ),
    ]
}

pub fn checks_automatically() -> bool {
    // SAFETY: SPUUpdater's BOOL property, read on the main thread as Sparkle requires.
    updater().is_some_and(|u| unsafe { msg_send![&*u, automaticallyChecksForUpdates] })
}

/// Whether updates are downloaded and installed without asking; only possible with automatic checks.
pub fn installs_automatically() -> bool {
    // SAFETY: SPUUpdater's BOOL property, read on the main thread as Sparkle requires.
    updater().is_some_and(|u| unsafe { msg_send![&*u, automaticallyDownloadsUpdates] })
}

pub fn can_install_automatically() -> bool {
    // SAFETY: SPUUpdater's BOOL property, read on the main thread as Sparkle requires.
    updater().is_some_and(|u| unsafe { msg_send![&*u, allowsAutomaticUpdates] })
}

/// Sparkle keeps the choice in the app's user defaults.
pub fn set_checks_automatically(on: bool) {
    if let Some(u) = updater() {
        // SAFETY: SPUUpdater's BOOL property, set on the main thread as Sparkle requires.
        let _: () = unsafe { msg_send![&*u, setAutomaticallyChecksForUpdates: on] };
    }
}

pub fn set_installs_automatically(on: bool) {
    if let Some(u) = updater() {
        // SAFETY: SPUUpdater's BOOL property, set on the main thread as Sparkle requires.
        let _: () = unsafe { msg_send![&*u, setAutomaticallyDownloadsUpdates: on] };
    }
}
