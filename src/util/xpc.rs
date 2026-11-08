//! The few calls of libxpc that hand a socket from the app to its decoding service, which macOS starts on demand from
//! the app bundle in a sandbox of its own.

use std::ffi::{CString, c_char, c_int, c_void};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::OnceLock;

use block2::{Block, RcBlock};

type Object = *mut c_void;

unsafe extern "C" {
    fn xpc_connection_create(name: *const c_char, queue: *mut c_void) -> Object;
    fn xpc_connection_set_event_handler(connection: Object, handler: &Block<dyn Fn(Object)>);
    fn xpc_connection_resume(connection: Object);
    fn xpc_connection_cancel(connection: Object);
    fn xpc_connection_get_pid(connection: Object) -> libc::pid_t;
    fn xpc_connection_send_message(connection: Object, message: Object);
    fn xpc_connection_send_message_with_reply_sync(connection: Object, message: Object) -> Object;
    fn xpc_dictionary_create(keys: *const *const c_char, values: *const Object, count: usize) -> Object;
    fn xpc_dictionary_create_reply(original: Object) -> Object;
    fn xpc_dictionary_get_remote_connection(dictionary: Object) -> Object;
    fn xpc_dictionary_set_fd(dictionary: Object, key: *const c_char, fd: c_int);
    fn xpc_dictionary_dup_fd(dictionary: Object, key: *const c_char) -> c_int;
    fn xpc_dictionary_set_bool(dictionary: Object, key: *const c_char, value: bool);
    fn xpc_dictionary_get_bool(dictionary: Object, key: *const c_char) -> bool;
    fn xpc_get_type(object: Object) -> *const c_void;
    fn xpc_release(object: Object);
    fn xpc_main(handler: extern "C" fn(Object)) -> !;
    static _xpc_type_dictionary: u8;
}

const STREAM: &std::ffi::CStr = c"stream";
const NATIVE: &std::ffi::CStr = c"native";
const TAKEN: &std::ffi::CStr = c"taken";

/// What the app may tell the service besides handing it a socket.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    /// Quit, as after a request that ran over its limits; the next socket starts the service anew.
    End,
    /// Stop answering without quitting, as a hang would; for the self-test.
    Stop,
}

impl Order {
    fn key(self) -> &'static std::ffi::CStr {
        match self {
            Order::End => c"end",
            Order::Stop => c"stop",
        }
    }
}

fn is_dictionary(object: Object) -> bool {
    // SAFETY: Any object or null may be asked for its type, and the type object is a static of libxpc.
    !object.is_null() && unsafe { xpc_get_type(object) == (&raw const _xpc_type_dictionary).cast() }
}

/// A new empty message, released when dropped.
struct Message(Object);

impl Message {
    fn new() -> Message {
        // SAFETY: An empty dictionary: no keys and no values to read.
        Message(unsafe { xpc_dictionary_create(std::ptr::null(), std::ptr::null(), 0) })
    }
}

impl Drop for Message {
    fn drop(&mut self) {
        // SAFETY: The dictionary this message created and owns.
        unsafe { xpc_release(self.0) };
    }
}

/// A connection to the XPC service in the app bundle with this identifier.
pub struct Connection(Object);

// SAFETY: libxpc objects are reference counted with atomic operations and their calls may come from any thread.
unsafe impl Send for Connection {}
// SAFETY: As above; the connection is only read through `&self`.
unsafe impl Sync for Connection {}

impl Connection {
    pub fn to_service(name: &str) -> Option<Connection> {
        let name = CString::new(name).ok()?;
        // SAFETY: A NUL-terminated name; without a queue libxpc uses a queue of its own for the events.
        let connection = unsafe { xpc_connection_create(name.as_ptr(), std::ptr::null_mut()) };
        if connection.is_null() {
            return None;
        }
        // Errors such as a service that quit need nothing here: the next message starts it again.
        let ignore = RcBlock::new(|_: Object| {});
        // SAFETY: A connection just created, not resumed yet; libxpc copies the handler.
        unsafe {
            xpc_connection_set_event_handler(connection, &ignore);
            xpc_connection_resume(connection);
        }
        Some(Connection(connection))
    }

