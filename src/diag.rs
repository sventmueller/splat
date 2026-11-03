//! What goes wrong out of the user's sight goes to the system log, where Console.app finds it under the app's name. A
//! message names what failed and never holds any text of a document.

use objc2_foundation::NSString;

use crate::util::ns;

unsafe extern "C" {
    fn NSLog(format: *const NSString, ...);
}

pub fn report(what: &str, detail: impl std::fmt::Display) {
    let message = ns(&format!("Splat: {what}: {detail}"));
    // SAFETY: A valid format with one object placeholder and that one object, so nothing in the message is read as format.
    unsafe { NSLog(&*ns("%@"), &*message as *const NSString) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_message_is_not_read_as_a_format() {
        super::report("a test", "100 %d %@ %s");
    }
}
