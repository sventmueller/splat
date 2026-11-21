//! The scheme the bundled pages and everything they load come through: the files of the resources folder and nothing
//! outside it. A library the bundle carries compressed lies there as `<name>.lzfse`, the length of the library as eight
//! bytes little-endian and then its LZFSE stream, and comes unpacked; a folder without such files serves the plain ones.
//! WebKit asks on the main thread, the file is read and unpacked on a lane of its own, and the answer goes only to a task
//! WebKit has not stopped meanwhile, as it raises an exception for any other.

use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::{c_int, c_void};
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{AnyThread, MainThreadOnly, Message, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSObjectProtocol, NSString, NSURL, NSURLErrorDomain, NSURLErrorFileDoesNotExist, NSURLResponse,
};
use objc2_web_kit::{WKURLSchemeHandler, WKURLSchemeTask, WKWebView, WKWebViewConfiguration};

use crate::appkit::{constant, ns, to_isize};
use crate::lanes::{Lane, guarded, main_soon};

/// The scheme of the pages' addresses.
pub const SCHEME: &str = "splat";

/// The host of the pages' addresses: WebKit counts a page from localhost as a secure context, as it did a page from a
/// file.
const HOST: &str = "localhost";

/// What a compressed file's name adds to the name it is asked for by.
const PACKED: &str = ".lzfse";

/// The most a compressed file may say it unpacks to, so that a broken one asks for no absurd amount of memory.
const LARGEST: u64 = 256 << 20;

/// `COMPRESSION_LZFSE` of `<compression.h>`.
const LZFSE: c_int = 0x801;

#[link(name = "compression")]
unsafe extern "C" {
    fn compression_decode_buffer(
        dst: *mut u8,
        dst_size: usize,
        src: *const u8,
        src_size: usize,
        scratch: *mut c_void,
        algorithm: c_int,
    ) -> usize;
    #[cfg(test)]
    fn compression_encode_buffer(
        dst: *mut u8,
        dst_size: usize,
        src: *const u8,
        src_size: usize,
        scratch: *mut c_void,
        algorithm: c_int,
    ) -> usize;
}

/// Reads and unpacks what the pages ask for; a lane of its own, so that a page waits for no picture.
static FILES: Lane = Lane::new("splat-pages");

/// What WebKit is to take a file for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Kind {
    mime: &'static str,
    /// Text in UTF-8.
    text: bool,
}

/// The address of `page` of the resources.
pub fn address(page: &str) -> String {
    format!("{SCHEME}://{HOST}/{page}")
}

/// Whether `url` is the address of `page`, whatever query or fragment it carries.
pub fn is_page(url: &NSURL, page: &str) -> bool {
    let is = |part: Option<Retained<NSString>>, wanted: &str| {
        part.is_some_and(|p| p.to_string().eq_ignore_ascii_case(wanted))
    };
    is(url.scheme(), SCHEME) && is(url.host(), HOST) && url.path().is_some_and(|p| p.to_string() == format!("/{page}"))
}

