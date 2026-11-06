//! What reading and writing Office Open XML files share: ZIP archives read from anywhere and written for Word, the
//! CRC-32 checksum of their entries, and text escaped for XML. It needs no window and no macOS.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

pub mod xml;
pub mod zip;
