mod app;
mod diag;
mod document;
mod editor;
mod large_alloc;
mod menu;
mod updates;
mod util;

use objc2::runtime::ProtocolObject;
use objc2::{ClassType, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use splat_i18n as i18n;
use splat_markdown as markdown;
use splat_preview as preview;

#[global_allocator]
static ALLOC: large_alloc::LargeAlloc = large_alloc::LargeAlloc;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some(editor::decoder::ARG) => editor::decoder::serve(),
        Some(editor::decoder::NATIVE_ARG) => editor::decoder::serve_native(),
        _ => {}
    }
    i18n::install(&app::localized_strings());
    let mtm = MainThreadMarker::new().expect("Splat must start on the main thread");
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Only the place: the message of a failed slice quotes the text around it.
        diag::report("panic", info.location().map_or_else(String::new, ToString::to_string));
        if !editor::panic_is_contained() {
            for path in editor::emergency_save() {
                diag::report("text saved after a panic", path.display());
            }
        }
        report(info);
    }));
    // NSDocumentController looks the class up by its Info.plist name, which only works once it is registered.
    document::MdDocument::class();
    let _documents = document::DocumentController::install(mtm);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = app::AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    updates::start(mtm);
    menu::install(&app, mtm);
    app.run();
}
