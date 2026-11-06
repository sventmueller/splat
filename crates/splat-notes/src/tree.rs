//! The tree of notes below a folder.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::fs::{DirEntry, FileType};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

const NOTE_EXTENSIONS: [&str; 4] = ["md", "markdown", "mdown", "mkd"];

/// Folders full of what somebody's tools made, which nobody reads as notes.
const LEFT_OUT: [&str; 5] = ["node_modules", "target", "Pods", "DerivedData", "__pycache__"];

const DEEPEST: usize = 64;

/// The most notes and folders a tree holds; beyond that the walk stops, so that memory stays bounded on any volume.
pub(crate) const MOST_ENTRIES: usize = 50_000;

/// Whether a file is a note by its extension: md, markdown, mdown, mkd, in any case.
pub fn is_note(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| NOTE_EXTENSIONS.iter().any(|note| note.eq_ignore_ascii_case(extension)))
}

/// The folder to show for a document: the Git work tree it lies in (nearest `.git` file or folder), unless that also
/// holds `home`, else the document's folder. Any `.git` marks a project here, also one that `splat_git::find` passes over
/// because Git could not read it.
pub fn root_for(file: &Path, home: Option<&Path>) -> PathBuf {
    let folder = file.parent().unwrap_or(file);
    let work_tree = folder.ancestors().find(|at| at.join(".git").exists());
    match work_tree {
        Some(tree) if home.is_none_or(|home| !home.starts_with(tree)) => tree.to_path_buf(),
        _ => folder.to_path_buf(),
    }
}

/// A note or a folder of notes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Node {
    name: String,
    path: PathBuf,
    folder: bool,
    children: Vec<usize>,
}

impl Node {
    /// The name to show; the path holds the real one, which need not be valid UTF-8.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where it lies, by its real name.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether it is a folder, with children, and not a note.
    pub fn is_folder(&self) -> bool {
        self.folder
    }

    /// What a folder holds, as indexes into `Tree::nodes` of the tree it is from: folders first, then by name as the
    /// Finder sorts, case-insensitive with numbers by value.
    pub fn children(&self) -> &[usize] {
        &self.children
    }
}

/// The notes below a folder, and the folders that lead to them. Only `scan`, `scan_flat`, `of_notes` and `renamed`
/// make one, so every index it holds names one of its nodes and every node but those at the top stands in exactly one
/// folder.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Tree {
    root: PathBuf,
    nodes: Vec<Node>,
    top: Vec<usize>,
    cut: bool,
    unreadable: usize,
}

impl Tree {
    /// The folder the tree shows, which is no node itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The notes and folders, in the order they show, each folder before what it holds.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The top level, as indexes into `nodes`.
    pub fn top(&self) -> &[usize] {
        &self.top
    }

    /// Whether the walk stopped at the most entries a tree may hold.
    pub fn is_cut(&self) -> bool {
        self.cut
    }

    /// How many folders and entries in them the walk could not read, and so left out.
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }

    /// The notes, in the order they show.
    pub fn notes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(|node| !node.folder)
    }

    /// The tree `scan` would make of a folder `root` that holds the notes `notes`, given as paths relative to `root`
    /// with `/` between the names, built without looking at the disk, for tests and fuzz targets.
    pub fn of_notes(root: &Path, notes: &[&str]) -> Tree {
        let mut found: Vec<Found> = Vec::new();
        let mut known: HashMap<(Option<usize>, &str), usize> = HashMap::new();
        for note in notes {
            let names: Vec<&str> = note.split('/').filter(|name| !name.is_empty()).collect();
            let mut parent = None;
            let mut path = root.to_path_buf();
            for (at, &name) in names.iter().enumerate() {
                path.push(name);
                let holds_more = at + 1 < names.len();
                let index = *known.entry((parent, name)).or_insert_with(|| {
                    found.push(Found {
                        node: Node {
                            name: name.to_string(),
                            path: path.clone(),
                            folder: holds_more,
                            children: Vec::new(),
                        },
                        parent,
                    });
                    found.len() - 1
                });
                // A name given as a note and as a folder is the folder, as a volume has one or the other.
                found[index].node.folder |= holds_more;
                parent = Some(index);
            }
        }
        assemble(root, found, false, 0)
    }

    /// The tree as it will be once the note `from` is called `to`, in the same folder.
    pub fn renamed(&self, from: &Path, to: &Path) -> Tree {
        let mut tree = self.clone();
        let Some(at) = tree.nodes.iter().position(|node| node.path == from) else {
            return tree;
        };
        tree.nodes[at].path = to.to_path_buf();
        tree.nodes[at].name = shown_name(to.file_name().unwrap_or_default());
        let mut top = std::mem::take(&mut tree.top);
        tree.sort_if_holding(&mut top, at);
        tree.top = top;
        for folder in 0..tree.nodes.len() {
            let mut children = std::mem::take(&mut tree.nodes[folder].children);
            tree.sort_if_holding(&mut children, at);
            tree.nodes[folder].children = children;
        }
        tree
    }

    fn sort_if_holding(&self, list: &mut [usize], at: usize) {
        if list.contains(&at) {
            list.sort_by_cached_key(|&node| sort_key(&self.nodes[node]));
        }
    }
}

