//! The render path of a Markdown preview on macOS, which Splat shares with other apps: the layout of `splat-preview` as
//! native text (`render`), formulas and diagrams that WebKit draws in a hidden window (`js`, `webkit`), image files
//! (`images`) and pictures from the web (`remote`), all decoded by the sandboxed service of `splat-decoder` and held
//! within one memory budget (`picture`). A document asks as a `Viewer` and hears through it what arrives for it.
//!
//! `start` says once, before anything else runs, where the page and its libraries lie and where failures go; the page
//! is `render.html` in that folder, with the libraries in `js/` beside it.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use objc2_foundation::{NSBundle, NSURLSessionConfiguration};

pub mod appkit;
pub mod attributes;
pub mod files;
pub mod images;
pub mod js;
pub mod lanes;
pub mod picture;
pub mod remote;
pub mod render;
pub mod style;
pub mod viewer;
pub mod volumes;
pub mod webkit;

pub use viewer::Viewer;

/// What the app around the render path decides for it.
pub struct Host {
    /// The folder holding `render.html` and `js/`; the page may read nothing outside it.
    pub resources: PathBuf,
    /// Where what fails out of the user's sight goes.
    pub report: splat_decoder::Report,
    /// The wait before a picture from the web that failed to load is tried again for the first time.
    pub first_retry: Duration,
    /// Sets up the session that loads pictures from the web, as a self-test that serves them from inside the app does.
    pub web_session: fn(&NSURLSessionConfiguration),
}

impl Host {
    /// The page in the main bundle's resources, a first retry after half a minute and the web as it is.
    pub fn of_main_bundle(report: splat_decoder::Report) -> Host {
        Host {
            resources: main_bundle_resources(),
            report,
            first_retry: Duration::from_secs(30),
            web_session: |_| {},
        }
    }
}

fn main_bundle_resources() -> PathBuf {
    NSBundle::mainBundle()
        .resourcePath()
        .map_or_else(PathBuf::new, |path| PathBuf::from(path.to_string()))
}

static HOST: OnceLock<Host> = OnceLock::new();

/// Takes what `host` decides; a second call changes nothing.
pub fn start(host: Host) {
    splat_decoder::report_to(host.report);
    let _ = HOST.set(host);
}

/// What `start` was given, or the main bundle's resources for a program that did not call it.
fn host() -> &'static Host {
    HOST.get_or_init(|| Host::of_main_bundle(|what, detail| eprintln!("{what}: {detail}")))
}

/// The document's window closed: formulas and pictures only it waited for are of no use to anybody.
pub fn stops_showing(viewer: &Rc<dyn Viewer>) {
    js::forget(viewer);
    remote::stops_showing(viewer);
}
