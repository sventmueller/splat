//! A web server inside the process, for the session that loads remote images: the checks behave the same without a
//! network, and can see how Splat treats answers no real server gives on demand.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{AnyThread, ClassType, Message, define_class, msg_send};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSHTTPURLResponse, NSString, NSURL, NSURLCacheStoragePolicy, NSURLProtocol,
    NSURLProtocolClient, NSURLRequest, NSURLResponse,
};

use crate::util::ns;

pub const HOST: &str = "https://bilder.splat-test.example";

/// A public name whose address lies in the local network, as a router's can.
pub const HOME_HOST: &str = "https://router.splat-test.example";

/// What the hosts of this server resolve to.
pub fn addresses_of(host: &str) -> Option<Vec<std::net::IpAddr>> {
    let address = match host {
        "bilder.splat-test.example" => [93, 184, 216, 34],
        "router.splat-test.example" => [192, 168, 178, 1],
        // A name the resolver has no answer for, as while the network is away.
        "weg.splat-test.example" => return Some(Vec::new()),
        "zaeh.splat-test.example" => {
            wait_until_freed();
            [93, 184, 216, 34]
        }
        _ => return None,
    };
    Some(vec![std::net::IpAddr::from(address)])
}

static SLOW_NAME_ASKED: AtomicBool = AtomicBool::new(false);
static SLOW_NAME_FREED: AtomicBool = AtomicBool::new(false);
static SLOW_NAME_IN_VAIN: AtomicBool = AtomicBool::new(false);

