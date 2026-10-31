mod app;
mod document;
mod editor;
mod large_alloc;
mod menu;
mod updates;
mod util;

use objc2::runtime::ProtocolObject;
use objc2::{ClassType, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use splat::{i18n, markdown, preview};

#[global_allocator]
static ALLOC: large_alloc::LargeAlloc = large_alloc::LargeAlloc;

fn main() {
    if std::env::args().nth(1).as_deref() == Some(editor::decoder::ARG) {
        editor::decoder::serve();
    }
    let mtm = MainThreadMarker::new().expect("Splat must start on the main thread");
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        for path in editor::emergency_save() {
            eprintln!("Splat: Text gesichert in {}", path.display());
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
