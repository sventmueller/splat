//! The pictures of an editor: its store of image files, and the bar above the preview that offers to grant the folder
//! around the document while the App Sandbox keeps it closed.

use std::cell::{OnceCell, RefCell};
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
pub use splat_render::images::{ImageFile, ImageStore, native_only, resolve_path, watch_memory};

use super::layers::ForPictures;
use crate::i18n::t;
use crate::util::{Stamp, ns};

/// Compares the images of open previews with their files, so one that was replaced, or one that appeared only after
/// the document named it, shows without opening the document again.
pub fn look_at_images(_mtm: MainThreadMarker) {
    let editors = super::open_editors();
    splat_render::images::look_at_images(editors.iter().filter_map(|e| Some((e.viewer(), e.images_shown()?))));
}

/// Height of the bar above the preview that offers to grant the document's folder.
const BANNER_HEIGHT: f64 = 48.0;

#[derive(Default)]
pub(super) struct PictureState {
    images: RefCell<ImageStore>,
    /// The bar above the preview, which offers to grant the document's folder.
    banner: OnceCell<Retained<NSView>>,
    banner_parts: OnceCell<(Retained<objc2_app_kit::NSTextField>, Retained<objc2_app_kit::NSButton>)>,
}

/// What the bar above the preview offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Offer {
    /// The sandbox keeps the folder around the document closed, and the document names files in it.
    GrantFolder,
}

impl Offer {
    fn wording(self) -> (&'static str, &'static str, objc2::runtime::Sel) {
        match self {
            Offer::GrantFolder => (
                t(
                    "Bilder, Notizen und Git-Daten neben diesem Dokument kann Splat erst lesen, wenn du ihren Ordner freigibst.",
                ),
                t("Ordner freigeben …"),
                sel!(mdGrantFolder:),
            ),
        }
    }
}

pub(super) struct Pictures<'a> {
    host: ForPictures<'a>,
    state: &'a PictureState,
}

impl<'a> Pictures<'a> {
    pub(super) fn new(host: ForPictures<'a>, state: &'a PictureState) -> Self {
        Pictures { host, state }
    }
}

#[cfg(feature = "selftest")]
impl Pictures<'_> {
    pub(super) fn banner_shown(&self) -> Option<bool> {
        self.state.banner.get().map(|b| !b.isHidden())
    }

    pub(super) fn banner_offers(&self) -> Option<String> {
        let shown = self.state.banner.get().is_some_and(|b| !b.isHidden());
        let (_, button) = self.state.banner_parts.get()?;
        shown.then(|| button.title().to_string())
    }
}

impl<'a> Pictures<'a> {
    pub(super) fn forget_images_no_longer_shown(&self) {
        if self.state.images.borrow().is_empty() {
            return;
        }
        let doc_dir = self.host.doc_dir();
        let shown: std::collections::HashSet<PathBuf> = self.host.model().read(move |st| {
            st.layout()
                .blocks
                .iter()
                .flat_map(|block| super::state::images_of(block))
                .filter_map(|url| resolve_path(url, doc_dir.as_deref()))
                .collect()
        });
        self.state.images.borrow_mut().keep_only(&shown);
    }

