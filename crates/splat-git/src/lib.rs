//! What a document holds in the last Git commit, and where a text differs from it.
//!
//! Splat treats every folder as untrusted, and running `git` in a foreign repository can start programs through its
//! configuration (`core.fsmonitor`, `core.sshCommand`, hooks, filters). So this crate reads the repository's files itself: it
//! starts no process, writes nothing and reaches no network. All its calls block; a repository that lies gives an error, and
//! no repository can make it panic or read without bound.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

mod config;
mod delta;
pub mod diff;
mod discover;
mod inflate;
mod object;
mod pack;
mod refs;
mod source;
mod store;
#[cfg(test)]
mod tests;
mod tree;

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub use object::Oid;
use object::{Kind, commit_tree, tag_target};
use source::{read_small, volume_error};
use store::Store;

const MAX_PATH_COMPONENTS: usize = 256;
/// Far more tags on tags than anyone has reason to make.
const MAX_TAG_DEPTH: usize = 8;
const MAX_CONFIG: u64 = 16 << 20;

/// Why something could not be read from a repository.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadError {
    /// The repository is in a form this crate cannot read, such as the sha256 object format or an extension it cannot honour.
    Unsupported,
    /// What the repository holds makes no sense, or an object it refers to is missing.
    Damaged,
    /// What the repository holds goes beyond a limit that protects this process.
    TooLarge,
    /// The volume reported an error of this kind.
    Io(io::ErrorKind),
}

impl fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Unsupported => formatter.write_str("the repository is in a form that cannot be read"),
            ReadError::Damaged => formatter.write_str("the repository is damaged or misses an object"),
            ReadError::TooLarge => formatter.write_str("the repository goes beyond a limit of the reader"),
            ReadError::Io(kind) => write!(formatter, "the volume reported an error: {kind}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// A repository found on the disk.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Repository {
    /// The folder the repository's files lie in, canonicalized.
    work_tree: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

/// The repository a file lies in, by walking up from it. The file itself need not exist, and neither `$GIT_DIR` nor any other
/// environment variable counts. A repository whose work tree holds the user's home folder is returned as well; the caller
/// decides what to make of it.
pub fn find(file: &Path) -> Option<Repository> {
    discover::find(file)
}

impl Repository {
    /// What the file holds in the last commit; `None` if the file is not in it or there is no commit yet. These are the bytes
    /// of the commit: no line ending conversion or other filter of the repository is applied.
    pub fn head_version(&self, file: &Path) -> Result<Option<Vec<u8>>, ReadError> {
        self.check_format()?;
        let file = discover::canonical(file).map_err(volume_error)?;
        let Ok(inside) = file.strip_prefix(&self.work_tree) else {
            return Ok(None);
        };
        let names: Vec<&[u8]> = inside.iter().map(|name| name.as_encoded_bytes()).collect();
        if names.len() > MAX_PATH_COMPONENTS {
            return Err(ReadError::TooLarge);
        }
        let Some(head) = refs::head(&self.git_dir, &self.common_dir)? else {
            return Ok(None);
        };
        let store = self.store()?;
        let (_, commit) = commit_of(&store, head)?;
        version_at(&store, commit_tree(&commit)?, &names)
    }

    /// The commit `HEAD` stands on, which tells whether a version read earlier is still current; `None` if there is no commit
    /// yet.
    pub fn head_commit(&self) -> Result<Option<Oid>, ReadError> {
        self.check_format()?;
        let Some(head) = refs::head(&self.git_dir, &self.common_dir)? else {
            return Ok(None);
        };
        Ok(Some(commit_of(&self.store()?, head)?.0))
    }

    fn check_format(&self) -> Result<(), ReadError> {
        match read_small(&self.common_dir.join("config"), MAX_CONFIG)? {
            Some(config) => config::check_format(&config),
            None => Ok(()),
        }
    }

    fn store(&self) -> Result<Store, ReadError> {
        Store::open(&self.common_dir.join("objects"))
    }
}

/// The commit an object stands for, with its contents, through any annotated tags.
fn commit_of(store: &Store, mut oid: Oid) -> Result<(Oid, Vec<u8>), ReadError> {
    for _ in 0..MAX_TAG_DEPTH {
        let object = store.read_any(&oid)?;
        match object.kind {
            Kind::Commit => return Ok((oid, object.data)),
            Kind::Tag => oid = tag_target(&object.data)?,
            Kind::Tree | Kind::Blob => return Err(ReadError::Damaged),
        }
    }
    Err(ReadError::Damaged)
}

/// What lies in a tree under a path. A commit can record a file where the disk has a folder and the other way round, so a type
/// that does not fit means that the path is not in the commit.
fn version_at(store: &Store, root: Oid, names: &[&[u8]]) -> Result<Option<Vec<u8>>, ReadError> {
    let Some((last, folders)) = names.split_last() else {
        return Ok(None);
    };
    let mut folder = root;
    for name in folders {
        match tree::lookup(&store.read(&folder, Kind::Tree)?, name)? {
            Some(entry) if entry.is_tree() => folder = entry.oid,
            _ => return Ok(None),
        }
    }
    match tree::lookup(&store.read(&folder, Kind::Tree)?, last)? {
        Some(entry) if !entry.is_tree() && !entry.is_gitlink() => store.read(&entry.oid, Kind::Blob).map(Some),
        _ => Ok(None),
    }
}

/// The parsers on raw bytes, for the fuzz target; nothing else has a reason to reach them.
#[doc(hidden)]
pub mod raw {
    pub use crate::config::check_format;
    pub use crate::delta::apply_delta;
    pub use crate::object::{Kind, Object, commit_tree, read_loose, tag_target};
    pub use crate::pack::Idx;
    pub use crate::refs::{RefFile, packed_ref, parse_ref_file, valid_ref_name};
    pub use crate::source::Source;
    pub use crate::store::read_packed;
    pub use crate::tree::{Entries, Entry as TreeEntry, lookup};
}
