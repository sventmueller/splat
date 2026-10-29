//! Loads images referenced by https URLs in the background. A document decides which URLs are requested, so only
//! public https hosts are contacted, downloads are size-limited, and the data is decoded by `safe_image`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Mutex;

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
    images: HashMap<String, Option<Rendered>>,
    loading: HashSet<String>,
    listeners: Vec<Weak<EditorController>>,
}

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
/// addresses would let a document send requests to routers and local services.
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
            Some(found) => (Some(found.clone()), false),
            None => (None, requester.is_some() && r.loading.insert(url.to_string())),
        }
    });
    if start {
        load(url);
    }
    found
}

pub(super) fn load(url: &str) {
    let Some(nsurl) = NSURL::URLWithString(&ns(url)).filter(|u| public_host(u)) else {
        let url = url.to_string();
        DispatchQueue::main().exec_async(move || finish(url, None));
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
            let ok = error.is_none()
                && bytes.len() <= MAX_BYTES
                && task
                    .response()
                    .and_then(|r| r.downcast::<NSHTTPURLResponse>().ok())
                    .is_some_and(|r| (200..300).contains(&r.statusCode()));
            let decoded = ok.then(|| super::decoder::decode(&bytes)).flatten();
            DispatchQueue::main().exec_async(move || finish(url, decoded));
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

fn finish(url: String, decoded: Option<Decoded>) {
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
        r.images.insert(url, rendered);
        r.listeners.iter().filter_map(Weak::load).collect::<Vec<_>>()
    });
    for l in listeners {
        l.images_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_https_hosts_are_contacted() {
        let ok = |s: &str| public_host(&NSURL::URLWithString(&ns(s)).unwrap());
        assert!(ok("https://img.shields.io/badge/a-b-blue.svg"));
        assert!(ok("https://93.184.216.34/x.png"));
        for blocked in [
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
