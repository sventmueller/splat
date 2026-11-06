//! What objects are, and those that lie in files of their own.

use std::fmt;

use crate::ReadError;
use crate::inflate::Inflater;
use crate::source::Source;

/// The SHA-1 name of an object.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Oid(pub(crate) [u8; 20]);

impl fmt::Display for Oid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}

/// The largest blob that is read: the 32 MiB of text that the app compares at most (`MOST_BYTES` in
/// `src/editor/changes.rs`), written as UTF-16 behind a byte order mark.
pub(crate) const MAX_OBJECT: u64 = 2 * (32 << 20) + 2;
/// The largest tree, commit or tag that is read; a tree this large already lists about a million and a half files.
pub(crate) const MAX_METADATA: u64 = 64 << 20;

/// What an object holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A commit, which names a tree.
    Commit,
    /// The entries of a folder.
    Tree,
    /// The contents of a file.
    Blob,
    /// An annotated tag, which names another object.
    Tag,
}

impl Kind {
    /// The largest object of this kind.
    pub fn limit(self) -> u64 {
        if self == Kind::Blob { MAX_OBJECT } else { MAX_METADATA }
    }

    /// `size` as a size in memory, if an object of this kind may have it.
    pub(crate) fn fit(self, size: u64) -> Result<usize, ReadError> {
        if size > self.limit() {
            return Err(ReadError::TooLarge);
        }
        usize::try_from(size).map_err(|_| ReadError::TooLarge)
    }
}

/// An object with its contents.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Object {
    /// Whether it is a commit, a tree, a blob or a tag.
    pub kind: Kind,
    /// Its contents, without any header.
    pub data: Vec<u8>,
}

/// The value of a hexadecimal object name.
pub(crate) fn parse_hex(text: &[u8]) -> Option<Oid> {
    if text.len() != 40 {
        return None;
    }
    let digit = |byte: u8| char::from(byte).to_digit(16).and_then(|value| u8::try_from(value).ok());
    let mut oid = [0; 20];
    for (out, [high, low]) in oid.iter_mut().zip(text.as_chunks::<2>().0) {
        *out = digit(*high)? << 4 | digit(*low)?;
    }
    Some(Oid(oid))
}

/// A loose object is a single zlib stream of `<kind> <size>\0` and the contents; the longest such header takes 28 bytes.
pub fn read_loose(source: &dyn Source) -> Result<Object, ReadError> {
    let mut inflater = Inflater::new(source, 0);
    let mut head = [0; 32];
    let got = inflater.read(&mut head)?;
    let end = head[..got]
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(ReadError::Damaged)?;
    let (kind, size) = parse_loose_header(&head[..end])?;
    let size = kind.fit(size)?;
    let first = &head[end + 1..got];
    if first.len() > size {
        return Err(ReadError::Damaged);
    }
    let data = inflater.finish(first.to_vec(), size)?;
    Ok(Object { kind, data })
}

fn parse_loose_header(head: &[u8]) -> Result<(Kind, u64), ReadError> {
    let space = head.iter().position(|&byte| byte == b' ').ok_or(ReadError::Damaged)?;
    let kind = match &head[..space] {
        b"commit" => Kind::Commit,
        b"tree" => Kind::Tree,
        b"blob" => Kind::Blob,
        b"tag" => Kind::Tag,
        _ => return Err(ReadError::Damaged),
    };
    let digits = &head[space + 1..];
    if digits.is_empty() {
        return Err(ReadError::Damaged);
    }
    let mut size = 0u64;
    for &byte in digits {
        if !byte.is_ascii_digit() {
            return Err(ReadError::Damaged);
        }
        size = size
            .checked_mul(10)
            .and_then(|size| size.checked_add(u64::from(byte - b'0')))
            .ok_or(ReadError::TooLarge)?;
    }
    Ok((kind, size))
}

/// The object a commit or tag names in the line that opens it.
fn first_line_names(data: &[u8], field: &[u8]) -> Result<Oid, ReadError> {
    let rest = data.strip_prefix(field).ok_or(ReadError::Damaged)?;
    let line = rest
        .get(..41)
        .filter(|line| line[40] == b'\n')
        .ok_or(ReadError::Damaged)?;
    parse_hex(&line[..40]).ok_or(ReadError::Damaged)
}

/// The tree of a commit.
pub fn commit_tree(commit: &[u8]) -> Result<Oid, ReadError> {
    first_line_names(commit, b"tree ")
}