    pub(super) fn store(&self) -> &'a RefCell<ImageStore> {
        &self.state.images
    }

    /// The bar is in the view hierarchy now.
    pub(super) fn install_banner(&self, banner: Retained<NSView>) {
        let _ = self.state.banner.set(banner);
    }

    pub(super) fn read_images_again(&self, changed: Vec<(PathBuf, Stamp)>) {
        let doc_dir = self.host.doc_dir();
        for (path, now) in changed {
            self.state.images.borrow_mut().note_seen(&path, now);
            splat_render::images::read_again(path, doc_dir.as_deref(), &self.host.object().viewer());
        }
    }

    pub(super) fn image_loaded(&self, path: PathBuf, file: ImageFile) {
        let key = path.to_string_lossy().into_owned();
        self.state.images.borrow_mut().finish(path, file);
        self.host.image_arrived(key);
    }

    pub(super) fn build_banner(&self, size: NSSize) -> Retained<NSView> {
        let mtm = self.host.mtm();
        let frame = NSRect::new(
            NSPoint::new(0.0, size.height - BANNER_HEIGHT),
            NSSize::new(size.width, BANNER_HEIGHT),
        );
        let banner = objc2_app_kit::NSBox::initWithFrame(objc2_app_kit::NSBox::alloc(mtm), frame);
        banner.setBoxType(objc2_app_kit::NSBoxType::Custom);
        banner.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        banner.setBorderWidth(0.0);
        banner.setContentViewMargins(NSSize::new(0.0, 0.0));
        banner.setFillColor(&objc2_app_kit::NSColor::controlBackgroundColor());
        banner.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        banner.setHidden(true);
        let content = banner.contentView().expect("box content view");
        let (text, title, action) = Offer::GrantFolder.wording();
        let button = crate::util::button(mtm, title, self.host.object().target(), action);
        button.setControlSize(objc2_app_kit::NSControlSize::Small);
        button.setFont(Some(&objc2_app_kit::NSFont::systemFontOfSize(
            objc2_app_kit::NSFont::smallSystemFontSize(),
        )));
        button.sizeToFit();
        let b = button.frame().size;
        button.setFrameOrigin(NSPoint::new(
            size.width - b.width - 12.0,
            (BANNER_HEIGHT - b.height) / 2.0,
        ));
        button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        content.addSubview(&button);
        let label = objc2_app_kit::NSTextField::wrappingLabelWithString(&ns(text), mtm);
        label.setFont(Some(&objc2_app_kit::NSFont::systemFontOfSize(
            objc2_app_kit::NSFont::smallSystemFontSize(),
        )));
        label.setTextColor(Some(&objc2_app_kit::NSColor::secondaryLabelColor()));
        label.setMaximumNumberOfLines(2);
        label.setFrame(NSRect::new(
            NSPoint::new(12.0, 4.0),
            NSSize::new((size.width - b.width - 36.0).max(40.0), BANNER_HEIGHT - 8.0),
        ));
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&label);
        let _ = self.state.banner_parts.set((label.clone(), button.clone()));
        let line = objc2_app_kit::NSBox::initWithFrame(
            objc2_app_kit::NSBox::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, 1.0)),
        );
        line.setBoxType(objc2_app_kit::NSBoxType::Separator);
        line.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&line);
        Retained::into_super(banner)
    }

    /// Shows the bar with what it offers, or hides it, and gives the preview the room that is left.
    pub(super) fn show_banner(&self, offer: Option<Offer>) {
        let Some(banner) = self.state.banner.get() else {
            return;
        };
        if let (Some(offer), Some((label, button))) = (offer, self.state.banner_parts.get()) {
            let (text, title, action) = offer.wording();
            label.setStringValue(&ns(text));
            button.setTitle(&ns(title));
            crate::util::set_action(button, self.host.object().target(), action);
            button.sizeToFit();
            let width = banner.frame().size.width;
            let b = button.frame().size;
            button.setFrameOrigin(NSPoint::new(width - b.width - 12.0, (BANNER_HEIGHT - b.height) / 2.0));
            label.setFrameSize(NSSize::new((width - b.width - 36.0).max(40.0), BANNER_HEIGHT - 8.0));
        }
        let show = offer.is_some();
        if banner.isHidden() != show {
            return;
        }
        banner.setHidden(!show);
        self.host
            .panes()
            .leave_room_above_preview(if show { BANNER_HEIGHT } else { 0.0 });
    }

    /// The images of files, read again: a folder granted just now holds those that failed before.
    pub(super) fn read_all_again(&self) {
        self.state
            .images
            .borrow_mut()
            .keep_only(&std::collections::HashSet::new());
    }
}