/// Holds the lookup of a slow name until the probe frees it, as a resolver that stops answering holds its caller.
fn wait_until_freed() {
    SLOW_NAME_ASKED.store(true, Ordering::SeqCst);
    let asked = Instant::now();
    while !SLOW_NAME_FREED.load(Ordering::SeqCst) {
        if asked.elapsed() > Duration::from_secs(20) {
            SLOW_NAME_IN_VAIN.store(true, Ordering::SeqCst);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn slow_name_asked() -> bool {
    SLOW_NAME_ASKED.load(Ordering::SeqCst)
}

pub fn free_slow_name() {
    SLOW_NAME_FREED.store(true, Ordering::SeqCst);
}

/// The probe did not free the slow name in time.
pub fn slow_name_waited_in_vain() -> bool {
    SLOW_NAME_IN_VAIN.load(Ordering::SeqCst)
}

/// Every address requested so far, and how often.
static REQUESTS: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// What starts with this takes long enough to answer that a document can close meanwhile.
pub const SLOW: &str = "/langsam";

/// How many requests went to addresses that hold `part`.
pub fn requests_holding(part: &str) -> usize {
    REQUESTS.lock().ok().map_or(0, |r| {
        r.as_ref().map_or(0, |all| {
            all.iter().filter(|(url, _)| url.contains(part)).map(|(_, n)| n).sum()
        })
    })
}

pub fn requests_of(url: &str) -> usize {
    REQUESTS
        .lock()
        .ok()
        .and_then(|r| r.as_ref()?.get(url).copied())
        .unwrap_or(0)
}

const BADGE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"90\" height=\"20\"><rect width=\"90\" height=\"20\" fill=\"#07c\"/></svg>";

enum Answer {
    Body(u16, &'static str),
    TooLarge,
    Unreachable,
    Redirect(&'static str),
}

/// The answer to the `count`th request of `path`.
fn answer(path: &str, count: usize) -> Answer {
    match path {
        "/badge.svg" => Answer::Body(200, BADGE),
        "/fehlt.png" => Answer::Body(404, "nicht da"),
        "/kein-bild.png" => Answer::Body(200, "<html>keine Grafik</html>"),
        "/zu-gross.png" => Answer::TooLarge,
        "/umleitung.png" => Answer::Redirect("https://127.0.0.1/heimlich.png"),
        "/umleitung-nach-hause.png" => Answer::Redirect("https://router.splat-test.example/router.png"),
        "/umleitung-weiter.svg" => Answer::Redirect("https://bilder.splat-test.example/badge.svg"),
        "/umleitung-ins-leere.svg" if count == 1 => Answer::Redirect("https://weg.splat-test.example/badge.svg"),
        "/umleitung-ins-leere.svg" => Answer::Body(200, BADGE),
        "/umleitung-zaeh.svg" => Answer::Redirect("https://zaeh.splat-test.example/badge.svg"),
        "/waehrend-der-suche.svg" => Answer::Body(200, BADGE),
        "/wackelig.svg" | "/nicht-gezeigt.svg" if count == 1 => Answer::Body(503, "gleich wieder"),
        "/wackelig.svg" | "/nicht-gezeigt.svg" => Answer::Body(200, BADGE),
        "/ohne-netz.svg" if count == 1 => Answer::Unreachable,
        "/ohne-netz.svg" => Answer::Body(200, BADGE),
        path if path.starts_with(SLOW) => Answer::Body(200, BADGE),
        _ => Answer::Body(404, ""),
    }
}

define_class!(
    #[unsafe(super(NSURLProtocol))]
    #[name = "MDMockWeb"]
    struct MockWeb;

    impl MockWeb {
        #[unsafe(method(canInitWithRequest:))]
        fn can_init_with_request(_request: &NSURLRequest) -> bool {
            true
        }

        #[unsafe(method_id(canonicalRequestForRequest:))]
        fn canonical_request(request: &NSURLRequest) -> Retained<NSURLRequest> {
            request.retain()
        }

        #[unsafe(method(startLoading))]
        fn start_loading(&self) {
            self.respond();
        }

        #[unsafe(method(stopLoading))]
        fn stop_loading(&self) {}
    }
);

impl MockWeb {
    fn respond(&self) {
        let request = self.request();
        let (Some(client), Some(url)) = (self.client(), request.URL()) else {
            return;
        };
        let address = url.absoluteString().map(|s| s.to_string()).unwrap_or_default();
        let count = REQUESTS.lock().map_or(1, |mut r| {
            let seen = r.get_or_insert_with(HashMap::new).entry(address.clone()).or_insert(0);
            *seen += 1;
            *seen
        });
        let path = url.path().map(|p| p.to_string()).unwrap_or_default();
        if path.starts_with(SLOW) {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let this: &NSURLProtocol = self;
        let response = |status: u16, headers: &[(&str, &str)]| -> Retained<NSURLResponse> {
            let keys: Vec<Retained<NSString>> = headers.iter().map(|(k, _)| ns(k)).collect();
            let values: Vec<Retained<NSString>> = headers.iter().map(|(_, v)| ns(v)).collect();
            let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
            let fields = NSDictionary::from_retained_objects(&keys, &values);
            let answer = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                NSHTTPURLResponse::alloc(),
                &url,
                isize::try_from(status).unwrap_or(500),
                Some(&ns("HTTP/1.1")),
                Some(&fields),
            )
            .expect("a response");
            answer.into_super()
        };
        let not_cached = NSURLCacheStoragePolicy::NotAllowed;
        match answer(&path, count) {
            Answer::Body(status, body) => {
                client.URLProtocol_didReceiveResponse_cacheStoragePolicy(this, &response(status, &[]), not_cached);
                client.URLProtocol_didLoadData(this, &NSData::with_bytes(body.as_bytes()));
                client.URLProtocolDidFinishLoading(this);
            }
            Answer::TooLarge => {
                let length = (super::super::decoder::MAX_BYTES + 1).to_string();
                let headers = [("Content-Length", length.as_str())];
                client.URLProtocol_didReceiveResponse_cacheStoragePolicy(this, &response(200, &headers), not_cached);
                client.URLProtocolDidFinishLoading(this);
            }
            Answer::Unreachable => {
                // NSURLErrorNotConnectedToInternet
                let error = NSError::new(-1009, &ns("NSURLErrorDomain"));
                client.URLProtocol_didFailWithError(this, &error);
            }
            Answer::Redirect(target) => {
                let next = NSURL::URLWithString(&ns(target)).expect("a target");
                let redirected = NSURLRequest::requestWithURL(&next);
                let headers = [("Location", target)];
                client.URLProtocol_wasRedirectedToRequest_redirectResponse(this, &redirected, &response(302, &headers));
                client.URLProtocolDidFinishLoading(this);
            }
        }
    }
}

/// The classes for a session configuration that loads from here.
pub fn protocol_classes() -> Retained<NSArray<AnyClass>> {
    let class: &AnyClass = MockWeb::class();
    // SAFETY: A class is an object, and NSArray retains it as any other.
    unsafe { msg_send![<NSArray<AnyObject> as ClassType>::class(), arrayWithObject: class] }
}