/// The object an annotated tag stands for.
pub fn tag_target(tag: &[u8]) -> Result<Oid, ReadError> {
    first_line_names(tag, b"object ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::zlib;

    fn loose(head: &str, body: &[u8]) -> Vec<u8> {
        let mut raw = head.as_bytes().to_vec();
        raw.push(0);
        raw.extend_from_slice(body);
        zlib(&raw)
    }

    #[test]
    fn every_kind_of_loose_object_reads() {
        for (name, kind) in [
            ("commit", Kind::Commit),
            ("tree", Kind::Tree),
            ("blob", Kind::Blob),
            ("tag", Kind::Tag),
        ] {
            let object = read_loose(&loose(&format!("{name} 3"), b"abc")).unwrap();
            assert_eq!(
                object,
                Object {
                    kind,
                    data: b"abc".to_vec()
                }
            );
        }
    }

    #[test]
    fn a_body_longer_than_the_first_read_still_reads_whole() {
        let body = b"x".repeat(100_000);
        assert_eq!(read_loose(&loose("blob 100000", &body)).unwrap().data, body);
    }

    #[test]
    fn a_header_that_makes_no_sense_is_damaged() {
        for head in ["blob", "blob ", "box 3", "blob 3x", "blob -3"] {
            assert_eq!(read_loose(&loose(head, b"abc")), Err(ReadError::Damaged), "{head}");
        }
    }

    #[test]
    fn a_stream_without_the_end_of_a_header_is_damaged() {
        assert_eq!(read_loose(&zlib(b"blob 3")), Err(ReadError::Damaged));
        assert_eq!(read_loose(&zlib(&[b'1'; 64])), Err(ReadError::Damaged));
    }

    #[test]
    fn a_body_of_another_size_than_declared_is_damaged() {
        assert_eq!(read_loose(&loose("blob 2", b"abc")), Err(ReadError::Damaged));
        assert_eq!(read_loose(&loose("blob 4", b"abc")), Err(ReadError::Damaged));
        assert_eq!(read_loose(&loose("blob 0", b"abc")), Err(ReadError::Damaged));
    }

    #[test]
    fn a_declared_size_above_the_limit_is_too_large_before_anything_inflates() {
        assert_eq!(
            read_loose(&loose(&format!("blob {}", MAX_OBJECT + 1), b"")),
            Err(ReadError::TooLarge)
        );
        assert_eq!(
            read_loose(&loose(&format!("tree {}", MAX_METADATA + 1), b"")),
            Err(ReadError::TooLarge)
        );
        assert_eq!(
            read_loose(&loose("blob 99999999999999999999", b"")),
            Err(ReadError::TooLarge)
        );
    }

    #[test]
    fn a_blob_may_be_larger_than_a_tree() {
        assert_eq!(
            Kind::Blob.fit(MAX_METADATA + 1),
            Ok(usize::try_from(MAX_METADATA + 1).unwrap())
        );
        assert_eq!(Kind::Tree.fit(MAX_METADATA + 1), Err(ReadError::TooLarge));
        assert_eq!(Kind::Commit.limit(), Kind::Tag.limit());
    }

    #[test]
    fn hexadecimal_names_read_in_both_cases_and_nothing_else() {
        let lower = parse_hex(&b"0123456789abcdef0123456789abcdef01234567"[..]).unwrap();
        assert_eq!(parse_hex(&b"0123456789ABCDEF0123456789ABCDEF01234567"[..]), Some(lower));
        assert_eq!(lower.to_string(), "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(parse_hex(&b"0123"[..]), None);
        assert_eq!(parse_hex(&[b'g'; 40]), None);
    }

    #[test]
    fn a_commit_names_its_tree_in_its_first_line() {
        let named = "0123456789abcdef0123456789abcdef01234567";
        let commit = format!("tree {named}\nauthor someone\n");
        assert_eq!(
            commit_tree(commit.as_bytes()).map(|oid| oid.to_string()).as_deref(),
            Ok(named)
        );
        let tag = format!("object {named}\ntype commit\n");
        assert_eq!(
            tag_target(tag.as_bytes()).map(|oid| oid.to_string()).as_deref(),
            Ok(named)
        );
    }

    #[test]
    fn a_commit_without_a_proper_first_line_is_damaged() {
        let named = "0123456789abcdef0123456789abcdef01234567";
        for commit in [
            String::new(),
            format!("author x\ntree {named}\n"),
            format!("tree {named}"),
            format!("tree {named} \n"),
            format!("tree {}\n", "z".repeat(40)),
            "tree 0123\n".to_string(),
        ] {
            assert_eq!(commit_tree(commit.as_bytes()), Err(ReadError::Damaged), "{commit}");
        }
    }
}
