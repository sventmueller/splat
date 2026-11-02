//! A web server inside the process, for the session that loads remote images: the checks behave the same without a
//! network, and can see how Splat treats answers no real server gives on demand.

use std::collections::HashMap;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{AnyThread, ClassType, Message, define_class, msg_send};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSHTTPURLResponse, NSString, NSURL, NSURLCacheStoragePolicy, NSURLProtocol,
    NSURLProtocolClient, NSURLRequest, NSURLResponse,
};

use crate::util::ns;

pub const HOST: &str = "https://bilder.splat-test.example";

/// Every address requested so far, and how often.
static REQUESTS: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

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
        "/wackelig.svg" if count == 1 => Answer::Body(503, "gleich wieder"),
        "/wackelig.svg" => Answer::Body(200, BADGE),
        "/ohne-netz.svg" if count == 1 => Answer::Unreachable,
        "/ohne-netz.svg" => Answer::Body(200, BADGE),
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
                let length = (super::super::safe_image::MAX_BYTES + 1).to_string();
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