fn shown_name(name: &OsStr) -> String {
    name.to_string_lossy().into_owned()
}

/// What a name sorts by: letters without their accents and case, as the Finder takes them, and a number as its count of
/// digits and these, so that `2` comes before `10` at any length. `0` stands in front of a number to keep it among the
/// digits. It is not `splat_git::folded_name`, which says which names a volume takes for one file: an order puts
/// `Äpfel` beside `apfel`, which are two files to a volume.
fn name_order(name: &str) -> Vec<u32> {
    let mut key = Vec::with_capacity(name.len());
    let mut chars = name
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .peekable();
    while let Some(c) = chars.next() {
        if !c.is_ascii_digit() {
            key.push(u32::from(c));
            continue;
        }
        let mut digits = Vec::new();
        if c != '0' {
            digits.push(u32::from(c));
        }
        while let Some(digit) = chars.next_if(char::is_ascii_digit) {
            if digit != '0' || !digits.is_empty() {
                digits.push(u32::from(digit));
            }
        }
        key.push(u32::from('0'));
        key.push(u32::try_from(digits.len()).unwrap_or(u32::MAX));
        key.append(&mut digits);
    }
    key
}

fn sort_key(node: &Node) -> (bool, Vec<u32>, String) {
    (!node.folder, name_order(&node.name), node.name.clone())
}

