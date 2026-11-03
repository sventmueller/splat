//! Loads images referenced by https URLs in the background. A document decides which URLs are requested, so only
//! public https hosts are contacted, downloads are size-limited, and the data is decoded by `safe_image`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSObjectProtocol, NSURL, NSURLRequest, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration, NSURLSessionDataDelegate, NSURLSessionDataTask, NSURLSessionDelegate,
    NSURLSessionResponseDisposition, NSURLSessionTask, NSURLSessionTaskDelegate,
};

use super::EditorController;
use super::images::{Rendered, Sizing};
use super::safe_image::{Decoded, MAX_BYTES};
use crate::util::ns;

#[derive(Default)]
struct Remote {
    images: HashMap<String, Stored>,
    loading: HashSet<String>,
    /// Waiting for a free connection: the time limit of a download runs from its start, so hundreds of images on one
    /// slow host would otherwise run out of time in the queue.
    waiting: VecDeque<String>,
    running: usize,
    listeners: Vec<Weak<EditorController>>,
}

struct Stored {
    found: Option<Rendered>,
    bytes: usize,
    /// When to try again after a failure the network may be to blame for, and the pause after that.
    retry: Option<(Instant, Duration)>,
}

const MAX_RUNNING: usize = 6;
#[cfg(not(feature = "selftest"))]
const FIRST_RETRY: Duration = Duration::from_secs(30);
#[cfg(feature = "selftest")]
const FIRST_RETRY: Duration = Duration::from_millis(500);
const LAST_RETRY: Duration = Duration::from_secs(15 * 60);

thread_local! {
    static REMOTE: RefCell<Remote> = RefCell::new(Remote::default());
    static SESSION: Retained<NSURLSession> = session();
}

/// URL and bytes received so far of each running download, keyed by task identifier.
type Downloads = HashMap<usize, (String, Vec<u8>)>;

static DOWNLOADS: Mutex<Option<Downloads>> = Mutex::new(None);

/// No download is running.
#[cfg(feature = "selftest")]
pub fn idle() -> bool {
    REMOTE.with(|r| r.borrow().loading.is_empty())
}

pub fn is_remote(url: &str) -> bool {
    url.starts_with("https://")
}

/// A host on the public internet: requests to `localhost`, `.local` names or private, loopback and link-local
/// addresses would let a document send requests to routers and local services. This reads the address only: a public name
/// that resolves to a private address passes, and then still needs a certificate for that name to get an answer.
fn public_host(url: &NSURL) -> bool {
    if url
        .scheme()
        .is_none_or(|s| !s.to_string().eq_ignore_ascii_case("https"))
    {
        return false;
    }
    let Some(host) = url.host().map(|h| h.to_string().to_ascii_lowercase()) else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.');
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || !host.contains('.') && !host.contains(':')
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.octets()[0] == 100 && ip.octets()[1] >= 64 && ip.octets()[1] < 128)
        }
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            !(ip.is_loopback()
                || ip.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || ip.to_ipv4_mapped().is_some())
        }
        // The system also reads `127.1`, `0x7f.1` or `0177.0.0.1` as addresses; a real name never ends in a number.
        Err(_) => !host
            .rsplit('.')
            .next()
            .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x")),
    }
}

/// Starts the next downloads as far as connections are free.
fn start_waiting() {
    while let Some(url) = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        if r.running >= MAX_RUNNING {
            return None;
        }
        let url = r.waiting.pop_front()?;
        r.running += 1;
        Some(url)
    }) {
        start(&url);
    }
}

/// The failures whose pause is over.
fn due_for_retry(images: &HashMap<String, Stored>, now: Instant) -> Vec<String> {
    images
        .iter()
        .filter(|(_, stored)| stored.retry.is_some_and(|(at, _)| at <= now))
        .map(|(url, _)| url.clone())
        .collect()
}

/// Failures the network may be to blame for get another try, for as long as a document shows them: a document decides
/// which hosts hear from Splat, and one whose image was deleted from its text no longer does.
pub fn try_failed_again() {
    let (due, listeners) = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        let listened = !r.listeners.is_empty();
        r.listeners.retain(|w| w.load().is_some());
        if r.listeners.is_empty() && listened {
            // The last document that showed them has closed.
            release(&mut r);
        }
        let listeners: Vec<_> = r.listeners.iter().filter_map(Weak::load).collect();
        (due_for_retry(&r.images, Instant::now()), listeners)
    });
    if due.is_empty() || listeners.is_empty() {
        return;
    }
    let shown: HashSet<String> = listeners.iter().flat_map(|l| l.updater().web_images_shown()).collect();
    let due: Vec<String> = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        due.into_iter()
            .filter(|url| shown.contains(url) && r.loading.insert(url.clone()))
            .collect()
    });
    for url in due {
        load(&url);
    }
}

/// Frees the images of the web once no open document shows them.
pub fn forget_unused() {
    REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        r.listeners.retain(|w| w.load().is_some());
        if r.listeners.is_empty() {
            release(&mut r);
        }
    });
}

