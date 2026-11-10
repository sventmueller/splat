//! The few conversions between Rust and AppKit values that every part needs.

use std::ops::Range;

use objc2::rc::Retained;
use objc2_foundation::{NSRange, NSString};
use splat_markdown::text::{Byte, Utf16Map};

/// Reads a constant that AppKit or Foundation exports.
macro_rules! constant {
    ($name:path) => {
        // SAFETY: the frameworks set their exported constants when they load, before any Rust code runs, and never
        // change them.
        unsafe { $name }
    };
}
pub(crate) use constant;

/// A size as AppKit's signed counts have it; no text or table comes near the difference.
pub fn to_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// A count from AppKit as a size; a negative one, which says "none", counts nothing.
pub fn to_usize(n: isize) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// Upcasts any Objective-C object for heterogeneous containers such as attribute dictionaries.
pub fn obj<T: objc2::ClassType + 'static>(r: Retained<T>) -> Retained<objc2::runtime::AnyObject> {
    r.into()
}

pub fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

/// A range of the source as AppKit counts it, in UTF-16 units.
pub fn byte_range_to_ns(map: &Utf16Map, r: &Range<Byte>) -> NSRange {
    let (start, end) = (map.to_u16(r.start).get(), map.to_u16(r.end).get());
    NSRange::new(start, end.saturating_sub(start))
}
