//! Temporary folders, the real `git`, and the bytes of a repository built by hand.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use miniz_oxide::deflate::compress_to_vec_zlib;

use crate::diff::Hunk;
use crate::object::Oid;
use crate::source::Source;

pub(crate) fn zlib(data: &[u8]) -> Vec<u8> {
    compress_to_vec_zlib(data, 6)
}

/// The head of a pack entry for a blob of `size` bytes.
pub(crate) fn blob_entry_head(size: u64) -> Vec<u8> {
    entry_head(3, size)
}

/// A folder of its own for one test.
pub(crate) struct Temp(PathBuf);

impl Temp {
    pub(crate) fn new() -> Temp {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("splat-git-{}-{number}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    pub(crate) fn write(&self, name: &str, data: &[u8]) -> PathBuf {
        write_in(&self.0, name, data)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn write_in(folder: &Path, name: &str, data: &[u8]) -> PathBuf {
    let path = folder.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, data).unwrap();
    path
}

/// A file of a given size that takes no room on the disk.
pub(crate) fn sparse_file(path: &Path, size: u64) {
    fs::File::create(path).unwrap().set_len(size).unwrap();
}

/// A source of bytes that fails on demand.
pub(crate) struct Failing {
    data: Vec<u8>,
    size: u64,
    interrupts_left: AtomicUsize,
    fails: bool,
}

impl Failing {
    pub(crate) fn always() -> Failing {
        Failing::sized(0)
    }

    pub(crate) fn sized(size: u64) -> Failing {
        Failing {
            data: Vec::new(),
            size,
            interrupts_left: AtomicUsize::new(0),
            fails: true,
        }
    }

    pub(crate) fn interrupted_once(data: Vec<u8>) -> Failing {
        Failing {
            size: data.len() as u64,
            data,
            interrupts_left: AtomicUsize::new(1),
            fails: false,
        }
    }
}

impl Source for Failing {
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        if self.fails {
            return Err(io::Error::other("a volume that fails"));
        }
        if self.interrupts_left.swap(0, Ordering::Relaxed) > 0 {
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.data.read_at(buffer, offset)
    }

    fn size(&self) -> u64 {
        self.size
    }
}

/// The instruction of a delta that copies from its base.
pub(crate) fn copy(offset: u64, size: u64) -> Vec<u8> {
    let mut command = 0x80;
    let mut operands = Vec::new();
    for (bit, value, bytes) in [(0, offset, 4), (4, size, 3)] {
        for index in 0..bytes {
            let byte = value.to_le_bytes()[index];
            if byte != 0 {
                command |= 1 << (bit + index);
                operands.push(byte);
            }
        }
    }
    let mut instruction = vec![command];
    instruction.extend(operands);
    instruction
}

/// The instruction of a delta that inserts new bytes.
pub(crate) fn insert(bytes: &[u8]) -> Vec<u8> {
    let mut instruction = vec![u8::try_from(bytes.len()).unwrap()];
    instruction.extend_from_slice(bytes);
    instruction
}

fn low_seven_bits(value: u64) -> u8 {
    value.to_le_bytes()[0] & 0x7f
}

fn seven_bits(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(low_seven_bits(value) | 0x80);
        value >>= 7;
    }
    out.push(low_seven_bits(value));
}

/// The data of a delta.
pub(crate) fn delta(base: u64, result: u64, instructions: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    seven_bits(base, &mut out);
    seven_bits(result, &mut out);
    instructions
        .iter()
        .for_each(|instruction| out.extend_from_slice(instruction));
    out
}

/// The bytes of a tree.
pub(crate) fn tree_bytes(entries: &[(&str, &str, Oid)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (mode, name, oid) in entries {
        out.extend_from_slice(mode.as_bytes());
        out.push(b' ');
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(&oid.0);
    }
    out
}

fn entry_head(kind: u8, size: u64) -> Vec<u8> {
    let low = low_seven_bits(size) & 15;
    let rest = size >> 4;
    let mut head = vec![kind << 4 | low | if rest > 0 { 0x80 } else { 0 }];
    if rest > 0 {
        seven_bits(rest, &mut head);
    }
    head
}

/// A pack and its index made entry by entry, with names of free choice since nothing checks them against the contents.
pub(crate) struct Packer {
    pack: Vec<u8>,
    places: Vec<(Oid, u64)>,
}

impl Packer {
    pub(crate) fn new() -> Packer {
        let mut pack = b"PACK\0\0\0\x02".to_vec();
        pack.extend_from_slice(&[0; 4]);
        Packer {
            pack,
            places: Vec::new(),
        }
    }

    /// Lists an object at a place the bytes of the pack do not reach, for a test to write there.
    pub(crate) fn far_away(&mut self, oid: Oid, offset: u64) {
        self.places.push((oid, offset));
    }

    pub(crate) fn raw(&mut self, oid: Oid, bytes: Vec<u8>) -> u64 {
        let offset = self.pack.len() as u64;
        self.places.push((oid, offset));
        self.pack.extend_from_slice(&bytes);
        offset
    }

    pub(crate) fn whole(&mut self, oid: Oid, kind: u8, data: &[u8]) -> u64 {
        self.declared(oid, kind, data.len() as u64, data)
    }

    pub(crate) fn declared(&mut self, oid: Oid, kind: u8, size: u64, data: &[u8]) -> u64 {
        let mut bytes = entry_head(kind, size);
        bytes.extend_from_slice(&zlib(data));
        self.raw(oid, bytes)
    }

    pub(crate) fn ofs_delta(&mut self, oid: Oid, base: u64, delta: &[u8]) -> u64 {
        let distance = self.pack.len() as u64 - base;
        self.ofs_delta_at_distance(oid, distance, delta)
    }

    pub(crate) fn ofs_delta_declared(&mut self, oid: Oid, base: u64, size: u64, delta: &[u8]) -> u64 {
        let mut bytes = entry_head(6, size);
        bytes.extend_from_slice(&distance_bytes(self.pack.len() as u64 - base));
        bytes.extend_from_slice(&zlib(delta));
        self.raw(oid, bytes)
    }

    pub(crate) fn ofs_delta_at_distance(&mut self, oid: Oid, distance: u64, delta: &[u8]) -> u64 {
        let mut bytes = entry_head(6, delta.len() as u64);
        bytes.extend_from_slice(&distance_bytes(distance));
        bytes.extend_from_slice(&zlib(delta));
        self.raw(oid, bytes)
    }

    pub(crate) fn ref_delta(&mut self, oid: Oid, base: Oid, delta: &[u8]) -> u64 {
        let mut bytes = entry_head(7, delta.len() as u64);
        bytes.extend_from_slice(&base.0);
        bytes.extend_from_slice(&zlib(delta));
        self.raw(oid, bytes)
    }

    pub(crate) fn finish(self) -> (Vec<u8>, Vec<u8>) {
        self.finish_with_wide_offsets(&[])
    }

    pub(crate) fn finish_with_wide_offsets(mut self, wide: &[Oid]) -> (Vec<u8>, Vec<u8>) {
        let count = u32::try_from(self.places.len()).unwrap();
        self.pack[8..12].copy_from_slice(&count.to_be_bytes());
        let checksum = [0xcd; 20];
        self.pack.extend_from_slice(&checksum);
        self.places.sort();
        let mut idx = b"\xfftOc\0\0\0\x02".to_vec();
        for first in 0..=255u8 {
            let up_to = self.places.iter().filter(|(oid, _)| oid.0[0] <= first).count();
            idx.extend_from_slice(&u32::try_from(up_to).unwrap().to_be_bytes());
        }
        self.places.iter().for_each(|(oid, _)| idx.extend_from_slice(&oid.0));
        idx.extend_from_slice(&vec![0; self.places.len() * 4]);
        let mut table = Vec::new();
        for (oid, offset) in &self.places {
            let short = if wide.contains(oid) {
                let number = u32::try_from(table.len() / 8).unwrap();
                table.extend_from_slice(&offset.to_be_bytes());
                0x8000_0000 | number
            } else {
                u32::try_from(*offset).unwrap()
            };
            idx.extend_from_slice(&short.to_be_bytes());
        }
        idx.extend_from_slice(&table);
        idx.extend_from_slice(&checksum);
        idx.extend_from_slice(&[0xef; 20]);
        (self.pack, idx)
    }
}

/// The distance to the base of an offset delta, most significant part first, each continued byte having one taken off.
fn distance_bytes(mut distance: u64) -> Vec<u8> {
    let mut bytes = vec![low_seven_bits(distance)];
    distance >>= 7;
    while distance > 0 {
        distance -= 1;
        bytes.insert(0, low_seven_bits(distance) | 0x80);
        distance >>= 7;
    }
    bytes
}

/// A zlib stream that inflates to `prefix`, which has to be ASCII, and then to `zeros` zero bytes, in about one hundred and
/// sixtieth of that size. Its checksum is wrong, which no reader that stops in time gets to see.
pub(crate) fn zlib_bomb(prefix: &[u8], zeros: u64) -> Vec<u8> {
    struct Bits {
        out: Vec<u8>,
        used: u32,
    }
    impl Bits {
        /// Huffman codes go into the stream from their top bit on, starting at the low bit of each byte.
        fn code(&mut self, code: u32, length: u32) {
            for bit in (0..length).rev() {
                if self.used == 0 {
                    self.out.push(0);
                }
                let value = u8::from(code >> bit & 1 == 1);
                *self.out.last_mut().unwrap() |= value << self.used;
                self.used = (self.used + 1) % 8;
            }
        }
    }
    const LITERAL_BASE: u32 = 0b0011_0000;
    const LENGTH_258: u32 = 0b1100_0101;
    let mut bits = Bits {
        out: vec![0x78, 0x01],
        used: 0,
    };
    bits.code(0b1, 1);
    bits.code(0b10, 2);
    for &byte in prefix {
        bits.code(LITERAL_BASE + u32::from(byte), 8);
    }
    bits.code(LITERAL_BASE, 8);
    for _ in 0..=zeros / 258 {
        bits.code(LENGTH_258, 8);
        bits.code(0, 5);
    }
    bits.code(0, 7);
    bits.out
}

pub(crate) fn write_loose(objects: &Path, oid: &Oid, kind: &str, data: &[u8]) {
    let mut raw = format!("{kind} {}\0", data.len()).into_bytes();
    raw.extend_from_slice(data);
    write_stream(objects, oid, &zlib(&raw));
}

/// A loose object from the bytes of its file.
pub(crate) fn write_stream(objects: &Path, oid: &Oid, stream: &[u8]) {
    let name = oid.to_string();
    write_in(objects, &format!("{}/{}", &name[..2], &name[2..]), stream);
}

/// A work tree with a git directory made by hand, with nothing in it but what a test puts there.
pub(crate) struct HandMade {
    temp: Temp,
    work_tree: PathBuf,
    git_dir: PathBuf,
}

impl HandMade {
    pub(crate) fn new() -> HandMade {
        let temp = Temp::new();
        let work_tree = temp.path().join("work");
        let git_dir = work_tree.join(".git");
        fs::create_dir_all(git_dir.join("objects")).unwrap();
        fs::create_dir_all(git_dir.join("refs")).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        HandMade {
            temp,
            work_tree,
            git_dir,
        }
    }

    pub(crate) fn work_tree(&self) -> &Path {
        &self.work_tree
    }

    pub(crate) fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub(crate) fn objects(&self) -> PathBuf {
        self.git_dir.join("objects")
    }

    pub(crate) fn write(&self, name: &str, data: &[u8]) {
        write_in(&self.git_dir, name, data);
    }

    pub(crate) fn write_beside_git_dir(&self, name: &str, data: &[u8]) {
        write_in(self.temp.path(), name, data);
    }

    pub(crate) fn write_in_objects(&self, name: &str, data: &[u8]) {
        write_in(&self.objects(), name, data);
    }

    pub(crate) fn loose(&self, oid: &Oid, kind: &str, data: &[u8]) {
        write_loose(&self.objects(), oid, kind, data);
    }

    pub(crate) fn pack(&self, name: &str, packer: Packer) {
        let (pack, idx) = packer.finish();
        self.raw_pack_files(name, &idx, &pack);
    }

    pub(crate) fn raw_pack_files(&self, name: &str, idx: &[u8], pack: &[u8]) {
        self.write_in_objects(&format!("pack/{name}.idx"), idx);
        self.write_in_objects(&format!("pack/{name}.pack"), pack);
    }

    /// Another folder of objects holding one, by its canonical path.
    pub(crate) fn another_store(&self, name: &str, oid: &Oid, data: &[u8]) -> PathBuf {
        let objects = self.git_dir.join(name).join("objects");
        write_loose(&objects, oid, "blob", data);
        objects.canonicalize().unwrap()
    }
}

pub(crate) fn commit_text(tree: &Oid) -> String {
    format!(
        "tree {}\nauthor a <a@b> 0 +0000\ncommitter a <a@b> 0 +0000\n\nmessage\n",
        tree
    )
}

/// Whether the real `git` can be run; a test without it says so and passes.
pub(crate) fn git_is_here() -> bool {
    static HERE: OnceLock<bool> = OnceLock::new();
    let here = *HERE.get_or_init(|| {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    });
    if !here {
        eprintln!("git is absent: skipped");
    }
    here
}

/// The real `git`, without anything of the user's configuration.
pub(crate) fn git_command(folder: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(folder)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", folder)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("LC_ALL", "C")
        .args(["-c", "init.defaultBranch=main"])
        .args(["-c", "gc.auto=0"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["-c", "protocol.file.allow=always"]);
    command
}

pub(crate) fn git_output(folder: &Path, args: &[&str]) -> Output {
    git_command(folder).args(args).output().unwrap()
}

/// What `git` prints, which has to succeed.
pub(crate) fn git(folder: &Path, args: &[&str]) -> Vec<u8> {
    let out = git_output(folder, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// A repository that the real `git` made.
pub(crate) struct Real {
    _temp: Temp,
    root: PathBuf,
}

impl Real {
    pub(crate) fn new() -> Option<Real> {
        Real::with_init_args(&[])
    }

    pub(crate) fn with_init_args(args: &[&str]) -> Option<Real> {
        if !git_is_here() {
            return None;
        }
        let temp = Temp::new();
        let root = temp.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        let mut init = vec!["init", "-q"];
        init.extend_from_slice(args);
        git(&root, &init);
        Some(Real { _temp: temp, root })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn git(&self, args: &[&str]) -> Vec<u8> {
        git(&self.root, args)
    }

    pub(crate) fn write(&self, name: &str, data: &[u8]) -> PathBuf {
        write_in(&self.root, name, data)
    }

    /// Adds everything and commits it.
    pub(crate) fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
    }
}

/// A small random number generator, so that a failure comes back on the next run.
pub(crate) struct Random(u64);

impl Random {
    pub(crate) fn new(seed: u64) -> Random {
        Random(seed)
    }

    pub(crate) fn below(&mut self, limit: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % limit.max(1) as u64).unwrap()
    }

    /// Up to `most` lines out of `alphabet` different ones.
    pub(crate) fn lines(&mut self, most: usize, alphabet: usize) -> String {
        let count = self.below(most + 1);
        (0..count).map(|_| format!("line {}\n", self.below(alphabet))).collect()
    }

    /// `text` with a few lines removed, replaced and put in, and sometimes without its last newline.
    pub(crate) fn edited(&mut self, text: &str, alphabet: usize) -> String {
        let mut out = String::new();
        for line in text.split_inclusive('\n') {
            match self.below(10) {
                0 => {}
                1 => out.push_str(&format!("line {}\n", self.below(alphabet))),
                2 => {
                    out.push_str(line);
                    out.push_str(&format!("new {}\n", self.below(alphabet)));
                }
                _ => out.push_str(line),
            }
        }
        if self.below(5) == 0 {
            out.pop();
        }
        out
    }
}

/// What a line is compared by.
fn key(line: &str) -> (&str, bool) {
    let ended = line.ends_with('\n');
    let text = line.strip_suffix('\n').unwrap_or(line);
    (text.strip_suffix('\r').unwrap_or(text), ended)
}

/// Everything `changes` promises: hunks in order that cover whole lines, of the kind their sides say, with bytes that are those
/// of their lines, and nothing but equal lines between them, the two texts having the same number of those.
pub(crate) fn check_hunks(old: &str, new: &str, hunks: &[Hunk]) {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let starts: Vec<usize> = std::iter::once(0)
        .chain(new_lines.iter().scan(0, |at, line| {
            *at += line.len();
            Some(*at)
        }))
        .collect();
    let (mut old_at, mut new_at) = (0, 0);
    for (number, hunk) in hunks.iter().enumerate() {
        let change = &hunk.change;
        assert!(
            old_at <= hunk.old_lines.start && new_at <= change.lines.start,
            "out of order: {hunks:?}"
        );
        let between = hunk.old_lines.start - old_at;
        assert_eq!(between, change.lines.start - new_at, "{old:?} -> {new:?}: {hunks:?}");
        assert!(number == 0 || between > 0, "two hunks that touch: {hunks:?}");
        for offset in 0..between {
            let (a, b) = (old_lines[old_at + offset], new_lines[new_at + offset]);
            assert_eq!(key(a), key(b), "{old:?} -> {new:?}: {hunks:?}");
        }
        assert!(hunk.old_lines.end <= old_lines.len() && change.lines.end <= new_lines.len());
        let kind = match (hunk.old_lines.is_empty(), change.lines.is_empty()) {
            (true, true) => panic!("a change without a line: {hunks:?}"),
            (true, false) => crate::diff::ChangeKind::Added,
            (false, true) => crate::diff::ChangeKind::Removed,
            (false, false) => crate::diff::ChangeKind::Modified,
        };
        assert_eq!(change.kind, kind, "{old:?} -> {new:?}: {hunks:?}");
        assert_eq!(
            change.bytes,
            starts[change.lines.start]..starts[change.lines.end],
            "{new:?}: {hunks:?}"
        );
        assert!(new.is_char_boundary(change.bytes.start) && new.is_char_boundary(change.bytes.end));
        old_at = hunk.old_lines.end;
        new_at = change.lines.end;
    }
    assert_eq!(
        old_lines.len() - old_at,
        new_lines.len() - new_at,
        "{old:?} -> {new:?}: {hunks:?}"
    );
    for (a, b) in old_lines[old_at..].iter().zip(&new_lines[new_at..]) {
        assert_eq!(key(a), key(b), "{old:?} -> {new:?}: {hunks:?}");
    }
}

/// The length of a longest common subsequence of lines, by the textbook table, to hold the search against.
pub(crate) fn longest_common_lines(old: &str, new: &str) -> usize {
    let a: Vec<(&str, bool)> = old.split_inclusive('\n').map(key).collect();
    let b: Vec<(&str, bool)> = new.split_inclusive('\n').map(key).collect();
    let mut row = vec![0usize; b.len() + 1];
    for line in &a {
        let mut diagonal = 0;
        for (index, other) in b.iter().enumerate() {
            let next = if line == other {
                diagonal + 1
            } else {
                row[index].max(row[index + 1])
            };
            diagonal = row[index + 1];
            row[index + 1] = next;
        }
    }
    row[b.len()]
}