    /// Hands `stream` to the service, which answers requests on it, of the system's decoders with `native`. The process
    /// of the service once it has taken the socket.
    pub fn hand_over(&self, stream: OwnedFd, native: bool) -> Option<i32> {
        let message = Message::new();
        // SAFETY: The message is a dictionary; libxpc duplicates the descriptor, which `stream` closes after.
        unsafe {
            xpc_dictionary_set_fd(message.0, STREAM.as_ptr(), stream.as_raw_fd());
            xpc_dictionary_set_bool(message.0, NATIVE.as_ptr(), native);
        }
        drop(stream);
        // SAFETY: A resumed connection and a dictionary; the reply comes back retained.
        let reply = Message(unsafe { xpc_connection_send_message_with_reply_sync(self.0, message.0) });
        // SAFETY: A dictionary is read only after its type was checked.
        let taken = is_dictionary(reply.0) && unsafe { xpc_dictionary_get_bool(reply.0, TAKEN.as_ptr()) };
        // SAFETY: The connection has exchanged a message, so it knows the process at its other end.
        taken.then(|| unsafe { xpc_connection_get_pid(self.0) })
    }

    pub fn tell(&self, order: Order) {
        let message = Message::new();
        // SAFETY: The message is a dictionary, the connection resumed.
        unsafe {
            xpc_dictionary_set_bool(message.0, order.key().as_ptr(), true);
            xpc_connection_send_message(self.0, message.0);
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: The connection this value created and owns, cancelled once before its last release.
        unsafe {
            xpc_connection_cancel(self.0);
            xpc_release(self.0);
        }
    }
}

static TAKE: OnceLock<fn(OwnedFd, bool)> = OnceLock::new();
static END: OnceLock<fn(i32)> = OnceLock::new();

/// Runs the service: every socket the app hands over goes to `take` on a thread of its own, with whether it is native;
/// `end` quits the process when the app asks.
pub fn serve(take: fn(OwnedFd, bool), end: fn(i32)) -> ! {
    let _ = TAKE.set(take);
    let _ = END.set(end);
    // SAFETY: `accept` has the signature libxpc calls for each new connection.
    unsafe { xpc_main(accept) }
}

/// The decoding service has none of the app's guards: a panic in what libxpc calls ends there, not in libxpc.
fn guarded(f: impl FnOnce()) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
}

extern "C" fn accept(peer: Object) {
    guarded(|| {
        let handler = RcBlock::new(|message: Object| guarded(|| receive(message)));
        // SAFETY: A new connection libxpc hands over, not resumed yet; libxpc copies the handler.
        unsafe {
            xpc_connection_set_event_handler(peer, &handler);
            xpc_connection_resume(peer);
        }
    });
}

fn receive(message: Object) {
    if !is_dictionary(message) {
        return;
    }
    // SAFETY: A dictionary libxpc hands over for the length of the call.
    let told = |order: Order| unsafe { xpc_dictionary_get_bool(message, order.key().as_ptr()) };
    if told(Order::End) {
        match END.get() {
            Some(end) => end(0),
            None => std::process::exit(0),
        }
    }
    if told(Order::Stop) {
        // SAFETY: A plain system call on this process.
        unsafe { libc::kill(libc::getpid(), libc::SIGSTOP) };
        return;
    }
    // SAFETY: As above; a key without a descriptor gives -1.
    let fd = unsafe { xpc_dictionary_dup_fd(message, STREAM.as_ptr()) };
    if fd < 0 {
        return;
    }
    // SAFETY: A descriptor libxpc duplicated for this process alone.
    let stream = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: As above.
    let native = unsafe { xpc_dictionary_get_bool(message, NATIVE.as_ptr()) };
    if let Some(take) = TAKE.get().copied() {
        std::thread::spawn(move || take(stream, native));
    }
    // SAFETY: The reply to a message that asked for one, sent back over the connection it came on.
    unsafe {
        let reply = xpc_dictionary_create_reply(message);
        if !reply.is_null() {
            xpc_dictionary_set_bool(reply, TAKEN.as_ptr(), true);
            xpc_connection_send_message(xpc_dictionary_get_remote_connection(message), reply);
            xpc_release(reply);
        }
    }
}
