//! Finding and reading objects: loose files, packs and the stores that `objects/info/alternates` names.

use std::cell::{Cell, OnceCell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::ReadError;
use crate::delta::apply_delta;
use crate::object::{Kind, MAX_METADATA, MAX_OBJECT, Object, Oid, read_loose};
use crate::pack::{Entry, Idx, Pack, Stored};
use crate::source::{OnDisk, Source, is_missing, read_exact_at, read_small, volume_error};

/// The longest delta chain that is followed. Together with offsets that only lead backwards it makes a cycle of deltas end.
const MAX_CHAIN: usize = 128;
/// The deepest delta chain Git makes unless `pack.depth` says otherwise.
const GIT_PACK_DEPTH: u64 = 50;
/// As deep as Git itself follows alternates of alternates.
const MAX_ALTERNATE_DEPTH: usize = 5;
const MAX_STORES: usize = 64;
const MAX_ALTERNATES_FILE: u64 = 1 << 20;
const MAX_PACKS: usize = 4096;
/// How many bytes one call may inflate and copy: the largest blob at the end of a chain as deep as Git makes by default,
/// which copies a whole version for each link, with room for its deltas and for the commit and the trees on the way.
const MAX_BYTES_PER_CALL: u64 = (GIT_PACK_DEPTH + 1) * MAX_OBJECT + MAX_METADATA;
/// An index up to this size is read whole, so that keeping it keeps no file open.
const SMALL_INDEX: u64 = 256 << 10;
/// How many bytes of indexes read whole one store keeps.
const MAX_INDEX_BYTES: u64 = 16 << 20;
/// How many larger indexes one store keeps open: an app on macOS starts with a limit of 256 open files, which the files
/// another thread saves count against as well.
const MAX_OPEN_INDEXES: usize = 16;

/// Where an object lies.
pub(crate) enum Located {
    Loose(Box<dyn Source>),
    Packed(Rc<Pack>, u64),
}

/// What knows where objects lie.
pub(crate) trait Locate {
    fn locate(&self, oid: &Oid) -> Result<Located, ReadError>;
}

/// What is left of `MAX_BYTES_PER_CALL`.
pub(crate) struct Budget(Cell<u64>);

impl Budget {
    fn spend(&self, bytes: usize) -> Result<(), ReadError> {
        let left = self.0.get().checked_sub(bytes as u64).ok_or(ReadError::TooLarge)?;
        self.0.set(left);
        Ok(())
    }
}

/// An entry of a delta chain, whose data is inflated only when its base is at hand: 128 deltas of the largest size at once
/// would not fit into memory.
struct Link {
    pack: Rc<Pack>,
    entry: Entry,
}

/// An object with all its deltas applied, whatever store its bases lie in.
pub(crate) fn read_object(locator: &dyn Locate, budget: &Budget, oid: &Oid) -> Result<Object, ReadError> {
    let mut chain: Vec<Link> = Vec::new();
    let mut place = locator.locate(oid)?;
    let (kind, mut data) = loop {
        let (pack, offset) = match place {
            Located::Loose(source) => {
                let object = read_loose(&*source)?;
                break (object.kind, object.data);
            }
            Located::Packed(pack, offset) => (pack, offset),
        };
        let entry = pack.entry(offset)?;
        place = match entry.stored {
            Stored::Whole(kind) => break (kind, pack.inflate(&entry, kind.limit())?),
            Stored::DeltaOnOffset(base) => Located::Packed(Rc::clone(&pack), base),
            Stored::DeltaOnName(base) => locator.locate(&base)?,
        };
        if chain.len() == MAX_CHAIN {
            return Err(ReadError::TooLarge);
        }
        chain.push(Link { pack, entry });
    };
    budget.spend(data.len())?;
    for link in chain.iter().rev() {
        let delta = link.pack.inflate(&link.entry, MAX_OBJECT)?;
        budget.spend(delta.len())?;
        data = apply_delta(&data, &delta, kind.limit())?;
        budget.spend(data.len())?;
    }
    Ok(Object { kind, data })
}

/// The index of a pack, parsed once for a store as long as there is room to keep it.
struct KnownIndex {
    path: PathBuf,
    kept: OnceCell<Result<Option<Rc<Idx>>, ReadError>>,
}

/// One folder of objects and the indexes of its packs.
struct ObjectFolder {
    path: PathBuf,
    indexes: Vec<KnownIndex>,
}

/// The objects of a repository for the length of one call, since any Git run may replace its packs in between calls.
pub(crate) struct Store {
    folders: Vec<ObjectFolder>,
    /// Objects of one commit tend to lie in one pack.
    last_pack: RefCell<Option<(Rc<Idx>, Rc<Pack>)>>,
    budget: Budget,
    index_bytes: Cell<u64>,
    open_indexes: Cell<usize>,
}

impl Store {
    pub(crate) fn open(objects: &Path) -> Result<Store, ReadError> {
        let mut found = vec![(objects.to_path_buf(), 0)];
        let mut at = 0;
        while let Some((folder, depth)) = found.get(at).cloned() {
            at += 1;
            if depth > MAX_ALTERNATE_DEPTH {
                continue;
            }
            for alternate in alternates(&folder)? {
                if !found.iter().any(|(seen, _)| *seen == alternate) {
                    if found.len() == MAX_STORES {
                        return Err(ReadError::TooLarge);
                    }
                    found.push((alternate, depth + 1));
                }
            }
        }
        let mut folders = Vec::with_capacity(found.len());
        for (path, _) in found {
            let indexes = indexes_in(&path)?
                .into_iter()
                .map(|path| KnownIndex {
                    path,
                    kept: OnceCell::new(),
                })
                .collect();
            folders.push(ObjectFolder { path, indexes });
        }
        Ok(Store {
            folders,
            last_pack: RefCell::new(None),
            budget: Budget(Cell::new(MAX_BYTES_PER_CALL)),
            index_bytes: Cell::new(0),
            open_indexes: Cell::new(0),
        })
    }

    /// The contents of an object, which has to be of that kind.
    pub(crate) fn read(&self, oid: &Oid, kind: Kind) -> Result<Vec<u8>, ReadError> {
        let object = self.read_any(oid)?;
        if object.kind == kind {
            Ok(object.data)
        } else {
            Err(ReadError::Damaged)
        }
    }

    pub(crate) fn read_any(&self, oid: &Oid) -> Result<Object, ReadError> {
        read_object(self, &self.budget, oid)
    }

    /// The parsed index, or `None` if its file is gone.
    fn index(&self, known: &KnownIndex) -> Result<Option<Rc<Idx>>, ReadError> {
        if let Some(kept) = known.kept.get() {
            return kept.clone();
        }
        let (index, holds_file) = self.load_index(&known.path);
        if !holds_file || self.open_indexes.get() < MAX_OPEN_INDEXES {
            self.open_indexes.set(self.open_indexes.get() + usize::from(holds_file));
            let _ = known.kept.set(index.clone());
        }
        index
    }

    /// An index read whole if it is small and there is room for it, or else searched in its file, which it then holds open.
    fn load_index(&self, path: &Path) -> (Result<Option<Rc<Idx>>, ReadError>, bool) {
        let file = match OnDisk::open(path) {
            Ok(Some(file)) => file,
            other => return (other.map(|_| None), false),
        };
        let size = file.size();
        if size <= SMALL_INDEX && self.index_bytes.get() + size <= MAX_INDEX_BYTES {
            self.index_bytes.set(self.index_bytes.get() + size);
            let whole = read_whole(&file).and_then(|bytes| Idx::parse(Box::new(bytes)));
            return (whole.map(|idx| Some(Rc::new(idx))), false);
        }
        let parsed = Idx::parse(Box::new(file));
        let holds_file = parsed.is_ok();
        (parsed.map(|idx| Some(Rc::new(idx))), holds_file)
    }

    fn in_pack(&self, index: &KnownIndex, oid: &Oid) -> Result<Option<Located>, ReadError> {
        let Some(idx) = self.index(index)? else {
            return Ok(None);
        };
        let Some(offset) = idx.find(oid)? else {
            return Ok(None);
        };
        let Some(file) = OnDisk::open(&index.path.with_extension("pack"))? else {
            return Ok(None);
        };
        let pack = Rc::new(Pack::parse(Box::new(file), &idx)?);
        *self.last_pack.borrow_mut() = Some((idx, Rc::clone(&pack)));
        Ok(Some(Located::Packed(pack, offset)))
    }
}

impl Locate for Store {
    /// A pack that cannot be read need not be the one that holds the object, so it only counts if no other holds it.
    fn locate(&self, oid: &Oid) -> Result<Located, ReadError> {
        if let Some((idx, pack)) = &*self.last_pack.borrow()
            && let Some(offset) = idx.find(oid)?
        {
            return Ok(Located::Packed(Rc::clone(pack), offset));
        }
        let name = oid.to_string();
        let (folder_name, file_name) = name.split_at(2);
        let mut failure = ReadError::Damaged;
        for folder in &self.folders {
            if let Some(file) = OnDisk::open(&folder.path.join(folder_name).join(file_name))? {
                return Ok(Located::Loose(Box::new(file)));
            }
            for index in &folder.indexes {
                match self.in_pack(index, oid) {
                    Ok(Some(located)) => return Ok(located),
                    Ok(None) => {}
                    Err(error) => failure = error,
                }
            }
        }
        Err(failure)
    }
}

fn read_whole(file: &OnDisk) -> Result<Vec<u8>, ReadError> {
    let mut bytes = vec![0; usize::try_from(file.size()).map_err(|_| ReadError::TooLarge)?];
    read_exact_at(file, &mut bytes, 0)?;
    Ok(bytes)
}

/// The folders an object folder names, as absolute paths; a relative one starts at that object folder. Lines in Git's C-quoted
/// form are skipped.
fn alternates(objects: &Path) -> Result<Vec<PathBuf>, ReadError> {
    let Some(list) = read_small(&objects.join("info/alternates"), MAX_ALTERNATES_FILE)? else {
        return Ok(Vec::new());
    };
    Ok(list
        .split(|&byte| byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .filter(|line| !line.is_empty() && !line.starts_with(['#', '"']))
        .take(MAX_STORES)
        .filter_map(|line| objects.join(line).canonicalize().ok())
        .collect())
}

fn indexes_in(objects: &Path) -> Result<Vec<PathBuf>, ReadError> {
    let entries = match fs::read_dir(objects.join("pack")) {
        Ok(entries) => entries,
        Err(error) if is_missing(&error) => return Ok(Vec::new()),
        Err(error) => return Err(volume_error(error)),
    };
    let mut indexes = Vec::new();
    for entry in entries {
        let path = entry.map_err(volume_error)?.path();
        if path.extension().is_some_and(|extension| extension == "idx") {
            if indexes.len() == MAX_PACKS {
                return Err(ReadError::TooLarge);
            }
            indexes.push(path);
        }
    }
    indexes.sort();
    Ok(indexes)
}

/// One pack and its index in memory, for what checks them on raw bytes.
struct InMemory {
    idx: Idx,
    pack: Rc<Pack>,
}

impl Locate for InMemory {
    fn locate(&self, oid: &Oid) -> Result<Located, ReadError> {
        let offset = self.idx.find(oid)?.ok_or(ReadError::Damaged)?;
        Ok(Located::Packed(Rc::clone(&self.pack), offset))
    }
}

/// Each object an index and its pack hold, up to `most` of them, within `bytes` of inflating and copying together.
pub fn read_packed(
    idx: Vec<u8>,
    pack: Vec<u8>,
    most: u32,
    bytes: u64,
) -> Result<Vec<Result<Object, ReadError>>, ReadError> {
    let idx = Idx::parse(Box::new(idx))?;
    let pack = Rc::new(Pack::parse(Box::new(pack), &idx)?);
    let budget = Budget(Cell::new(bytes));
    let names = (0..idx.objects().min(most))
        .map(|index| idx.name_at(index))
        .collect::<Result<Vec<_>, _>>()?;
    let memory = InMemory { idx, pack };
    Ok(names.iter().map(|oid| read_object(&memory, &budget, oid)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{HandMade, Packer, Temp, blob_entry_head, copy, delta, insert, zlib};

    const PLENTY: u64 = 1 << 30;

    fn all(packer: Packer) -> Vec<Result<Object, ReadError>> {
        let (pack, idx) = packer.finish();
        read_packed(idx, pack, u32::MAX, PLENTY).unwrap()
    }

    fn blob(text: &[u8]) -> Result<Object, ReadError> {
        Ok(Object {
            kind: Kind::Blob,
            data: text.to_vec(),
        })
    }

    #[test]
    fn a_delta_on_an_offset_is_applied_to_its_base() {
        let mut packer = Packer::new();
        let base = packer.whole(Oid([1; 20]), 3, b"hello world");
        packer.ofs_delta(
            Oid([2; 20]),
            base,
            &delta(11, 9, &[copy(6, 5), insert(b"-"), copy(0, 3)]),
        );
        assert_eq!(all(packer), vec![blob(b"hello world"), blob(b"world-hel")]);
    }

    #[test]
    fn a_delta_on_a_name_is_applied_to_its_base() {
        let mut packer = Packer::new();
        packer.whole(Oid([1; 20]), 3, b"hello");
        packer.ref_delta(Oid([2; 20]), Oid([1; 20]), &delta(5, 4, &[copy(1, 4)]));
        assert_eq!(all(packer), vec![blob(b"hello"), blob(b"ello")]);
    }

    /// A chain of `length` deltas on one base, each adding one letter; the deepest has the name that sorts last.
    fn chain(length: u32) -> Packer {
        let mut packer = Packer::new();
        let mut offset = packer.whole(Oid([1; 20]), 3, b"x");
        for step in 0..length {
            let size = u64::from(step) + 1;
            let made = delta(size, size + 1, &[copy(0, size), insert(b"y")]);
            let mut name = Oid([2; 20]);
            name.0[0] = if step + 1 == length { 9 } else { 2 };
            name.0[1..5].copy_from_slice(&step.to_be_bytes());
            offset = packer.ofs_delta(name, offset, &made);
        }
        packer
    }

    #[test]
    fn a_chain_as_long_as_the_cap_reads() {
        let results = all(chain(u32::try_from(MAX_CHAIN).unwrap()));
        let last = results.last().unwrap().as_ref().unwrap();
        assert_eq!(last.data.len(), MAX_CHAIN + 1);
        assert!(results.iter().all(Result::is_ok));
    }

    #[test]
    fn a_chain_deeper_than_the_cap_is_too_large() {
        let results = all(chain(u32::try_from(MAX_CHAIN).unwrap() + 1));
        assert_eq!(*results.last().unwrap(), Err(ReadError::TooLarge));
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    }

    #[test]
    fn deltas_that_name_each_other_end_at_the_cap() {
        let mut packer = Packer::new();
        packer.ref_delta(Oid([1; 20]), Oid([2; 20]), &delta(1, 1, &[insert(b"a")]));
        packer.ref_delta(Oid([2; 20]), Oid([1; 20]), &delta(1, 1, &[insert(b"b")]));
        assert_eq!(all(packer), vec![Err(ReadError::TooLarge), Err(ReadError::TooLarge)]);
    }

    #[test]
    fn a_delta_on_itself_ends_at_the_cap() {
        let mut packer = Packer::new();
        packer.ref_delta(Oid([1; 20]), Oid([1; 20]), &delta(1, 1, &[insert(b"a")]));
        assert_eq!(all(packer), vec![Err(ReadError::TooLarge)]);
    }

    #[test]
    fn a_delta_that_reads_past_its_base_is_damaged() {
        let mut packer = Packer::new();
        let base = packer.whole(Oid([1; 20]), 3, b"abc");
        packer.ofs_delta(Oid([2; 20]), base, &delta(3, 4, &[copy(0, 4)]));
        assert_eq!(all(packer)[1], Err(ReadError::Damaged));
    }

    #[test]
    fn a_delta_on_a_base_that_is_missing_is_damaged() {
        let mut packer = Packer::new();
        packer.ref_delta(Oid([1; 20]), Oid([7; 20]), &delta(1, 1, &[insert(b"a")]));
        assert_eq!(all(packer), vec![Err(ReadError::Damaged)]);
    }

    #[test]
    fn a_delta_result_above_the_limit_of_its_kind_is_too_large() {
        let mut packer = Packer::new();
        let base = packer.whole(Oid([1; 20]), 2, b"");
        packer.ofs_delta(Oid([2; 20]), base, &delta(0, crate::object::MAX_METADATA + 1, &[]));
        assert_eq!(all(packer)[1], Err(ReadError::TooLarge));
    }

    #[test]
    fn a_delta_of_a_declared_size_above_the_limit_is_too_large() {
        let mut packer = Packer::new();
        let base = packer.whole(Oid([1; 20]), 3, b"a");
        packer.ofs_delta_declared(Oid([2; 20]), base, MAX_OBJECT + 1, b"tiny");
        assert_eq!(all(packer)[1], Err(ReadError::TooLarge));
    }

    #[test]
    fn what_a_chain_inflates_and_copies_is_bounded_altogether() {
        let (pack, idx) = chain(3).finish();
        let nothing = read_packed(idx.clone(), pack.clone(), 1, 0).unwrap();
        assert_eq!(nothing, [Err(ReadError::TooLarge)]);
        let one_byte_is_the_base_alone = read_packed(idx.clone(), pack.clone(), 2, 1).unwrap();
        assert_eq!(one_byte_is_the_base_alone, [blob(b"x"), Err(ReadError::TooLarge)]);
        let a_delta_but_not_its_result = read_packed(idx, pack, 2, 1 + 1 + 6).unwrap();
        assert_eq!(a_delta_but_not_its_result, [blob(b"x"), Err(ReadError::TooLarge)]);
    }

    /// A pack entry of a blob of `size` bytes, stored without compression so that a large one is quick to make.
    fn stored_blob(size: u64) -> Vec<u8> {
        let mut entry = blob_entry_head(size);
        let data = vec![b'x'; usize::try_from(size).unwrap()];
        entry.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(&data, 0));
        entry
    }

    #[test]
    fn a_blob_of_the_largest_size_reads_and_one_a_byte_larger_is_too_large() {
        let mut packer = Packer::new();
        packer.raw(Oid([1; 20]), stored_blob(MAX_OBJECT));
        packer.raw(Oid([2; 20]), stored_blob(MAX_OBJECT + 1));
        let results = all(packer);
        assert_eq!(
            results[0].as_ref().map(|object| object.data.len() as u64),
            Ok(MAX_OBJECT)
        );
        assert_eq!(results[1], Err(ReadError::TooLarge));
    }

    /// A blob of the largest size, whole, under a chain of `links` deltas that each write their number into its first
    /// byte; the deepest is named `[9; 20]`.
    fn largest_under(links: u8) -> InMemory {
        let mut packer = Packer::new();
        let mut at = packer.raw(Oid([1; 20]), stored_blob(MAX_OBJECT));
        for link in 1..=links {
            let mut instructions = vec![insert(&[link])];
            let mut copied = 1;
            while copied < MAX_OBJECT {
                let piece = (MAX_OBJECT - copied).min(0xff_ffff);
                instructions.push(copy(copied, piece));
                copied += piece;
            }
            let mut name = Oid([2; 20]);
            name.0[1] = link;
            let name = if link == links { Oid([9; 20]) } else { name };
            at = packer.ofs_delta(name, at, &delta(MAX_OBJECT, MAX_OBJECT, &instructions));
        }
        let (pack, idx) = packer.finish();
        let idx = Idx::parse(Box::new(idx)).unwrap();
        let pack = Rc::new(Pack::parse(Box::new(pack), &idx).unwrap());
        InMemory { idx, pack }
    }

    #[test]
    fn the_largest_blob_at_the_end_of_a_chain_as_deep_as_git_makes_one_reads_in_one_call() {
        let depth = u8::try_from(GIT_PACK_DEPTH).unwrap();
        let object = read_object(
            &largest_under(depth),
            &Budget(Cell::new(MAX_BYTES_PER_CALL)),
            &Oid([9; 20]),
        )
        .unwrap();
        assert_eq!(object.data.len() as u64, MAX_OBJECT);
        assert_eq!(object.data[..2], [depth, b'x']);
    }

    #[test]
    fn the_largest_blob_one_link_deeper_is_too_large_for_one_call() {
        let depth = u8::try_from(GIT_PACK_DEPTH).unwrap() + 1;
        let read = read_object(
            &largest_under(depth),
            &Budget(Cell::new(MAX_BYTES_PER_CALL)),
            &Oid([9; 20]),
        );
        assert_eq!(read.err(), Some(ReadError::TooLarge));
    }

    #[test]
    fn an_index_or_pack_that_makes_no_sense_gives_no_objects() {
        let (pack, idx) = chain(1).finish();
        assert_eq!(
            read_packed(vec![0; 9], pack.clone(), 1, 1).err(),
            Some(ReadError::Damaged)
        );
        assert_eq!(read_packed(idx, vec![0; 9], 1, 1).err(), Some(ReadError::Damaged));
    }

    #[test]
    fn loose_objects_and_packs_are_found_on_disk() {
        let repo = HandMade::new();
        repo.loose(&Oid([1; 20]), "blob", b"loose");
        let mut packer = Packer::new();
        packer.whole(Oid([2; 20]), 3, b"packed");
        repo.pack("one", packer);
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"loose"[..]));
        assert_eq!(store.read(&Oid([2; 20]), Kind::Blob).as_deref(), Ok(&b"packed"[..]));
        assert_eq!(store.read(&Oid([2; 20]), Kind::Tree), Err(ReadError::Damaged));
        assert_eq!(store.read(&Oid([3; 20]), Kind::Blob), Err(ReadError::Damaged));
    }

    #[test]
    fn the_pack_that_held_the_last_object_is_asked_first() {
        let repo = HandMade::new();
        let mut packer = Packer::new();
        packer.whole(Oid([1; 20]), 3, b"a");
        packer.whole(Oid([2; 20]), 3, b"b");
        repo.pack("one", packer);
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"a"[..]));
        fs::remove_file(repo.objects().join("pack/one.idx")).unwrap();
        assert_eq!(store.read(&Oid([2; 20]), Kind::Blob).as_deref(), Ok(&b"b"[..]));
    }

    #[test]
    fn an_index_is_parsed_once_for_a_store() {
        let repo = HandMade::new();
        for (name, oid, text) in [("one", 1, b"a"), ("two", 2, b"b")] {
            let mut packer = Packer::new();
            packer.whole(Oid([oid; 20]), 3, text);
            repo.pack(name, packer);
        }
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([3; 20]), Kind::Blob), Err(ReadError::Damaged));
        fs::remove_file(repo.objects().join("pack/one.idx")).unwrap();
        fs::remove_file(repo.objects().join("pack/two.idx")).unwrap();
        assert_eq!(store.read(&Oid([2; 20]), Kind::Blob).as_deref(), Ok(&b"b"[..]));
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"a"[..]));
    }

    fn numbered(number: u32) -> Oid {
        let mut name = Oid([0x10; 20]);
        name.0[1..5].copy_from_slice(&number.to_be_bytes());
        name
    }

    /// A pack of `count` empty blobs, named by their number, and maybe one more.
    fn many_empty_blobs(count: u32, more: Option<Oid>) -> (Vec<u8>, Vec<u8>) {
        let mut entry = blob_entry_head(0);
        entry.extend_from_slice(&zlib(b""));
        let mut packer = Packer::new();
        for number in 0..count {
            packer.raw(numbered(number), entry.clone());
        }
        if let Some(name) = more {
            packer.raw(name, entry);
        }
        packer.finish()
    }

    #[test]
    fn indexes_beyond_the_room_in_memory_and_for_open_files_are_parsed_again() {
        let repo = HandMade::new();
        let (pack, idx) = many_empty_blobs(9000, None);
        let size = idx.len() as u64;
        assert!(size <= SMALL_INDEX);
        let kept = usize::try_from(MAX_INDEX_BYTES / size).unwrap() + MAX_OPEN_INDEXES;
        for number in 0..kept {
            repo.raw_pack_files(&format!("a{number:03}"), &idx, &pack);
        }
        let last = Oid([0xee; 20]);
        let (pack, idx) = many_empty_blobs(9000, Some(last));
        repo.raw_pack_files("b", &idx, &pack);
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([0xff; 20]), Kind::Blob), Err(ReadError::Damaged));
        for entry in fs::read_dir(repo.objects().join("pack")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "idx") {
                fs::remove_file(path).unwrap();
            }
        }
        assert_eq!(store.read(&numbered(0), Kind::Blob), Ok(Vec::new()));
        assert_eq!(store.read(&last, Kind::Blob), Err(ReadError::Damaged));
        assert_eq!(store.open_indexes.get(), MAX_OPEN_INDEXES);
    }

    #[test]
    fn a_delta_may_rest_on_a_loose_object_or_another_pack() {
        let repo = HandMade::new();
        repo.loose(&Oid([1; 20]), "blob", b"hello");
        let mut packer = Packer::new();
        packer.ref_delta(Oid([2; 20]), Oid([1; 20]), &delta(5, 4, &[copy(1, 4)]));
        repo.pack("one", packer);
        let mut packer = Packer::new();
        packer.ref_delta(Oid([3; 20]), Oid([2; 20]), &delta(4, 2, &[copy(0, 2)]));
        repo.pack("two", packer);
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([3; 20]), Kind::Blob).as_deref(), Ok(&b"el"[..]));
    }

    #[test]
    fn a_damaged_pack_only_counts_if_no_other_holds_the_object() {
        let repo = HandMade::new();
        repo.raw_pack_files("damaged", b"not an index", b"not a pack");
        let mut packer = Packer::new();
        packer.whole(Oid([1; 20]), 3, b"fine");
        repo.pack("fine", packer);
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"fine"[..]));
        assert_eq!(store.read(&Oid([2; 20]), Kind::Blob), Err(ReadError::Damaged));
    }

    #[test]
    fn an_index_of_an_unknown_version_explains_a_missing_object() {
        let repo = HandMade::new();
        let mut unknown = vec![0xff, b't', b'O', b'c', 0, 0, 0, 9];
        unknown.resize(2000, 0);
        repo.raw_pack_files("new", &unknown, b"");
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob), Err(ReadError::Unsupported));
    }

    #[test]
    fn an_index_without_its_pack_holds_nothing() {
        let repo = HandMade::new();
        let mut packer = Packer::new();
        packer.whole(Oid([1; 20]), 3, b"gone");
        repo.pack("one", packer);
        fs::remove_file(repo.objects().join("pack/one.pack")).unwrap();
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob), Err(ReadError::Damaged));
    }

    #[test]
    fn a_vanished_index_holds_nothing() {
        let repo = HandMade::new();
        let mut packer = Packer::new();
        packer.whole(Oid([1; 20]), 3, b"gone");
        repo.pack("one", packer);
        let store = Store::open(&repo.objects()).unwrap();
        fs::remove_file(repo.objects().join("pack/one.idx")).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob), Err(ReadError::Damaged));
    }

    #[test]
    fn other_files_in_the_pack_folder_are_no_packs() {
        let repo = HandMade::new();
        repo.write_in_objects("pack/one.rev", b"x");
        repo.write_in_objects("pack/multi-pack-index", b"x");
        repo.write_in_objects("pack/.idx", b"x");
        assert_eq!(indexes_in(&repo.objects()).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn too_many_packs_are_too_large() {
        let repo = HandMade::new();
        for number in 0..=MAX_PACKS {
            repo.write_in_objects(&format!("pack/{number}.idx"), b"");
        }
        assert_eq!(Store::open(&repo.objects()).err(), Some(ReadError::TooLarge));
    }

    #[test]
    fn an_alternate_is_read_by_a_relative_and_by_an_absolute_path() {
        let repo = HandMade::new();
        let near = repo.another_store("near", &Oid([1; 20]), b"near");
        let far = Temp::new();
        let far_objects = far.path().join("objects");
        fs::create_dir_all(&far_objects).unwrap();
        crate::tests::support::write_loose(&far_objects, &Oid([2; 20]), "blob", b"far");
        assert!(near.is_absolute());
        repo.write_in_objects(
            "info/alternates",
            format!(
                "# a comment\n\n../near/objects\n\"quoted\"\n{}\nnot there\n",
                far_objects.display()
            )
            .as_bytes(),
        );
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"near"[..]));
        assert_eq!(store.read(&Oid([2; 20]), Kind::Blob).as_deref(), Ok(&b"far"[..]));
    }

    #[test]
    fn alternates_that_name_each_other_end() {
        let repo = HandMade::new();
        let other = repo.another_store("other", &Oid([1; 20]), b"there");
        repo.write_in_objects("info/alternates", other.display().to_string().as_bytes());
        crate::tests::support::write_in(
            &other,
            "info/alternates",
            repo.objects().canonicalize().unwrap().display().to_string().as_bytes(),
        );
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"there"[..]));
        assert_eq!(store.read(&Oid([2; 20]), Kind::Blob), Err(ReadError::Damaged));
    }

    /// A chain of `length` alternates, the last of which holds `Oid([1; 20])`.
    fn alternates_of_alternates(repo: &HandMade, length: usize) {
        let mut previous = repo.objects();
        for step in 0..length {
            let next = repo.another_store(&format!("store{step}"), &Oid([9; 20]), b"unused");
            crate::tests::support::write_in(&previous, "info/alternates", next.display().to_string().as_bytes());
            previous = next;
        }
        crate::tests::support::write_loose(&previous, &Oid([1; 20]), "blob", b"deep");
    }

    #[test]
    fn alternates_are_followed_as_deep_as_git_does_and_no_deeper() {
        let reachable = HandMade::new();
        alternates_of_alternates(&reachable, MAX_ALTERNATE_DEPTH + 1);
        let store = Store::open(&reachable.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"deep"[..]));
        let beyond = HandMade::new();
        alternates_of_alternates(&beyond, MAX_ALTERNATE_DEPTH + 2);
        let store = Store::open(&beyond.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob), Err(ReadError::Damaged));
    }

    #[test]
    fn too_many_alternates_are_too_large() {
        let repo = HandMade::new();
        let mut list = String::new();
        for number in 0..MAX_STORES {
            let store = repo.another_store(&format!("store{number}"), &Oid([9; 20]), b"unused");
            list.push_str(&format!("{}\n", store.display()));
        }
        repo.write_in_objects("info/alternates", list.as_bytes());
        assert_eq!(Store::open(&repo.objects()).err(), Some(ReadError::TooLarge));
    }

    #[test]
    fn an_alternates_file_of_any_size_is_refused_above_the_limit() {
        let repo = HandMade::new();
        repo.write_in_objects(
            "info/alternates",
            &vec![b'#'; usize::try_from(MAX_ALTERNATES_FILE).unwrap() + 1],
        );
        assert_eq!(Store::open(&repo.objects()).err(), Some(ReadError::TooLarge));
    }

    #[test]
    fn lines_of_an_alternates_file_that_are_no_text_are_skipped() {
        let repo = HandMade::new();
        let other = repo.another_store("other", &Oid([1; 20]), b"there");
        let mut list = b"\xff\xfe\n".to_vec();
        list.extend_from_slice(other.display().to_string().as_bytes());
        repo.write_in_objects("info/alternates", &list);
        let store = Store::open(&repo.objects()).unwrap();
        assert_eq!(store.read(&Oid([1; 20]), Kind::Blob).as_deref(), Ok(&b"there"[..]));
    }

    #[test]
    fn a_store_without_a_pack_folder_is_a_store_without_packs() {
        let temp = Temp::new();
        assert_eq!(indexes_in(temp.path()).unwrap(), Vec::<PathBuf>::new());
    }
}