fn release(r: &mut Remote) {
    for (_, stored) in r.images.drain() {
        super::images::count_decoded(-crate::util::to_isize(stored.bytes));
    }
}

/// `Some(None)` means loading failed; `None` means not loaded yet (a load is started).
pub fn get(url: &str, requester: Option<&EditorController>) -> Option<Option<Rendered>> {
    if !NSURL::URLWithString(&ns(url)).is_some_and(|u| public_host(&u)) {
        return Some(None);
    }
    let (found, start) = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        if let Some(c) = requester
            && !r
                .listeners
                .iter()
                .any(|w| w.load().is_some_and(|l| std::ptr::eq(&*l, c)))
        {
            r.listeners.retain(|w| w.load().is_some());
            r.listeners.push(Weak::from(c));
        }
        match r.images.get(url) {
            Some(stored) => (Some(stored.found.clone()), false),
            None => (None, requester.is_some() && r.loading.insert(url.to_string())),
        }
    });
    if start {
        load(url);
    }
    found
}

pub(super) fn load(url: &str) {
    REMOTE.with(|r| r.borrow_mut().waiting.push_back(url.to_string()));
    start_waiting();
}

fn start(url: &str) {
    let Some(nsurl) = NSURL::URLWithString(&ns(url)).filter(|u| public_host(u)) else {
        let url = url.to_string();
        DispatchQueue::main().exec_async(move || finish(url, None, Failure::Final));
        return;
    };
    let task = SESSION.with(|s| s.dataTaskWithURL(&nsurl));
    if let Ok(mut d) = DOWNLOADS.lock() {
        d.get_or_insert_with(HashMap::new)
            .insert(task.taskIdentifier(), (url.to_string(), Vec::new()));
    }
    task.resume();
}

