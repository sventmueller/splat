//! A document as the render path knows it: what it asks for goes by it, and what arrives later reaches it through it.

use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use dispatch2::MainThreadBound;
use objc2::MainThreadMarker;
use splat_file_state::Stamp;

use crate::lanes::main_soon;
use crate::picture::ImageFile;

/// What a document hears from the render path, on the main thread. The render path holds it weakly, so a document that
/// is gone hears nothing.
pub trait Viewer {
    /// Formulas and diagrams it asked for arrived; it hears this once the renderer has nothing left to do, not after each
    /// one, possibly while it is itself rendering.
    fn renders_arrived(&self);
    /// The picture from the web at `url` arrived, or failed to.
    fn image_arrived(&self, url: String);
    /// The image file at `path`, which `images::ImageStore::file` began to read, was read.
    fn image_loaded(&self, path: PathBuf, file: ImageFile);
    /// Image files it shows changed on disk; each with how it looks now.
    fn images_changed(&self, changed: Vec<(PathBuf, Stamp)>);
    /// The addresses of the pictures from the web it shows, wherever they stand.
    fn web_images_shown(&self) -> Vec<String>;
    /// Its window closed: nobody sees what it asks for from now on.
    fn has_closed(&self) -> bool;
}

/// A viewer as the render path keeps it.
pub(crate) struct Held(Weak<dyn Viewer>);

impl Held {
    pub(crate) fn of(viewer: &Rc<dyn Viewer>) -> Held {
        Held(Rc::downgrade(viewer))
    }

    pub(crate) fn load(&self) -> Option<Rc<dyn Viewer>> {
        self.0.upgrade()
    }

    /// Whether it holds `viewer`; one that is gone is nobody, so another that comes to its address is not taken for it.
    pub(crate) fn is(&self, viewer: &Rc<dyn Viewer>) -> bool {
        self.load()
            .is_some_and(|held| std::ptr::addr_eq(Rc::as_ptr(&held), Rc::as_ptr(viewer)))
    }
}

/// A number for `viewer` that no other open one has.
pub(crate) fn number_of(viewer: &Rc<dyn Viewer>) -> usize {
    Rc::as_ptr(viewer).cast::<()>().addr()
}

/// A viewer as other threads reach it: what they send arrives on the main thread, and only while its window is open.
#[derive(Clone)]
pub(crate) struct Messenger(Option<Arc<MainThreadBound<Weak<dyn Viewer>>>>);

impl Drop for Messenger {
    fn drop(&mut self) {
        // `MainThreadBound` drops its value elsewhere by waiting for the main thread, which may be waiting for this one.
        if let Some(to) = self.0.take()
            && MainThreadMarker::new().is_none()
        {
            main_soon(move || drop(to));
        }
    }
}

impl Messenger {
    /// The messenger of `viewer`, which lives on the main thread like every viewer; elsewhere one that reaches nobody.
    pub(crate) fn of(viewer: &Rc<dyn Viewer>) -> Messenger {
        Messenger(MainThreadMarker::new().map(|mtm| Arc::new(MainThreadBound::new(Rc::downgrade(viewer), mtm))))
    }

    /// Hands `message` to the viewer on the main thread when its turn comes, if its window is still open then.
    pub(crate) fn send_soon(&self, message: impl FnOnce(&dyn Viewer) + Send + 'static) {
        let to = self.clone();
        main_soon(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let viewer = to.0.as_ref().and_then(|to| to.get(mtm).upgrade());
            if let Some(viewer) = viewer.filter(|v| !v.has_closed()) {
                message(&*viewer);
            }
        });
    }
}
