//! Loads images referenced by https URLs in the background. A document decides which URLs are requested, so only
//! public https hosts are contacted, downloads are size-limited, and the data is decoded by `safe_image`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPCookieAcceptPolicy, NSHTTPURLResponse, NSObjectProtocol, NSURL, NSURLRequest, NSURLResponse,
    NSURLSession, NSURLSessionConfiguration, NSURLSessionDataDelegate, NSURLSessionDataTask, NSURLSessionDelegate,
    NSURLSessionResponseDisposition, NSURLSessionTask, NSURLSessionTaskDelegate,
};

use super::EditorController;
use super::picture::{Rendered, Sizing};
use super::safe_image::{Decoded, MAX_BYTES};
use crate::util::{Lane, guarded, main_soon, ns};

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

/// A resolver that stops answering holds its thread for as long as it waits.
static NAMES: Lane = Lane::new("splat-names").with_workers(MAX_RUNNING);
const FIRST_RETRY: Duration = crate::test_hooks::FIRST_RETRY;
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
    crate::markdown::url::is_https(url)
}

/// A host on the public internet as written: requests to `localhost`, `.local` names or addresses that are not public would
/// let a document send requests to routers and local services. What a name resolves to is [`lookup`]'s part.
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
        Ok(IpAddr::V4(ip)) => public_v4(ip),
        Ok(IpAddr::V6(ip)) => public_v6(ip),
        // The system also reads `127.1`, `0x7f.1` or `0177.0.0.1` as addresses, and an IPv6 address with a zone such as
        // `fe80::1%en0`; a real name never ends in a number and holds no colon.
        Err(_) => {
            !host.contains(':')
                && !host
                    .rsplit('.')
                    .next()
                    .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x"))
        }
    }
}

/// Where a download to a name would go.
#[derive(Debug, PartialEq, Eq)]
enum Lookup {
    Public,
    /// At least one address is not public: a public name can point at a router in the local network.
    Private,
    /// The name does not resolve, as when the Mac is offline; the download would ask again and could hear otherwise.
    Unknown,
}

/// What the name of `url` resolves to now. A name whose answer changes before the download looks it up again still gets
/// through.
fn lookup(url: &NSURL) -> Lookup {
    let Some(host) = url.host().map(|h| h.to_string().to_ascii_lowercase()) else {
        return Lookup::Unknown;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<IpAddr> = crate::test_hooks::addresses_of(host).unwrap_or_else(|| {
        (host, 443)
            .to_socket_addrs()
            .map(|found| found.map(|a| a.ip()).collect())
            .unwrap_or_default()
    });
    if addresses.is_empty() {
        Lookup::Unknown
    } else if addresses.into_iter().all(public_address) {
        Lookup::Public
    } else {
        Lookup::Private
    }
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    }
}

/// Whether an IPv4 address belongs to a host on the public internet, by IANA's registry of special-purpose addresses.
fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0 // this network
        || a == 10 // private
        || a == 100 && b & 0xc0 == 64 // shared by carriers
        || a == 127 // loopback
        || a == 169 && b == 254 // link-local
        || a == 172 && b & 0xf0 == 16 // private
        || a == 192 && b == 0 && c == 0 // protocol assignments
        || a == 192 && b == 0 && c == 2 // documentation
        || a == 192 && b == 88 && c == 99 // 6to4 relays
        || a == 192 && b == 168 // private
        || a == 198 && b & 0xfe == 18 // benchmarking
        || a == 198 && b == 51 && c == 100 // documentation
        || a == 203 && b == 0 && c == 113 // documentation
        || a >= 224) // multicast, reserved and broadcast
}

/// Whether an IPv6 address belongs to a host on the public internet: global unicast outside the special-purpose ranges,
/// or an address that reaches a public IPv4 address. IPv4-compatible addresses (`::a.b.c.d`) are deprecated and lie
/// outside global unicast, so they stay blocked whatever they hold.
fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(embedded) = embedded_v4(ip) {
        return public_v4(embedded);
    }
    let [a, b, ..] = ip.segments();
    a & 0xe000 == 0x2000 // global unicast
        && !(a == 0x2001 && b < 0x0200) // protocol assignments: Teredo, benchmarking, ORCHID
        && !(a == 0x2001 && b == 0x0db8) // documentation
        && !(a == 0x3fff && b < 0x1000) // documentation
}

