//! Where a link in a note leads, by what the tree holds: the same answers as `find_note` and `classify` in the app's
//! `src/editor/links.rs`, which ask the disk.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use splat_markdown::url::scheme;
use unicode_normalization::UnicodeNormalization;

use crate::tree::Tree;

/// How many folders below a document a wiki link reaches.
const SEARCH_DEPTH: usize = 8;

/// What makes two names the same to the volumes of a Mac: neither the case of ASCII letters nor the normal form of what
/// is not.
fn name_key(name: &str) -> String {
    name.nfc().map(|c| c.to_ascii_lowercase()).collect()
}

fn path_key(path: &Path) -> String {
    name_key(&path.to_string_lossy())
}

/// `path` without its `.` and `..`, as far as the path itself tells them.
pub(crate) fn tidied(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.as_os_str().is_empty() || out.ends_with("..") {
                    out.push("..");
                } else {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn strip_suffixes(target: &str) -> &str {
    &target[..target.find(['?', '#']).unwrap_or(target.len())]
}

/// Decodes `%XX` escapes; invalid escapes and bytes stay as they are.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |at: usize| bytes.get(at).and_then(|b| char::from(*b).to_digit(16));
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let (Some(high), Some(low)) = (hex(at + 1), hex(at + 2))
        {
            out.extend(u8::try_from(high * 16 + low));
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Answers where links lead in a tree. Making one goes through all of the tree once, and each answer then takes no
/// longer than its link is, so it is what to use for more than a few links.
pub(crate) struct Resolver<'t> {
    exact: HashMap<&'t Path, &'t Path>,
    loose: HashMap<String, &'t Path>,
    folders: HashMap<&'t Path, usize>,
    names: HashMap<String, usize>,
    /// For a folder and a name, the nearest note of that name at or below the folder: how deep it lies, and which comes
    /// first in the order of the tree.
    nearest: HashMap<(usize, usize), (usize, &'t Path)>,
}

impl<'t> Resolver<'t> {
    /// The answers for `tree`.
    pub(crate) fn new(tree: &'t Tree) -> Resolver<'t> {
        let mut resolver = Resolver {
            exact: HashMap::new(),
            loose: HashMap::new(),
            folders: HashMap::from([(tree.root.as_path(), 0)]),
            names: HashMap::new(),
            nearest: HashMap::new(),
        };
        let mut seen = vec![false; tree.nodes.len()];
        // Folders from the root down to where the walk stands; children go on the stack with how many folders stand
        // above them.
        let mut chain = vec![0];
        let mut stack: Vec<(usize, usize)> = tree.top.iter().rev().map(|&at| (at, 1)).collect();
        while let Some((at, depth)) = stack.pop() {
            let Some(node) = tree.nodes.get(at).filter(|_| !std::mem::replace(&mut seen[at], true)) else {
                continue;
            };
            chain.truncate(depth);
            if node.folder {
                chain.push(resolver.folders.len());
                let id = resolver.folders.len();
                resolver.folders.insert(&node.path, id);
                stack.extend(node.children.iter().rev().map(|&child| (child, depth + 1)));
            } else {
                resolver.add_note(&node.path, &chain);
            }
        }
        resolver
    }

    fn add_note(&mut self, path: &'t Path, chain: &[usize]) {
        self.exact.entry(path).or_insert(path);
        self.loose.entry(path_key(path)).or_insert(path);
        let name = name_key(&path.file_name().unwrap_or_default().to_string_lossy());
        let next_id = self.names.len();
        let name = *self.names.entry(name).or_insert(next_id);
        for (depth, &folder) in chain.iter().rev().enumerate().take(SEARCH_DEPTH + 1) {
            let best = self.nearest.entry((folder, name)).or_insert((depth, path));
            if depth < best.0 {
                *best = (depth, path);
            }
        }
    }

    /// The note that lies at `path`, if the tree holds one, as a volume of a Mac would find it.
    fn note_at(&self, path: &Path) -> Option<&'t Path> {
        self.exact
            .get(path)
            .or_else(|| self.loose.get(&path_key(path)))
            .copied()
    }

    /// Where the wiki link naming `name` in the note `from_file` leads: a note of that name in the note's folder, else
    /// the nearest one below it, as far as `find_note` in the app looks. `#heading`, `^block` and `|title` do not
    /// matter.
    pub(crate) fn wiki(&self, name: &str, from_file: &Path) -> Option<PathBuf> {
        let name = name.trim().split(['#', '|', '^']).next()?.trim();
        if name.is_empty() || name.contains("..") || Path::new(name).has_root() {
            return None;
        }
        let dir = from_file.parent()?;
        let direct = self
            .note_at(&dir.join(name))
            .or_else(|| self.note_at(&dir.join(format!("{name}.md"))));
        if let Some(note) = direct {
            return Some(note.to_path_buf());
        }
        let file = Path::new(name).file_name().and_then(OsStr::to_str)?;
        let folder = *self.folders.get(dir)?;
        [name_key(file), name_key(&format!("{file}.md"))]
            .iter()
            .filter_map(|key| self.nearest.get(&(folder, *self.names.get(key)?)))
            .min_by_key(|(depth, _)| *depth)
            .map(|(_, note)| note.to_path_buf())
    }

    /// Where the Markdown address `address` in the note `from_file` leads: to a path, which need not be a note of the
    /// tree. A heading in the same note and whatever has a scheme other than `file:` lead to no file.
    pub(crate) fn address(&self, address: &str, from_file: &Path) -> Option<PathBuf> {
        let address = address.trim();
        if address.starts_with('#') {
            return None;
        }
        let path = match scheme(address).as_deref() {
            Some("file") => {
                let rest = &address["file:".len()..];
                let rest = rest
                    .strip_prefix("//localhost")
                    .or_else(|| rest.strip_prefix("//"))
                    .unwrap_or(rest);
                if !rest.starts_with('/') {
                    return None;
                }
                PathBuf::from(percent_decode(strip_suffixes(rest)))
            }
            Some(_) => return None,
            None => {
                let decoded = percent_decode(strip_suffixes(address));
                let path = PathBuf::from(&decoded);
                if decoded.is_empty() {
                    return None;
                } else if path.is_absolute() {
                    path
                } else if let Some(rest) = decoded.strip_prefix("~/")
                    && let Some(home) = std::env::var_os("HOME")
                {
                    PathBuf::from(home).join(rest)
                } else {
                    from_file.parent()?.join(path)
                }
            }
        };
        let path = tidied(&path);
        Some(self.note_at(&path).map_or(path, Path::to_path_buf))
    }

    /// Whether `one` and `other` are the same file, as far as the tree and the paths tell.
    pub(crate) fn same(&self, one: &Path, other: &Path) -> bool {
        let found = |path: &Path| {
            let path = tidied(path);
            self.note_at(&path).map_or(path, Path::to_path_buf)
        };
        found(one) == found(other)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::search::tests::Folder;
    use crate::tree::{Node, scan};

    /// Where a link leads, written as a document holds it: `[[Note|Title]]` and `![[Note]]` are wiki links; anything
    /// else is an address and, if that leads to no note of the tree, the bare name of a wiki link.
    fn link(resolver: &Resolver, link_target: &str, from_file: &Path) -> Option<PathBuf> {
        let written = link_target.trim();
        if let Some(inner) = written
            .strip_prefix('!')
            .unwrap_or(written)
            .strip_prefix("[[")
            .and_then(|rest| rest.strip_suffix("]]"))
        {
            return resolver.wiki(inner, from_file);
        }
        let address = resolver.address(written, from_file);
        if address.as_ref().is_some_and(|path| resolver.note_at(path).is_some()) || scheme(written).is_some() {
            return address;
        }
        resolver.wiki(written, from_file).or(address)
    }

    fn resolve(link_target: &str, from_file: &Path, tree: &Tree) -> Option<PathBuf> {
        link(&Resolver::new(tree), link_target, from_file)
    }

    fn resolves_to(link_target: &str, from_file: &Path, target: &Path, tree: &Tree) -> bool {
        let resolver = Resolver::new(tree);
        link(&resolver, link_target, from_file).is_some_and(|leads_to| resolver.same(&leads_to, target))
    }

    /// A tree that exists only in memory.
    pub(crate) fn tree_of(root: &str, notes: &[&str]) -> Tree {
        let mut tree = Tree {
            root: PathBuf::from(root),
            ..Tree::default()
        };
        for note in notes {
            let mut list_owner: Option<usize> = None;
            let mut path = PathBuf::from(root);
            let parts: Vec<&str> = note.split('/').collect();
            for (at, part) in parts.iter().enumerate() {
                path.push(part);
                let list = match list_owner {
                    Some(owner) => &tree.nodes[owner].children,
                    None => &tree.top,
                };
                let known = list.iter().copied().find(|&child| tree.nodes[child].name == *part);
                let index = known.unwrap_or_else(|| {
                    let index = tree.nodes.len();
                    tree.nodes.push(Node {
                        name: (*part).to_string(),
                        path: path.clone(),
                        folder: at + 1 < parts.len(),
                        children: Vec::new(),
                    });
                    match list_owner {
                        Some(owner) => tree.nodes[owner].children.push(index),
                        None => tree.top.push(index),
                    }
                    index
                });
                list_owner = Some(index);
            }
        }
        tree
    }

    fn leads(tree: &Tree, link: &str, from: &str) -> Option<String> {
        resolve(link, Path::new(from), tree).map(|path| path.display().to_string())
    }

    fn vault() -> Tree {
        tree_of(
            "/v",
            &[
                "Note.md",
                "Other.markdown",
                "My Note.md",
                "Müller.md",
                "100%.md",
                "a%20b.md",
                "C#.md",
                "q?.md",
                "sub/Deep.md",
                "sub/Note.md",
                "sub/below/Lower.md",
                "sub/below/Note.md",
                "x/Same.md",
                "y/Same.md",
            ],
        )
    }

    #[test]
    fn a_wiki_link_names_a_note_in_the_same_folder_first() {
        let tree = vault();
        for (link, wanted) in [
            ("[[Note]]", "/v/Note.md"),
            ("[[Note|Titel]]", "/v/Note.md"),
            ("[[Note#Heading]]", "/v/Note.md"),
            ("[[Note#Heading|Titel]]", "/v/Note.md"),
            ("[[Note^block]]", "/v/Note.md"),
            ("![[Note]]", "/v/Note.md"),
            ("[[ Note ]]", "/v/Note.md"),
            ("[[Note.md]]", "/v/Note.md"),
            ("[[note]]", "/v/Note.md"),
            ("[[My Note]]", "/v/My Note.md"),
            ("[[Other.markdown]]", "/v/Other.markdown"),
            ("[[Mu\u{308}ller]]", "/v/Müller.md"),
            ("[[sub/Deep]]", "/v/sub/Deep.md"),
            ("[[sub/Deep.md]]", "/v/sub/Deep.md"),
            ("[[Deep]]", "/v/sub/Deep.md"),
            ("[[Lower]]", "/v/sub/below/Lower.md"),
            ("[[elsewhere/Deep]]", "/v/sub/Deep.md"),
        ] {
            assert_eq!(leads(&tree, link, "/v/from.md").as_deref(), Some(wanted), "{link}");
        }
    }

    #[test]
    fn a_wiki_link_looks_nowhere_but_at_and_below_its_note() {
        let tree = vault();
        assert_eq!(
            leads(&tree, "[[Note]]", "/v/sub/from.md").as_deref(),
            Some("/v/sub/Note.md")
        );
        assert_eq!(
            leads(&tree, "[[Note]]", "/v/sub/below/from.md").as_deref(),
            Some("/v/sub/below/Note.md")
        );
        assert_eq!(leads(&tree, "[[Other]]", "/v/sub/from.md"), None, "above it");
        assert_eq!(leads(&tree, "[[Other]]", "/v/from.md"), None, "only .md is added");
        assert_eq!(
            leads(&tree, "[[Same]]", "/v/from.md").as_deref(),
            Some("/v/x/Same.md"),
            "first in the tree"
        );
        assert_eq!(
            leads(&tree, "[[Deep]]", "/w/from.md"),
            None,
            "a folder the tree does not know"
        );
        for link in ["[[]]", "[[#h]]", "[[|t]]", "[[../Note]]", "[[/v/Note]]", "[[a..b]]"] {
            assert_eq!(leads(&tree, link, "/v/from.md"), None, "{link}");
        }
        assert_eq!(leads(&tree, "[[Note]]", "/"), None, "a file without a folder");
    }

    #[test]
    fn a_wiki_link_reaches_eight_folders_down_as_the_app_does() {
        let deep = "a/b/c/d/e/f/g/h";
        let too_deep = "a/b/c/d/e/f/g/h/i";
        let tree = tree_of("/v", &[&format!("{deep}/In.md"), &format!("{too_deep}/Out.md")]);
        assert!(leads(&tree, "[[In]]", "/v/from.md").is_some());
        assert_eq!(leads(&tree, "[[Out]]", "/v/from.md"), None);
        assert!(leads(&tree, "[[Out]]", "/v/a/from.md").is_some());
    }

    #[test]
    fn the_nearer_of_two_notes_of_one_name_wins_however_the_tree_is_ordered() {
        let tree = tree_of("/v", &["z/k/N.md", "a/b/c/N.md"]);
        assert_eq!(leads(&tree, "[[N]]", "/v/from.md").as_deref(), Some("/v/z/k/N.md"));
        let tree = tree_of("/v", &["a/b/c/N.md", "z/k/N.md"]);
        assert_eq!(leads(&tree, "[[N]]", "/v/from.md").as_deref(), Some("/v/z/k/N.md"));
    }

    #[test]
    fn a_name_with_its_extension_and_one_without_compete_by_depth() {
        let tree = tree_of("/v", &["a/b/N.md.md", "a/N.md"]);
        assert_eq!(leads(&tree, "[[N.md]]", "/v/from.md").as_deref(), Some("/v/a/N.md"));
    }

    #[test]
    fn an_address_is_read_as_a_path_with_its_escapes() {
        let tree = vault();
        for (link, wanted) in [
            ("Note.md", "/v/Note.md"),
            ("./Note.md", "/v/Note.md"),
            ("Note.md#h", "/v/Note.md"),
            ("Note.md?x=1", "/v/Note.md"),
            ("My%20Note.md", "/v/My Note.md"),
            ("M%C3%BCller.md", "/v/Müller.md"),
            ("100%25.md", "/v/100%.md"),
            ("100%.md", "/v/100%.md"),
            ("a%2520b.md", "/v/a%20b.md"),
            ("C%23.md", "/v/C#.md"),
            ("q%3F.md", "/v/q?.md"),
            ("sub/../Note.md", "/v/Note.md"),
            ("/v/Note.md", "/v/Note.md"),
            ("file:///v/My%20Note.md", "/v/My Note.md"),
            ("file://localhost/v/Note.md", "/v/Note.md"),
            ("note.MD", "/v/Note.md"),
            (" Note.md ", "/v/Note.md"),
        ] {
            assert_eq!(leads(&tree, link, "/v/from.md").as_deref(), Some(wanted), "{link}");
        }
        assert_eq!(
            leads(&tree, "../Note.md", "/v/sub/from.md").as_deref(),
            Some("/v/Note.md")
        );
        assert_eq!(
            leads(&tree, "../../../Note.md", "/v/sub/from.md").as_deref(),
            Some("/Note.md")
        );
    }

    #[test]
    fn what_leads_to_no_file_is_no_link_to_a_note() {
        let tree = vault();
        for link in [
            "#heading",
            "https://example.com/Note.md",
            "mailto:a@b.c",
            "vscode://file/Note.md",
            "file://server/Note.md",
            "",
            "?x",
        ] {
            assert_eq!(leads(&tree, link, "/v/from.md"), None, "{link:?}");
        }
    }

    #[test]
    fn a_home_folder_is_expanded_where_there_is_one() {
        let tree = vault();
        if let Some(home) = std::env::var_os("HOME") {
            let wanted = Path::new(&home).join("x.md").display().to_string();
            assert_eq!(leads(&tree, "~/x.md", "/v/from.md"), Some(wanted));
        }
    }

    #[test]
    fn an_address_that_finds_no_note_may_be_the_bare_name_of_one() {
        let tree = vault();
        assert_eq!(leads(&tree, "Deep", "/v/from.md").as_deref(), Some("/v/sub/Deep.md"));
        assert_eq!(leads(&tree, "Deep#h", "/v/from.md").as_deref(), Some("/v/sub/Deep.md"));
        assert_eq!(leads(&tree, "Note|Titel", "/v/from.md").as_deref(), Some("/v/Note.md"));
        assert_eq!(
            leads(&tree, "Fehlt.md", "/v/from.md").as_deref(),
            Some("/v/Fehlt.md"),
            "a path all the same"
        );
        let resolver = Resolver::new(&tree);
        assert_eq!(
            resolver.address("Deep", Path::new("/v/from.md")),
            Some(PathBuf::from("/v/Deep"))
        );
        assert_eq!(
            resolver.wiki("sub/Deep.md", Path::new("/v/from.md")),
            Some(PathBuf::from("/v/sub/Deep.md"))
        );
    }

    #[test]
    fn resolves_to_says_whether_a_link_leads_to_a_note() {
        let tree = vault();
        let to = |link: &str, target: &str| resolves_to(link, Path::new("/v/from.md"), Path::new(target), &tree);
        assert!(to("[[Note]]", "/v/Note.md"));
        assert!(to("[[Note]]", "/v/note.md"), "as the volume would find it");
        assert!(!to("[[Note]]", "/v/sub/Note.md"));
        assert!(to("sub/Note.md", "/v/sub/Note.md"));
        assert!(to("sub/Note.md", "/v/sub/../sub/Note.md"));
        assert!(!to("#h", "/v/from.md"));
        assert!(!to("https://a.de", "/v/Note.md"));
        assert!(
            to("Elsewhere.md", "/v/Elsewhere.md"),
            "outside the tree the paths decide"
        );
    }

    #[test]
    fn a_tree_of_any_shape_gets_answers() {
        let mut tree = tree_of("/v", &["a/N.md"]);
        tree.nodes[0].children.push(0);
        tree.top.push(99);
        assert_eq!(
            leads(&tree, "[[N]]", "/v/from.md").as_deref(),
            Some("/v/a/N.md"),
            "a cycle and a wrong index"
        );
    }

    #[test]
    fn the_answers_from_the_tree_are_those_of_the_disk_the_app_reads() {
        let folder = Folder::new("disk");
        for note in ["Note.md", "sub/Deep.md", "sub/below/Lower.md"] {
            folder.note(note, "");
        }
        let root = folder.path();
        let tree = scan(root, &AtomicBool::new(false));
        let from = root.join("from.md");
        for (link, wanted) in [
            ("[[Note]]", "Note.md"),
            ("[[Deep]]", "sub/Deep.md"),
            ("[[Lower]]", "sub/below/Lower.md"),
        ] {
            assert_eq!(resolve(link, &from, &tree), Some(root.join(wanted)), "{link}");
        }
    }

    #[test]
    fn escapes_that_are_none_stay_as_they_stand() {
        assert_eq!(percent_decode("a%20b%2"), "a b%2");
        assert_eq!(percent_decode("%€.png"), "%€.png");
        assert_eq!(percent_decode("%C3%A4"), "ä");
        assert_eq!(percent_decode("%zz%"), "%zz%");
        assert_eq!(strip_suffixes("a.md?b#c"), "a.md");
        assert_eq!(tidied(Path::new("../a/./b/../c")), Path::new("../a/c"));
        assert_eq!(tidied(Path::new("../../a")), Path::new("../../a"));
        assert_eq!(tidied(Path::new("/../a")), Path::new("/a"));
    }
}