fn session() -> Retained<NSURLSession> {
    let config = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    config.setTimeoutIntervalForRequest(20.0);
    config.setTimeoutIntervalForResource(60.0);
    // Self-tests must not depend on a server somewhere.
    #[cfg(feature = "selftest")]
    // SAFETY: The array holds a subclass of NSURLProtocol.
    unsafe {
        config.setProtocolClasses(Some(&super::selftest::protocol_classes()))
    };
    // SAFETY: NSObject's initializer, called once on the newly allocated object.
    let delegate: Retained<Loader> = unsafe { msg_send![super(Loader::alloc().set_ivars(())), init] };
    // The session keeps its delegate; its callbacks arrive on a background queue of its own.
    // SAFETY: The session retains its delegate; without a queue it creates a serial one of its own.
    unsafe {
        NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
            &config,
            Some(ProtocolObject::from_ref(&*delegate)),
            None,
        )
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MDImageLoader"]
    struct Loader;

    unsafe impl NSObjectProtocol for Loader {}
    unsafe impl NSURLSessionDelegate for Loader {}

    unsafe impl NSURLSessionTaskDelegate for Loader {
        #[unsafe(method(URLSession:task:willPerformHTTPRedirection:newRequest:completionHandler:))]
        fn redirect(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionTask,
            _response: &NSHTTPURLResponse,
            request: &NSURLRequest,
            completion: &block2::DynBlock<dyn Fn(*mut NSURLRequest)>,
        ) {
            let allowed = request.URL().is_some_and(|u| public_host(&u));
            let next = if allowed {
                request as *const NSURLRequest as *mut NSURLRequest
            } else {
                std::ptr::null_mut()
            };
            completion.call((next,));
        }

        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn completed(&self, _session: &NSURLSession, task: &NSURLSessionTask, error: Option<&NSError>) {
            let Some((url, bytes)) = DOWNLOADS
                .lock()
                .ok()
                .and_then(|mut d| d.as_mut()?.remove(&task.taskIdentifier()))
            else {
                return;
            };
            let status = task
                .response()
                .and_then(|r| r.downcast::<NSHTTPURLResponse>().ok())
                .map(|r| r.statusCode());
            let failure = failure_of(error.map(NSError::code), status);
            if error.is_some() || bytes.len() > MAX_BYTES || !status.is_some_and(|s| (200..300).contains(&s)) {
                DispatchQueue::main().exec_async(move || finish(url, None, failure));
                return;
            }
            // The session's queue also delivers the data of all other downloads, so decoding must not hold it up.
            super::images::load_in_background(move || {
                let decoded =
                    crate::util::contained(|| super::decoder::decode(&bytes, super::images::allowed_side())).flatten();
                DispatchQueue::main().exec_async(move || finish(url, decoded, Failure::Final));
            });
        }
    }

    unsafe impl NSURLSessionDataDelegate for Loader {
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn response(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionDataTask,
            response: &NSURLResponse,
            completion: &block2::DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
        ) {
            let too_big = usize::try_from(response.expectedContentLength()).is_ok_and(|n| n > MAX_BYTES);
            completion.call((if too_big {
                NSURLSessionResponseDisposition::Cancel
            } else {
                NSURLSessionResponseDisposition::Allow
            },));
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn data(&self, _session: &NSURLSession, task: &NSURLSessionDataTask, data: &NSData) {
            let over = DOWNLOADS.lock().ok().is_none_or(|mut d| {
                d.as_mut()
                    .and_then(|d| d.get_mut(&task.taskIdentifier()))
                    .is_none_or(|(_, bytes)| {
                        bytes.extend_from_slice(&data.to_vec());
                        bytes.len() > MAX_BYTES
                    })
            });
            if over {
                task.cancel();
            }
        }
    }
);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Failure {
    /// The image is what it is: too large, not found or no image at all.
    Final,
    /// The network or the server's state may be to blame.
    Passing,
}

/// `NSURLErrorCancelled`, which Splat itself causes for a download over the size limit.
const CANCELLED: isize = -999;

fn failure_of(error: Option<isize>, status: Option<isize>) -> Failure {
    match (error, status) {
        (Some(CANCELLED), _) => Failure::Final,
        (Some(_), _) | (None, Some(408 | 429 | 500..=599)) => Failure::Passing,
        _ => Failure::Final,
    }
}

fn finish(url: String, decoded: Option<Decoded>, failure: Failure) {
    let bytes = match &decoded {
        Some(Decoded::Rgba { pixels, .. }) => pixels.len(),
        _ => 0,
    };
    super::images::count_decoded(crate::util::to_isize(bytes));
    let rendered = decoded.and_then(super::decoder::image).map(|image| {
        let size = image.size();
        Rendered {
            image,
            sizing: Sizing::Points { size, descent: 0.0 },
        }
    });
    let listeners = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        r.loading.remove(&url);
        r.running = r.running.saturating_sub(1);
        let pause = r
            .images
            .get(&url)
            .and_then(|old| old.retry)
            .map_or(FIRST_RETRY, |(_, pause)| (pause * 2).min(LAST_RETRY));
        let retry = (rendered.is_none() && failure == Failure::Passing).then(|| (Instant::now() + pause, pause));
        let stored = Stored {
            found: rendered,
            bytes,
            retry,
        };
        if let Some(old) = r.images.insert(url.clone(), stored) {
            super::images::count_decoded(-crate::util::to_isize(old.bytes));
        }
        r.listeners.iter().filter_map(Weak::load).collect::<Vec<_>>()
    });
    start_waiting();
    for l in listeners {
        l.updater().image_arrived(url.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_is_due_once_its_pause_is_over() {
        let now = Instant::now();
        let stored = |retry| Stored {
            found: None,
            bytes: 0,
            retry,
        };
        let pause = Duration::from_secs(30);
        let images = HashMap::from([
            ("over".to_string(), stored(Some((now - pause, pause)))),
            ("now".to_string(), stored(Some((now, pause)))),
            ("later".to_string(), stored(Some((now + pause, pause)))),
            ("final".to_string(), stored(None)),
        ]);
        let mut due = due_for_retry(&images, now);
        due.sort();
        assert_eq!(due, ["now", "over"]);
    }

    #[test]
    fn only_passing_failures_are_tried_again() {
        assert_eq!(failure_of(Some(-1009), None), Failure::Passing, "offline");
        assert_eq!(failure_of(Some(-1001), None), Failure::Passing, "time limit");
        assert_eq!(failure_of(None, Some(503)), Failure::Passing);
        assert_eq!(failure_of(None, Some(429)), Failure::Passing);
        assert_eq!(failure_of(None, Some(404)), Failure::Final);
        assert_eq!(failure_of(None, Some(200)), Failure::Final);
        assert_eq!(failure_of(Some(CANCELLED), None), Failure::Final, "over the size limit");
    }

    #[test]
    fn only_public_https_hosts_are_contacted() {
        let ok = |s: &str| public_host(&NSURL::URLWithString(&ns(s)).unwrap());
        assert!(ok("https://img.shields.io/badge/a-b-blue.svg"));
        assert!(ok("https://93.184.216.34/x.png"));
        assert!(ok("https://172.15.0.1/x"), "just below the private range");
        assert!(ok("https://172.32.0.1/x"), "just above the private range");
        for blocked in [
            "https://172.16.0.1/x",
            "https://172.31.255.255/x",
            "https://[fe80::1]/x",
            "http://example.com/x.png",
            "https://localhost/x.png",
            "https://router.local/x.png",
            "https://intranet/x.png",
            "https://192.168.0.1/reboot",
            "https://10.0.0.1/x",
            "https://127.0.0.1/x",
            "https://169.254.1.1/x",
            "https://100.64.0.1/x",
            "https://[::1]/x",
            "https://[fd00::1]/x",
            "https://[::ffff:192.168.0.1]/x",
            "https://127.1/x",
            "https://0x7f.1/x",
            "https://0177.0.0.1/x",
            "file:///etc/passwd",
        ] {
            assert!(!ok(blocked), "{blocked}");
        }
    }
}
