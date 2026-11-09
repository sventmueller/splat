//! Loads images referenced by https URLs in the background. Downloads are size-limited and keep no cookies, and the data
//! is decoded in the decoding service.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPCookieAcceptPolicy, NSHTTPURLResponse, NSObjectProtocol, NSURL, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration, NSURLSessionDataDelegate, NSURLSessionDataTask, NSURLSessionDelegate,
    NSURLSessionResponseDisposition, NSURLSessionTask, NSURLSessionTaskDelegate,
};

use super::EditorController;
use super::decoder::{Decoded, MAX_BYTES};
use super::picture::{Rendered, Sizing};
use crate::util::{guarded, main_soon, ns};

#[derive(Default)]
struct Remote {
    images: HashMap<String, Stored>,
    loading: HashSet<String>,
    /// Waiting for a free connection: the time limit of a download runs from its start, so hundreds of images on one
    /// slow host would otherwise run out of time in the queue.
    waiting: VecDeque<String>,
    running: usize,
    /// The downloads under way, by address: nobody may be left to show what they bring.
    tasks: HashMap<String, Retained<NSURLSessionDataTask>>,
    /// The documents that wait for what is loading or waiting to load.
    askers: HashMap<String, HashSet<usize>>,
    listeners: Vec<Weak<EditorController>>,
}

struct Stored {
    found: Option<Rendered>,
    bytes: usize,
    /// When to try again after a failure the network may be to blame for, and the pause after that.
    retry: Option<(Instant, Duration)>,
}

const MAX_RUNNING: usize = 6;
const FIRST_RETRY: Duration = crate::test_hooks::FIRST_RETRY;
const LAST_RETRY: Duration = Duration::from_secs(15 * 60);

thread_local! {
    static REMOTE: RefCell<Remote> = RefCell::new(Remote::default());
    static SESSION: Retained<NSURLSession> = session();
}

struct Download {
    url: String,
    bytes: Vec<u8>,
}

/// Keyed by task identifier.
type Downloads = HashMap<usize, Download>;

static DOWNLOADS: Mutex<Option<Downloads>> = Mutex::new(None);

/// No download is running.
#[cfg(feature = "selftest")]
pub fn idle() -> bool {
    REMOTE.with(|r| r.borrow().loading.is_empty())
}

pub fn is_remote(url: &str) -> bool {
    crate::markdown::url::is_https(url)
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
    let shown: Vec<(usize, Vec<String>)> = listeners.iter().map(|l| (asker(l), l.web_images_shown())).collect();
    let due: Vec<String> = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        let due: Vec<String> = due
            .into_iter()
            .filter(|url| shown.iter().any(|(_, urls)| urls.contains(url)) && r.loading.insert(url.clone()))
            .collect();
        for url in &due {
            let owners = shown
                .iter()
                .filter(|(_, urls)| urls.contains(url))
                .map(|(owner, _)| *owner);
            r.askers.entry(url.clone()).or_default().extend(owners);
        }
        due
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

fn asker(controller: &EditorController) -> usize {
    std::ptr::from_ref(controller) as usize
}

/// A document has closed, and a document decides which hosts hear from Splat.
pub(super) fn stops_showing(controller: &EditorController) {
    REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        r.listeners
            .retain(|w| w.load().is_some_and(|l| !std::ptr::eq(&*l, controller)));
        forget_asker(&mut r, asker(controller));
        if r.listeners.is_empty() {
            release(&mut r);
        }
    });
}

/// What only `gone` waited for stops waiting for its turn or loading.
fn forget_asker(r: &mut Remote, gone: usize) {
    let mut nobody = HashSet::new();
    r.askers.retain(|url, owners| {
        owners.remove(&gone);
        if owners.is_empty() {
            nobody.insert(url.clone());
        }
        !owners.is_empty()
    });
    let Remote { waiting, loading, .. } = &mut *r;
    waiting.retain(|url| {
        let keep = !nobody.contains(url);
        if !keep {
            loading.remove(url);
        }
        keep
    });
    for url in &nobody {
        if let Some(task) = r.tasks.get(url) {
            task.cancel();
        }
    }
}

fn release(r: &mut Remote) {
    for (_, stored) in r.images.drain() {
        super::picture::count_decoded(-crate::util::to_isize(stored.bytes));
    }
    let Remote { waiting, loading, .. } = &mut *r;
    for url in waiting.drain(..) {
        loading.remove(&url);
    }
    r.askers.clear();
    for task in r.tasks.values() {
        task.cancel();
    }
}

/// `Some(None)` means loading failed; `None` means not loaded yet (a load is started).
pub fn get(url: &str, requester: Option<&EditorController>) -> Option<Option<Rendered>> {
    if !is_remote(url) || NSURL::URLWithString(&ns(url)).is_none() {
        return Some(None);
    }
    let requester = requester.filter(|c| !c.has_closed());
    let owner = requester.map(asker);
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
            None => {
                if let Some(owner) = owner {
                    r.askers.entry(url.to_string()).or_default().insert(owner);
                }
                (None, requester.is_some() && r.loading.insert(url.to_string()))
            }
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
    let url = url.to_string();
    let Some(nsurl) = NSURL::URLWithString(&ns(&url)) else {
        main_soon(move || finish(url, None, Failure::Final));
        return;
    };
    let task = SESSION.with(|s| s.dataTaskWithURL(&nsurl));
    if let Ok(mut d) = DOWNLOADS.lock() {
        let download = Download {
            url: url.clone(),
            bytes: Vec::new(),
        };
        d.get_or_insert_with(HashMap::new)
            .insert(task.taskIdentifier(), download);
    }
    task.resume();
    REMOTE.with(|r| r.borrow_mut().tasks.insert(url, task));
}