/// The IPv4 address an IPv6 address reaches: mapped (`::ffff:0:0/96`), through the well-known NAT64 prefix
/// (`64:ff9b::/96`) or through 6to4 (`2002::/16`). The local NAT64 prefix `64:ff9b:1::/48` places it where each network
/// chooses, and stays blocked as outside global unicast.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let of = |high: u16, low: u16| Ipv4Addr::from((u32::from(high) << 16) | u32::from(low));
    match s {
        [0, 0, 0, 0, 0, 0xffff, high, low] | [0x64, 0xff9b, 0, 0, 0, 0, high, low] => Some(of(high, low)),
        [0x2002, high, low, ..] => Some(of(high, low)),
        _ => None,
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
    if !NSURL::URLWithString(&ns(url)).is_some_and(|u| public_host(&u)) {
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
    if !NSURL::URLWithString(&ns(&url)).is_some_and(|u| public_host(&u)) {
        main_soon(move || finish(url, None, Failure::Final));
        return;
    }
    NAMES.run(move || {
        let found = NSURL::URLWithString(&ns(&url)).map_or(Lookup::Unknown, |u| lookup(&u));
        main_soon(move || match found {
            Lookup::Public => begin(url),
            Lookup::Private => finish(url, None, Failure::Final),
            Lookup::Unknown => finish(url, None, Failure::Passing),
        });
    });
}

fn begin(url: String) {
    // The documents that asked may have closed while the name was looked up.
    let asked = REMOTE.with(|r| r.borrow().askers.contains_key(&url));
    let Some(nsurl) = NSURL::URLWithString(&ns(&url)).filter(|_| asked) else {
        finish(url, None, Failure::Final);
        return;
    };
    let task = SESSION.with(|s| s.dataTaskWithURL(&nsurl));
    if let Ok(mut d) = DOWNLOADS.lock() {
        d.get_or_insert_with(HashMap::new)
            .insert(task.taskIdentifier(), (url.clone(), Vec::new()));
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
        #[unsafe(method(URLSession:task:willPerformHTTPRedirection:newRequest:completionHandler:))]
        fn redirect(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionTask,
            _response: &NSHTTPURLResponse,
            request: &NSURLRequest,
            completion: &block2::DynBlock<dyn Fn(*mut NSURLRequest)>,
        ) {
            let allowed = guarded(false, || {
                request
                    .URL()
                    .is_some_and(|u| public_host(&u) && lookup(&u) == Lookup::Public)
            });
            let next = if allowed {
                request as *const NSURLRequest as *mut NSURLRequest
            } else {
                std::ptr::null_mut()
            };
            completion.call((next,));
        }

        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn completed(&self, _session: &NSURLSession, task: &NSURLSessionTask, error: Option<&NSError>) {
            guarded((), || {
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
                        .is_none_or(|(_, bytes)| {
                            bytes.extend_from_slice(&data.to_vec());
                            bytes.len() > MAX_BYTES
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

    #[test]
    fn only_addresses_of_the_public_internet_count_as_public() {
        let public = [
            "1.1.1.1",
            "93.184.216.34",
            "9.255.255.255",
            "11.0.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "126.255.255.255",
            "128.0.0.0",
            "169.253.255.255",
            "169.255.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.0.1.0",
            "192.0.3.0",
            "192.88.98.255",
            "192.167.255.255",
            "192.169.0.0",
            "198.17.255.255",
            "198.20.0.0",
            "198.51.99.255",
            "203.0.112.255",
            "223.255.255.255",
            "2606:4700::1111",
            "2a00:1450:4001:80b::200e",
            "2001:200::1",
            "2001:db9::1",
            "2a01:4f8::1",
            "3fff:1000::1",
            "::ffff:93.184.216.34",
            "64:ff9b::93.184.216.34",
            "2002:5db8:d822::1",
        ];
        let blocked = [
            ("0.0.0.0", "unspecified"),
            ("0.1.2.3", "this network"),
            ("10.0.0.1", "private"),
            ("100.64.0.1", "carrier-grade NAT"),
            ("100.127.255.255", "carrier-grade NAT"),
            ("127.0.0.1", "loopback"),
            ("127.255.255.254", "loopback"),
            ("169.254.169.254", "link-local, as cloud metadata"),
            ("172.16.0.1", "private"),
            ("172.31.255.255", "private"),
            ("192.0.0.170", "protocol assignments, NAT64 discovery"),
            ("192.0.2.1", "documentation"),
            ("192.88.99.1", "6to4 relays"),
            ("192.168.0.1", "private"),
            ("198.18.0.1", "benchmarking"),
            ("198.19.255.255", "benchmarking"),
            ("198.51.100.1", "documentation"),
            ("203.0.113.1", "documentation"),
            ("224.0.0.1", "multicast"),
            ("239.255.255.250", "multicast, SSDP"),
            ("240.0.0.1", "reserved"),
            ("255.255.255.255", "broadcast"),
            ("::", "unspecified"),
            ("::1", "loopback"),
            ("::127.0.0.1", "IPv4-compatible"),
            ("::8.8.8.8", "IPv4-compatible, deprecated even when public"),
            ("::ffff:127.0.0.1", "mapped loopback"),
            ("::ffff:192.168.0.1", "mapped private"),
            ("::ffff:0:10.0.0.1", "translated"),
            ("64:ff9b::7f00:1", "NAT64 of loopback"),
            ("64:ff9b::192.168.1.1", "NAT64 of private"),
            ("64:ff9b::a9fe:a9fe", "NAT64 of link-local"),
            ("64:ff9b:1::5db8:d822", "local NAT64"),
            ("2002:7f00:1::1", "6to4 of loopback"),
            ("2002:c0a8:101::1", "6to4 of private"),
            ("2002:e000:1::1", "6to4 of multicast"),
            ("100::1", "discard only"),
            ("2001::1", "Teredo"),
            ("2001:2::1", "benchmarking"),
            ("2001:db8::1", "documentation"),
            ("3fff::1", "documentation"),
            ("5f00::1", "segment routing"),
            ("fc00::1", "unique local"),
            ("fd12:3456::1", "unique local"),
            ("fe80::1", "link-local"),
            ("febf::1", "link-local"),
            ("fec0::1", "site-local"),
            ("ff02::1", "multicast"),
            ("ff0e::1", "global multicast"),
        ];
        let is_public = |address: &str| match address.parse::<IpAddr>().expect(address) {
            IpAddr::V4(ip) => public_v4(ip),
            IpAddr::V6(ip) => public_v6(ip),
        };
        for address in public {
            assert!(is_public(address), "{address}");
        }
        for (address, kind) in blocked {
            assert!(!is_public(address), "{address}: {kind}");
        }
    }

    #[test]
    fn a_name_counts_by_where_it_leads() {
        let of = |s: &str| lookup(&NSURL::URLWithString(&ns(s)).unwrap());
        assert_eq!(of("https://localhost/x.png"), Lookup::Private);
        assert_eq!(of("https://93.184.216.34/x.png"), Lookup::Public);
        assert_eq!(of("https://[2606:4700::1111]/x.png"), Lookup::Public);
        assert_eq!(of("https://[::ffff:192.168.0.1]/x.png"), Lookup::Private);
        assert_eq!(of("https://nicht-da.invalid/x.png"), Lookup::Unknown);
    }

    #[test]
    fn a_host_written_as_an_address_is_read_as_one() {
        let ok = |s: &str| NSURL::URLWithString(&ns(s)).is_some_and(|url| public_host(&url));
        assert!(ok("https://[2606:4700::1111]/x.png"));
        assert!(ok("https://[::ffff:93.184.216.34]/x.png"));
        for blocked in [
            "https://[64:ff9b::7f00:1]/x",
            "https://[2002:c0a8:101::1]/x",
            "https://[ff02::1]/x",
            "https://[::127.0.0.1]/x",
            "https://[fe80::1%25en0]/x",
            "https://224.0.0.251/x",
            "https://198.18.0.1/x",
            "https://0.0.0.0/x",
        ] {
            assert!(!ok(blocked), "{blocked}");
        }
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