/// Whether to keep what `entry` is: a folder (`true`) or a note (`false`). Only a note behind a symlink is worth a look
/// at the disk; a symlink to a folder could lead back up. A symlink that leads nowhere is no note, one whose target
/// cannot be looked at is unreadable.
fn kind_of(entry: &DirEntry, kind: FileType) -> io::Result<Option<bool>> {
    let name = entry.file_name();
    if kind.is_dir() {
        let left_out = LEFT_OUT.iter().any(|left| name == OsStr::new(left));
        return Ok((!left_out).then_some(true));
    }
    if !is_note(Path::new(&name)) {
        return Ok(None);
    }
    if kind.is_file() {
        return Ok(Some(false));
    }
    match std::fs::metadata(entry.path()) {
        Ok(meta) => Ok(meta.is_file().then_some(false)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// What the walk found, in the order it found it, so a folder stands before what it holds.
struct Found {
    node: Node,
    parent: Option<usize>,
}

/// The tree below `root`. What is hidden, tools' folders, symlinks to folders, folders without a note below them and
/// other files stay out; so does what lies more than 64 folders deep. The walk goes level by level, so what a cut
/// leaves out is the deepest.
pub fn scan(root: &Path, stop: &AtomicBool) -> Tree {
    walk(root, stop, MOST_ENTRIES)
}

/// The notes right in a folder, for one too large to walk, such as a home folder.
pub fn scan_flat(root: &Path, stop: &AtomicBool) -> Tree {
    walk_down_to(0, root, stop, MOST_ENTRIES)
}

fn walk(root: &Path, stop: &AtomicBool, most: usize) -> Tree {
    walk_down_to(DEEPEST, root, stop, most)
}

fn walk_down_to(deepest: usize, root: &Path, stop: &AtomicBool, most: usize) -> Tree {
    let mut found: Vec<Found> = Vec::new();
    let mut cut = false;
    let mut unreadable = 0;
    let mut queue = VecDeque::from([(None, root.to_path_buf(), 1)]);
    'walk: while let Some((parent, folder, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            unreadable += 1;
            continue;
        };
        for entry in entries {
            if stop.load(Ordering::Relaxed) {
                break 'walk;
            }
            let Ok(entry) = entry else {
                unreadable += 1;
                continue;
            };
            let name = entry.file_name();
            if name.as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let is_folder = match entry.file_type().and_then(|kind| kind_of(&entry, kind)) {
                Ok(Some(is_folder)) => is_folder,
                Ok(None) => continue,
                Err(_) => {
                    unreadable += 1;
                    continue;
                }
            };
            if is_folder && depth > deepest {
                continue;
            }
            if found.len() == most {
                cut = true;
                break 'walk;
            }
            let path = entry.path();
            if is_folder {
                queue.push_back((Some(found.len()), path.clone(), depth + 1));
            }
            found.push(Found {
                node: Node {
                    name: shown_name(&name),
                    path,
                    folder: is_folder,
                    children: Vec::new(),
                },
                parent,
            });
        }
    }
    assemble(root, found, cut, unreadable)
}

/// The tree of what the walk found: folders without a note go, and the rest gets its order and final indexes.
fn assemble(root: &Path, found: Vec<Found>, cut: bool, unreadable: usize) -> Tree {
    let mut holds_note: Vec<bool> = found.iter().map(|f| !f.node.folder).collect();
    for at in (0..found.len()).rev() {
        if let (true, Some(parent)) = (holds_note[at], found[at].parent) {
            holds_note[parent] = true;
        }
    }
    let mut top = Vec::new();
    let mut held: Vec<Vec<usize>> = vec![Vec::new(); found.len()];
    for (at, item) in found.iter().enumerate().filter(|(at, _)| holds_note[*at]) {
        match item.parent {
            Some(parent) => held[parent].push(at),
            None => top.push(at),
        }
    }
    let sorted = |list: &mut Vec<usize>| list.sort_by_cached_key(|&at| sort_key(&found[at].node));
    sorted(&mut top);
    held.iter_mut().for_each(sorted);

    let mut order = Vec::with_capacity(found.len());
    let mut stack: Vec<usize> = top.iter().rev().copied().collect();
    while let Some(at) = stack.pop() {
        order.push(at);
        stack.extend(held[at].iter().rev());
    }
    let mut index_of = vec![0; found.len()];
    for (index, &at) in order.iter().enumerate() {
        index_of[at] = index;
    }
    let renumbered = |list: &[usize]| list.iter().map(|&at| index_of[at]).collect::<Vec<usize>>();
    let mut items: Vec<Option<Node>> = found.into_iter().map(|f| Some(f.node)).collect();
    let nodes = order
        .iter()
        .filter_map(|&at| {
            let mut node = items[at].take()?;
            node.children = renumbered(&held[at]);
            Some(node)
        })
        .collect();
    Tree {
        root: root.to_path_buf(),
        nodes,
        top: renumbered(&top),
        cut,
        unreadable,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStrExt;

    use super::*;
    use crate::search::tests::Folder;

    fn no_stop() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn a_flat_walk_stays_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("splat-flat-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("unter")).unwrap();
        std::fs::write(dir.join("a.md"), "").unwrap();
        std::fs::write(dir.join("unter/b.md"), "").unwrap();
        let tree = scan_flat(&dir, &no_stop());
        let paths: Vec<_> = tree.notes().map(|note| note.name.clone()).collect();
        assert_eq!(paths, ["a.md"]);
        assert_eq!(scan(&dir, &no_stop()).notes().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn names(tree: &Tree, list: &[usize]) -> Vec<String> {
        list.iter().map(|&at| tree.nodes[at].name.clone()).collect()
    }

    fn at_path<'t>(tree: &'t Tree, path: &[&str]) -> &'t Node {
        let mut list = &tree.top;
        let mut node = None;
        for part in path {
            let found = list.iter().find(|&&at| tree.nodes[at].name == *part).expect("a node");
            node = Some(&tree.nodes[*found]);
            list = &tree.nodes[*found].children;
        }
        node.expect("a path")
    }

    #[test]
    fn a_note_goes_by_its_extension_in_any_case() {
        for note in ["a.md", "a.MD", "b/a.markdown", "a.MDown", "a.mkd", "a.b.md"] {
            assert!(is_note(Path::new(note)), "{note}");
        }
        for other in ["a.txt", "a", ".md", "a.md.bak", "a.mdx"] {
            assert!(!is_note(Path::new(other)), "{other}");
        }
    }

    /// The least a folder needs to be a git directory.
    fn git_directory(at: &Path) {
        std::fs::create_dir_all(at.join("objects")).unwrap();
        std::fs::create_dir_all(at.join("refs")).unwrap();
        std::fs::write(at.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    }

    #[test]
    fn the_root_of_a_document_is_its_work_tree_unless_that_holds_home() {
        let folder = Folder::new("root");
        let root = folder.path();
        git_directory(&root.join("repo/.git"));
        std::fs::create_dir_all(root.join("repo/a/b")).unwrap();
        git_directory(&root.join("elsewhere.git"));
        std::fs::create_dir_all(root.join("worktree/c")).unwrap();
        let pointer = format!("gitdir: {}\n", root.join("elsewhere.git").display());
        std::fs::write(root.join("worktree/.git"), pointer).unwrap();
        std::fs::create_dir_all(root.join("plain/d")).unwrap();
        let doc = root.join("repo/a/b/n.md");
        assert_eq!(root_for(&doc, None), root.join("repo"));
        assert_eq!(root_for(&doc, Some(&root.join("elsewhere"))), root.join("repo"));
        assert_eq!(
            root_for(&doc, Some(&root.join("repo"))),
            root.join("repo/a/b"),
            "home is the work tree"
        );
        assert_eq!(
            root_for(&doc, Some(&root.join("repo/a"))),
            root.join("repo/a/b"),
            "home lies in the work tree"
        );
        assert_eq!(
            root_for(&root.join("worktree/c/n.md"), None),
            root.join("worktree"),
            "a file named .git"
        );
        assert_eq!(root_for(&root.join("plain/d/n.md"), None), root.join("plain/d"));
    }

    #[test]
    fn a_nested_work_tree_is_the_nearest_one() {
        let folder = Folder::new("nested");
        let root = folder.path();
        git_directory(&root.join("outer/.git"));
        git_directory(&root.join("outer/inner/.git"));
        assert_eq!(root_for(&root.join("outer/inner/n.md"), None), root.join("outer/inner"));
    }

    #[test]
    fn any_dot_git_marks_a_work_tree() {
        let folder = Folder::new("any-git");
        let root = folder.path();
        std::fs::create_dir_all(root.join("outer/empty/.git")).unwrap();
        assert_eq!(
            root_for(&root.join("outer/empty/n.md"), None),
            root.join("outer/empty"),
            "a .git folder without HEAD, objects and refs"
        );
        std::fs::create_dir_all(root.join("broken/d")).unwrap();
        std::fs::write(root.join("broken/.git"), "gitdir: gone\n").unwrap();
        assert_eq!(
            root_for(&root.join("broken/d/n.md"), None),
            root.join("broken"),
            "a .git file that leads nowhere"
        );
    }

    #[test]
    fn the_root_keeps_the_spelling_of_the_document_path() {
        let folder = Folder::new("spelling");
        let root = folder.path();
        git_directory(&root.join("repo/.git"));
        std::fs::create_dir_all(root.join("repo/a")).unwrap();
        std::os::unix::fs::symlink(root.join("repo"), root.join("link")).unwrap();
        assert_eq!(root_for(&root.join("link/a/n.md"), None), root.join("link"));
        assert_eq!(root_for(&root.join("repo/a/n.md"), None), root.join("repo"));
    }

    #[test]
    fn a_tree_of_notes_is_one_the_scanner_would_make() {
        let tree = Tree::of_notes(Path::new("/v"), &["b.md", "a/c.md", "a/d/e.md", "a.md"]);
        assert_eq!(tree.root(), Path::new("/v"));
        assert_eq!(names(&tree, tree.top()), ["a", "a.md", "b.md"]);
        assert_eq!(names(&tree, at_path(&tree, &["a"]).children()), ["d", "c.md"]);
        assert_eq!(at_path(&tree, &["a", "d", "e.md"]).path(), Path::new("/v/a/d/e.md"));
        assert!(at_path(&tree, &["a", "d"]).is_folder() && !at_path(&tree, &["a.md"]).is_folder());
        assert_eq!(at_path(&tree, &["a.md"]).name(), "a.md");
        assert!(!tree.is_cut() && tree.unreadable() == 0);
        assert_eq!(tree.nodes().len(), 6);
        let odd = Tree::of_notes(Path::new("/v"), &["a", "a/b.md", "//c.md", ""]);
        assert_eq!(names(&odd, odd.top()), ["a", "c.md"]);
        assert!(
            at_path(&odd, &["a"]).is_folder(),
            "a name that holds another is a folder"
        );
    }

    #[test]
    fn the_walk_keeps_notes_and_the_folders_that_lead_to_them() {
        let folder = Folder::new("walk");
        folder.note("a.md", "");
        folder.note("b/c.MD", "");
        folder.note("b/d/e.markdown", "");
        folder.note("f/g.txt", "");
        folder.note("h/i/j.txt", "");
        folder.note(".hidden/k.md", "");
        folder.note(".l.md", "");
        folder.note("node_modules/m.md", "");
        folder.note("target/n.md", "");
        folder.note("Pods/o.md", "");
        folder.note("DerivedData/p.md", "");
        folder.note("__pycache__/q.md", "");
        folder.note("r.txt", "");
        folder.note("target.md", "");
        std::fs::create_dir_all(folder.path().join("empty")).unwrap();
        let tree = scan(folder.path(), &no_stop());
        assert_eq!(tree.root, folder.path());
        assert_eq!(names(&tree, &tree.top), ["b", "a.md", "target.md"]);
        assert_eq!(names(&tree, &at_path(&tree, &["b"]).children), ["d", "c.MD"]);
        assert!(!tree.cut);
        let notes: Vec<&str> = tree.notes().map(|n| n.name.as_str()).collect();
        assert_eq!(
            notes,
            ["e.markdown", "c.MD", "a.md", "target.md"],
            "in the order they show"
        );
        assert_eq!(
            at_path(&tree, &["b", "d", "e.markdown"]).path,
            folder.path().join("b/d/e.markdown")
        );
    }

    #[test]
    fn names_sort_as_the_finder_sorts_them() {
        let folder = Folder::new("order");
        for name in [
            "note10.md",
            "note2.md",
            "Note1.md",
            "Zebra.md",
            "äpfel.md",
            "Birne.md",
            "a.md",
            "A.md",
            "note02.md",
            "99999999999999999999999.md",
            "100000000000000000000000.md",
        ] {
            folder.note(name, "");
        }
        folder.note("z folder/x.md", "");
        folder.note("B folder/x.md", "");
        let tree = scan(folder.path(), &no_stop());
        let found = names(&tree, &tree.top);
        let case_sensitive_volume = found.iter().any(|name| name == "a.md") && found.iter().any(|name| name == "A.md");
        let a = if case_sensitive_volume {
            vec!["A.md", "a.md"]
        } else {
            vec!["a.md"]
        };
        let mut wanted = vec![
            "B folder",
            "z folder",
            "99999999999999999999999.md",
            "100000000000000000000000.md",
        ];
        wanted.extend(a);
        wanted.extend([
            "äpfel.md",
            "Birne.md",
            "Note1.md",
            "note02.md",
            "note2.md",
            "note10.md",
            "Zebra.md",
        ]);
        assert_eq!(found, wanted);
    }

    #[test]
    fn a_number_sorts_by_its_value() {
        let order = |a: &str, b: &str| name_order(a).cmp(&name_order(b));
        assert!(order("a2", "a10").is_lt());
        assert!(order("a007", "a8").is_lt());
        assert!(order("a0", "a1").is_lt());
        assert!(order("a1b", "a01b").is_eq(), "the name itself decides then");
        assert!(order("1", "a").is_lt());
        assert!(order("Äb", "ac").is_lt());
        assert!(order("a\u{308}b", "äb").is_eq());
        let (first, second) = (sort_key(&note("a01")), sort_key(&note("a1")));
        assert!(first < second);
        assert!(sort_key(&folder_node("z")) < sort_key(&note("a")));
    }

    fn note(name: &str) -> Node {
        Node {
            name: name.to_string(),
            path: PathBuf::from(name),
            folder: false,
            children: Vec::new(),
        }
    }

    fn folder_node(name: &str) -> Node {
        Node {
            folder: true,
            ..note(name)
        }
    }

    #[test]
    fn symlinks_lead_to_notes_but_never_to_folders() {
        let folder = Folder::new("links");
        folder.note("real/a.md", "");
        folder.note("elsewhere.txt", "");
        let root = folder.path();
        std::os::unix::fs::symlink(root.join("real"), root.join("real/up")).unwrap();
        std::os::unix::fs::symlink(root, root.join("real/cycle")).unwrap();
        std::os::unix::fs::symlink(root.join("real/a.md"), root.join("b.md")).unwrap();
        std::os::unix::fs::symlink(root.join("gone.md"), root.join("dangling.md")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("folder.md")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere.txt"), root.join("c.txt")).unwrap();
        let tree = scan(root, &no_stop());
        assert_eq!(names(&tree, &tree.top), ["real", "b.md"]);
        assert_eq!(names(&tree, &at_path(&tree, &["real"]).children), ["a.md"]);
    }

    #[test]
    fn a_name_that_is_no_text_shows_as_far_as_it_can_and_keeps_its_path() {
        let name = OsStr::from_bytes(b"caf\xE9.md");
        assert_eq!(shown_name(name), "caf\u{FFFD}.md");
        assert!(is_note(Path::new(name)));
        let folder = Folder::new("bytes");
        let path = folder.path().join(name);
        // APFS refuses such a name, other volumes take it.
        if std::fs::write(&path, "").is_ok() {
            let tree = scan(folder.path(), &no_stop());
            assert_eq!(tree.nodes[0].name, "caf\u{FFFD}.md");
            assert_eq!(tree.nodes[0].path, path);
        }
    }

    #[test]
    fn the_walk_stops_at_its_limit_and_says_so() {
        let folder = Folder::new("cut");
        for at in 0..8 {
            folder.note(&format!("a{at}.md"), "");
        }
        folder.note("b/deep.md", "");
        let tree = walk(folder.path(), &no_stop(), 4);
        assert!(tree.cut);
        assert!(tree.nodes.len() <= 4, "{}", tree.nodes.len());
        let whole = walk(folder.path(), &no_stop(), 10);
        assert!(!whole.cut);
        assert_eq!(whole.nodes.len(), 10);
        assert_eq!(MOST_ENTRIES, 50_000);
    }

    #[test]
    fn the_walk_goes_no_deeper_than_its_limit() {
        let folder = Folder::new("deep");
        let mut path = String::new();
        for _ in 0..70 {
            path.push_str("d/");
        }
        folder.note(&format!("{path}too-deep.md"), "");
        let mut reachable = String::new();
        for _ in 0..DEEPEST {
            reachable.push_str("d/");
        }
        folder.note(&format!("{reachable}deepest.md"), "");
        let tree = scan(folder.path(), &no_stop());
        let notes: Vec<&str> = tree.notes().map(|n| n.name.as_str()).collect();
        assert_eq!(notes, ["deepest.md"]);
    }

    #[test]
    fn a_stop_ends_the_walk_with_what_it_has() {
        let folder = Folder::new("stopped");
        folder.note("a.md", "");
        assert!(scan(folder.path(), &AtomicBool::new(true)).nodes.is_empty());
    }

    #[test]
    fn what_cannot_be_read_gives_an_empty_tree() {
        let tree = scan(Path::new("/does/not/exist/at/all"), &no_stop());
        assert_eq!(tree.nodes, []);
        assert_eq!(tree.root, Path::new("/does/not/exist/at/all"));
        assert_eq!(tree.unreadable, 1);
    }

    #[test]
    fn folders_and_links_that_cannot_be_read_are_counted_and_the_rest_shows() {
        use std::os::unix::fs::PermissionsExt;
        let folder = Folder::new("locked");
        let root = folder.path();
        folder.note("a.md", "");
        folder.note("locked/b.md", "");
        folder.note("open/c.md", "");
        std::os::unix::fs::symlink(root.join("locked/b.md"), root.join("open/via.md")).unwrap();
        let lock = |mode| std::fs::set_permissions(root.join("locked"), std::fs::Permissions::from_mode(mode)).unwrap();
        lock(0o000);
        let tree = scan(root, &no_stop());
        lock(0o755);
        assert_eq!(names(&tree, &tree.top), ["open", "a.md"]);
        assert_eq!(names(&tree, &at_path(&tree, &["open"]).children), ["c.md"]);
        assert_eq!(tree.unreadable, 2, "the locked folder and the link into it");
        assert_eq!(scan(root, &no_stop()).unreadable, 0);
    }

    #[test]
    fn a_tree_can_say_how_it_looks_after_a_rename() {
        let folder = Folder::new("renamed");
        folder.note("a.md", "");
        folder.note("b.md", "");
        folder.note("sub/c.md", "");
        folder.note("sub/d.md", "");
        let root = folder.path();
        let tree = scan(root, &no_stop());
        let later = tree.renamed(&root.join("a.md"), &root.join("z.md"));
        assert_eq!(names(&later, &later.top), ["sub", "b.md", "z.md"]);
        let later = tree.renamed(&root.join("sub/d.md"), &root.join("sub/a.md"));
        assert_eq!(names(&later, &at_path(&later, &["sub"]).children), ["a.md", "c.md"]);
        assert_eq!(tree.renamed(&root.join("none.md"), &root.join("z.md")), tree);
    }
}