fn session() -> Retained<NSURLSession> {
    let config = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    // An ephemeral session still keeps cookies until the app quits, which lets a server tell documents apart.
    config.setHTTPCookieAcceptPolicy(NSHTTPCookieAcceptPolicy::Never);
    config.setHTTPShouldSetCookies(false);
    config.setHTTPCookieStorage(None);
    config.setTimeoutIntervalForRequest(20.0);
    config.setTimeoutIntervalForResource(60.0);
    crate::test_hooks::serve_the_web_inside(&config);
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
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn completed(&self, _session: &NSURLSession, task: &NSURLSessionTask, error: Option<&NSError>) {
            guarded((), || {
                let Some(Download { url, bytes }) = DOWNLOADS
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
                    main_soon(move || finish(url, None, failure));
                    return;
                }
                // The session's queue also delivers the data of all other downloads, so decoding must not hold it up.
                super::picture::load_in_background(move || {
                    let room = super::picture::room_for_more();
                    let decoded = room
                        .then(|| {
                            crate::util::contained(|| super::decoder::decode(&bytes, super::picture::allowed_side()))
                        })
                        .flatten()
                        .flatten();
                    let failure = if room { Failure::Final } else { Failure::Passing };
                    main_soon(move || finish(url, decoded, failure));
                });
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
            let too_big = guarded(true, || {
                usize::try_from(response.expectedContentLength()).is_ok_and(|n| n > MAX_BYTES)
            });
            completion.call((if too_big {
                NSURLSessionResponseDisposition::Cancel
            } else {
                NSURLSessionResponseDisposition::Allow
            },));
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn data(&self, _session: &NSURLSession, task: &NSURLSessionDataTask, data: &NSData) {
            let over = guarded(true, || {
                DOWNLOADS.lock().ok().is_none_or(|mut d| {
                    d.as_mut()
                        .and_then(|d| d.get_mut(&task.taskIdentifier()))
                        .is_none_or(|download| {
                            download.bytes.extend_from_slice(&data.to_vec());
                            download.bytes.len() > MAX_BYTES
                        })
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
    let shown = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
        r.loading.remove(&url);
        r.tasks.remove(&url);
        r.running = r.running.saturating_sub(1);
        r.askers.remove(&url).is_some()
    });
    if !shown {
        start_waiting();
        return;
    }
    let bytes = match &decoded {
        Some(Decoded::Rgba { pixels, .. }) => pixels.len(),
        _ => 0,
    };
    super::picture::count_decoded(crate::util::to_isize(bytes));
    let rendered = decoded.and_then(super::decoder::image).map(|image| {
        let size = image.size();
        Rendered {
            image,
            sizing: Sizing::Points { size, descent: 0.0 },
        }
    });
    let listeners = REMOTE.with(|r| {
        let mut r = r.borrow_mut();
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
            super::picture::count_decoded(-crate::util::to_isize(old.bytes));
        }
        r.listeners.iter().filter_map(Weak::load).collect::<Vec<_>>()
    });
    start_waiting();
    for l in listeners {
        l.image_arrived(url.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_that_closes_takes_back_only_what_nobody_else_waits_for() {
        let mut remote = Remote::default();
        for (url, owners) in [("nur eines", vec![1]), ("beide", vec![1, 2]), ("das andere", vec![2])] {
            remote.loading.insert(url.to_string());
            remote.waiting.push_back(url.to_string());
            remote.askers.insert(url.to_string(), owners.into_iter().collect());
        }
        forget_asker(&mut remote, 1);
        assert_eq!(remote.waiting, ["beide", "das andere"]);
        assert!(!remote.loading.contains("nur eines") && remote.loading.contains("beide"));
        assert!(!remote.askers.contains_key("nur eines"));
        assert_eq!(remote.askers["beide"].len(), 1);
    }

    #[test]
    fn what_nobody_shows_any_more_stops_waiting_for_its_turn() {
        let mut remote = Remote::default();
        for url in ["a", "b"] {
            remote.loading.insert(url.to_string());
            remote.waiting.push_back(url.to_string());
        }
        remote.loading.insert("unterwegs".to_string());
        release(&mut remote);
        assert!(remote.waiting.is_empty());
        assert_eq!(remote.loading.into_iter().collect::<Vec<_>>(), ["unterwegs"]);
    }

    #[test]
    fn an_address_on_the_web_is_one_however_its_scheme_is_written() {
        assert!(is_remote("https://a.de/b.png") && is_remote("HTTPS://a.de/b.png"));
        assert!(!is_remote("http://a.de/b.png") && !is_remote("b.png"));
    }

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
}