/// The file an address's decoded `path` names below the resources; none for a path that would leave them, or that names
/// a folder.
fn relative(path: &str) -> Option<PathBuf> {
    let mut parts = path.split('/');
    if parts.next() != Some("") {
        return None;
    }
    let mut relative = PathBuf::new();
    for part in parts {
        if matches!(part, "" | "." | "..") || part.contains('\0') {
            return None;
        }
        relative.push(part);
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

/// What `path` holds in `resources`, unpacked; `None` for anything that is not a file within them, also through a link.
fn contents(resources: &Path, path: &str) -> Option<Vec<u8>> {
    let relative = relative(path)?;
    let folder = resources.canonicalize().ok()?;
    let read = |file: PathBuf| {
        let real = file.canonicalize().ok().filter(|real| real.starts_with(&folder))?;
        crate::files::read_regular_file(&real, LARGEST)
    };
    let plain = resources.join(relative);
    let mut packed = plain.clone().into_os_string();
    packed.push(PACKED);
    let packed = PathBuf::from(packed);
    if packed.exists() {
        return unpack(&read(packed)?);
    }
    read(plain)
}

/// The bytes of a compressed file: their length, then their LZFSE stream.
fn unpack(packed: &[u8]) -> Option<Vec<u8>> {
    let (length, stream) = packed.split_first_chunk::<8>()?;
    let length = Some(u64::from_le_bytes(*length))
        .filter(|&n| n <= LARGEST)
        .and_then(|n| usize::try_from(n).ok())?;
    // The decoder stops at the end of the buffer without saying so; a byte to spare shows a stream that holds more.
    let mut bytes = vec![0; length + 1];
    // SAFETY: Both buffers are valid for the lengths given; without a scratch buffer the library allocates its own.
    let written = unsafe {
        compression_decode_buffer(
            bytes.as_mut_ptr(),
            bytes.len(),
            stream.as_ptr(),
            stream.len(),
            std::ptr::null_mut(),
            LZFSE,
        )
    };
    (written == length).then(|| {
        bytes.truncate(length);
        bytes
    })
}

fn kind(path: &str) -> Kind {
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let (mime, text) = match extension.as_deref() {
        Some("html") => ("text/html", true),
        Some("js") => ("text/javascript", true),
        Some("css") => ("text/css", true),
        Some("json") => ("application/json", true),
        Some("svg") => ("image/svg+xml", true),
        Some("woff2") => ("font/woff2", false),
        Some("wasm") => ("application/wasm", false),
        _ => ("application/octet-stream", false),
    };
    Kind { mime, text }
}

type Task = ProtocolObject<dyn WKURLSchemeTask>;

thread_local! {
    static HANDLER: OnceCell<Retained<Scheme>> = const { OnceCell::new() };
    /// The tasks still to be answered, by number; WebKit's stop takes one out.
    static RUNNING: RefCell<Vec<(u64, Retained<Task>)>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDResourceScheme"]
    struct Scheme;

    unsafe impl NSObjectProtocol for Scheme {}

    unsafe impl WKURLSchemeHandler for Scheme {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(&self, _web: &WKWebView, task: &Task) {
            guarded((), || start(task));
        }

        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(&self, _web: &WKWebView, task: &Task) {
            guarded((), || stop(task));
        }
    }
);

/// Lets the pages of `config` load the resources through `SCHEME`.
pub fn serve(config: &WKWebViewConfiguration) {
    let mtm = config.mtm();
    let handler = HANDLER.with(|handler| {
        handler
            .get_or_init(|| {
                // SAFETY: NSObject's initializer, called once on the newly allocated object.
                unsafe { msg_send![super(Scheme::alloc(mtm).set_ivars(())), init] }
            })
            .clone()
    });
    // SAFETY: A scheme WebKit leaves to the app, set once on a configuration that no view uses yet; the configuration
    // keeps its handler.
    unsafe { config.setURLSchemeHandler_forURLScheme(Some(ProtocolObject::from_ref(&*handler)), &ns(SCHEME)) };
}

fn start(task: &Task) {
    // SAFETY: WebKit hands over a task with the request it is for.
    let request = unsafe { task.request() };
    // A request without a method has the default one, GET.
    let reads = request.HTTPMethod().is_none_or(|method| method.to_string() == "GET");
    let path = request.URL().and_then(|url| url.path()).map(|path| path.to_string());
    let Some(path) = path.filter(|_| reads) else {
        return fail(task);
    };
    let kind = kind(&path);
    let number = remember(task);
    let resources = &crate::host().resources;
    FILES.ask(
        move || contents(resources, &path),
        move |found| main_soon(move || answer(number, kind, found.flatten())),
    );
}

fn remember(task: &Task) -> u64 {
    let number = NEXT.replace(NEXT.get() + 1);
    RUNNING.with_borrow_mut(|running| running.push((number, task.retain())));
    number
}

fn stop(task: &Task) {
    RUNNING.with_borrow_mut(|running| running.retain(|(_, held)| !std::ptr::eq(&**held, task)));
}

/// Answers task `number` with what it asked for, if WebKit has not stopped it.
fn answer(number: u64, kind: Kind, found: Option<Vec<u8>>) {
    let task = RUNNING.with_borrow_mut(|running| {
        let at = running.iter().position(|(n, _)| *n == number)?;
        Some(running.swap_remove(at).1)
    });
    let Some(task) = task else {
        return;
    };
    let response = found.as_ref().and_then(|bytes| response(&task, kind, bytes.len()));
    let Some((bytes, response)) = found.zip(response) else {
        return fail(&task);
    };
    // SAFETY: The task is running and hears its response, then its data, then its end, each once.
    unsafe {
        task.didReceiveResponse(&response);
        task.didReceiveData(&NSData::from_vec(bytes));
        task.didFinish();
    }
}

fn response(task: &Task, kind: Kind, length: usize) -> Option<Retained<NSURLResponse>> {
    // SAFETY: WebKit hands over a task with the request it is for.
    let url = unsafe { task.request() }.URL()?;
    let encoding = kind.text.then(|| ns("utf-8"));
    Some(
        NSURLResponse::initWithURL_MIMEType_expectedContentLength_textEncodingName(
            NSURLResponse::alloc(),
            &url,
            Some(&ns(kind.mime)),
            to_isize(length),
            encoding.as_deref(),
        ),
    )
}

fn fail(task: &Task) {
    let error = NSError::new(NSURLErrorFileDoesNotExist, constant!(NSURLErrorDomain));
    // SAFETY: The task is running, and failing is the last it hears.
    unsafe { task.didFailWithError(&error) };
}

#[cfg(test)]
mod tests {
    use objc2::DefinedClass;
    use objc2_foundation::{NSMutableURLRequest, NSURLRequest};

    use super::*;

    fn pack(bytes: &[u8]) -> Vec<u8> {
        let mut stream = vec![0; bytes.len() + 4096];
        // SAFETY: Both buffers are valid for the lengths given; without a scratch buffer the library allocates its own.
        let written = unsafe {
            compression_encode_buffer(
                stream.as_mut_ptr(),
                stream.len(),
                bytes.as_ptr(),
                bytes.len(),
                std::ptr::null_mut(),
                LZFSE,
            )
        };
        assert!(written > 0, "LZFSE packs {} bytes", bytes.len());
        stream.truncate(written);
        let mut packed = (bytes.len() as u64).to_le_bytes().to_vec();
        packed.extend(stream);
        packed
    }

    fn script() -> Vec<u8> {
        "const formula = 'a^2 + b^2 = c^2';\n".repeat(4000).into_bytes()
    }

    #[test]
    fn a_compressed_file_unpacks_only_to_the_length_it_names() {
        let script = script();
        let packed = pack(&script);
        assert!(packed.len() < script.len() / 10, "{} bytes", packed.len());
        assert_eq!(unpack(&packed).as_deref(), Some(&script[..]));
        let with_length = |length: u64| {
            let mut changed = packed.clone();
            changed[..8].copy_from_slice(&length.to_le_bytes());
            unpack(&changed)
        };
        assert_eq!(with_length(script.len() as u64 - 1), None, "a stream that holds more");
        assert_eq!(with_length(script.len() as u64 + 1), None, "a stream that holds less");
        assert_eq!(with_length(LARGEST + 1), None, "more than any library");
        assert_eq!(unpack(&packed[..7]), None, "no length");
        assert_eq!(unpack(&packed[..packed.len() / 2]), None, "half a stream");
    }

    #[test]
    fn an_address_leads_only_to_a_file_below_the_resources() {
        assert_eq!(relative("/render.html"), Some(PathBuf::from("render.html")));
        assert_eq!(
            relative("/js/mathjax/input/tex/extensions/mhchem.js"),
            Some(PathBuf::from("js/mathjax/input/tex/extensions/mhchem.js"))
        );
        for leaves in [
            "/js/../../Info.plist",
            "/../Info.plist",
            "/./render.html",
            "//etc/passwd",
            "/js//render.js",
            "/js/",
            "/",
            "",
            "render.html",
            "/a\0b.js",
        ] {
            assert_eq!(relative(leaves), None, "{leaves:?}");
        }
    }

    #[test]
    fn only_a_file_within_the_resources_is_read_and_a_compressed_one_comes_unpacked() {
        let dir = std::env::temp_dir().join(format!("splat-scheme-{}", std::process::id()));
        let resources = dir.join("Resources");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(resources.join("js")).unwrap();
        std::fs::write(resources.join("render.html"), "<!doctype html>").unwrap();
        let script = script();
        std::fs::write(resources.join("js/mermaid.min.js.lzfse"), pack(&script)).unwrap();
        std::fs::write(
            resources.join("js/broken.js.lzfse"),
            [&5u64.to_le_bytes()[..], b"xyz"].concat(),
        )
        .unwrap();
        std::fs::write(dir.join("Info.plist"), "<plist/>").unwrap();
        std::os::unix::fs::symlink(dir.join("Info.plist"), resources.join("js/out.js")).unwrap();
        std::os::unix::fs::symlink(resources.join("render.html"), resources.join("js/in.html")).unwrap();
        assert_eq!(
            contents(&resources, "/render.html").as_deref(),
            Some(&b"<!doctype html>"[..])
        );
        assert_eq!(contents(&resources, "/js/mermaid.min.js").as_deref(), Some(&script[..]));
        assert_eq!(
            contents(&resources, "/js/in.html").as_deref(),
            Some(&b"<!doctype html>"[..])
        );
        assert_eq!(contents(&resources, "/js/out.js"), None, "a link out of the resources");
        assert_eq!(contents(&resources, "/js/../../Info.plist"), None);
        assert_eq!(contents(&resources, "/js/broken.js"), None);
        assert_eq!(contents(&resources, "/js/nosuch.js"), None);
        assert_eq!(contents(&resources, "/js"), None, "a folder");
        assert_eq!(contents(&dir.join("nowhere"), "/render.html"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn each_file_has_its_type() {
        let mime = |path: &str| (kind(path).mime, kind(path).text);
        assert_eq!(mime("/render.html"), ("text/html", true));
        assert_eq!(mime("/js/mermaid.min.js"), ("text/javascript", true));
        assert_eq!(mime("/style.CSS"), ("text/css", true));
        assert_eq!(mime("/data.json"), ("application/json", true));
        assert_eq!(mime("/picture.svg"), ("image/svg+xml", true));
        assert_eq!(
            mime("/js/mathlive-fonts/KaTeX_Main-Regular.woff2"),
            ("font/woff2", false)
        );
        assert_eq!(mime("/graphviz.wasm"), ("application/wasm", false));
        assert_eq!(mime("/js/LICENSE-mermaid"), ("application/octet-stream", false));
        assert_eq!(mime("/js.d/LICENSE"), ("application/octet-stream", false));
    }

    #[test]
    fn only_the_page_itself_is_the_page() {
        let url = |text: &str| NSURL::URLWithString(&ns(text)).expect(text);
        assert!(is_page(&url(&address("render.html")), "render.html"));
        assert!(is_page(&url("SPLAT://LocalHost/render.html#top"), "render.html"));
        assert!(!is_page(&url(&address("mathfield.html")), "render.html"));
        assert!(!is_page(&url(&address("js/render.html")), "render.html"));
        assert!(!is_page(&url("splat://elsewhere/render.html"), "render.html"));
        assert!(!is_page(
            &url("file:///Applications/Splat.app/Contents/Resources/render.html"),
            "render.html"
        ));
        assert!(!is_page(&url("https://localhost/render.html"), "render.html"));
    }

    struct Heard {
        request: Retained<NSURLRequest>,
        calls: RefCell<Vec<String>>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "MDTestResourceTask"]
        #[ivars = Heard]
        struct FakeTask;

        unsafe impl NSObjectProtocol for FakeTask {}

        unsafe impl WKURLSchemeTask for FakeTask {
            #[unsafe(method_id(request))]
            fn its_request(&self) -> Retained<NSURLRequest> {
                self.ivars().request.clone()
            }

            #[unsafe(method(didReceiveResponse:))]
            fn heard_response(&self, response: &NSURLResponse) {
                let mime = response.MIMEType().map(|m| m.to_string()).unwrap_or_default();
                let encoding = response.textEncodingName().map(|e| e.to_string()).unwrap_or_default();
                let length = response.expectedContentLength();
                self.heard(format!("response {mime} {encoding} {length}"));
            }

            #[unsafe(method(didReceiveData:))]
            fn heard_data(&self, data: &NSData) {
                self.heard(format!("data {}", data.len()));
            }

            #[unsafe(method(didFinish))]
            fn heard_finish(&self) {
                self.heard("finish".into());
            }

            #[unsafe(method(didFailWithError:))]
            fn heard_failure(&self, error: &NSError) {
                self.heard(format!("failure {}", error.code()));
            }
        }
    );

    impl FakeTask {
        fn new(request: Retained<NSURLRequest>) -> Retained<FakeTask> {
            let heard = Heard {
                request,
                calls: RefCell::default(),
            };
            // SAFETY: NSObject's initializer, called once on the newly allocated object.
            unsafe { msg_send![super(FakeTask::alloc().set_ivars(heard)), init] }
        }

        fn for_page(page: &str) -> Retained<FakeTask> {
            let url = NSURL::URLWithString(&ns(&address(page))).expect("an address");
            FakeTask::new(NSURLRequest::requestWithURL(&url))
        }

        fn heard(&self, call: String) {
            self.ivars().calls.borrow_mut().push(call);
        }

        fn calls(&self) -> Vec<String> {
            self.ivars().calls.borrow().clone()
        }
    }

    #[test]
    fn a_task_hears_its_response_its_data_and_its_end() {
        let task = FakeTask::for_page("js/render.js");
        let number = remember(ProtocolObject::from_ref(&*task));
        answer(number, kind("/js/render.js"), Some(b"load();".to_vec()));
        assert_eq!(task.calls(), ["response text/javascript utf-8 7", "data 7", "finish"]);
        answer(number, kind("/js/render.js"), Some(b"load();".to_vec()));
        assert_eq!(task.calls().len(), 3, "a task answered hears nothing more");
    }

    #[test]
    fn a_task_for_what_is_not_there_fails() {
        let task = FakeTask::for_page("js/nosuch.js");
        let number = remember(ProtocolObject::from_ref(&*task));
        answer(number, kind("/js/nosuch.js"), None);
        assert_eq!(task.calls(), [format!("failure {NSURLErrorFileDoesNotExist}")]);
    }

    #[test]
    fn a_stopped_task_hears_nothing() {
        let (stopped, other) = (FakeTask::for_page("js/a.js"), FakeTask::for_page("js/b.js"));
        let first = remember(ProtocolObject::from_ref(&*stopped));
        let second = remember(ProtocolObject::from_ref(&*other));
        stop(ProtocolObject::from_ref(&*stopped));
        answer(first, kind("/js/a.js"), Some(b"a".to_vec()));
        answer(second, kind("/js/b.js"), None);
        assert!(stopped.calls().is_empty(), "{:?}", stopped.calls());
        assert_eq!(other.calls().len(), 1, "the other one still runs");
    }

    #[test]
    fn a_request_that_does_not_read_fails_at_once() {
        let url = NSURL::URLWithString(&ns(&address("render.html"))).expect("an address");
        let request = NSMutableURLRequest::requestWithURL(&url);
        request.setHTTPMethod(&ns("POST"));
        let task = FakeTask::new(Retained::into_super(request));
        start(ProtocolObject::from_ref(&*task));
        assert_eq!(task.calls(), [format!("failure {NSURLErrorFileDoesNotExist}")]);
        assert!(RUNNING.with_borrow(Vec::is_empty));
    }
}
