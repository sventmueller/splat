//! Where a link in a document may lead when it is opened. A document can carry links to apps, scripts and
//! installers; none of them may start from a click.

use std::path::{Component, Path, PathBuf};

use crate::markdown::url::percent_decode;
use crate::preview::layout::Link;

/// Where a link leads, by what may happen when it opens.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// http, https and mailto, handed to the default browser or mail app.
    Web(String),
    /// A heading in the same document.
    Anchor(String),
    /// A Markdown file, opened in Splat.
    Document(PathBuf),
    /// Any other file or folder, shown in the Finder only.
    File(PathBuf),
    /// A URL another app handles, opened only after the user confirms it.
    App,
    /// Nothing to open, such as a relative path in an unsaved document.
    Nothing,
}

const MARKDOWN: [&str; 6] = ["md", "markdown", "mdown", "mkd", "mkdn", "mdwn"];

/// Where the link address `target` of a document in `doc_dir` leads.
pub fn classify(target: &str, doc_dir: Option<&Path>) -> LinkTarget {
    let target = target.trim();
    if let Some(anchor) = target.strip_prefix('#') {
        return LinkTarget::Anchor(anchor.to_string());
    }
    match crate::markdown::url::scheme(target).as_deref() {
        Some("http" | "https" | "mailto") => LinkTarget::Web(target.to_string()),
        Some("file") => {
            let rest = &target["file:".len()..];
            let rest = rest
                .strip_prefix("//localhost")
                .or_else(|| rest.strip_prefix("//"))
                .unwrap_or(rest);
            if rest.starts_with('/') {
                local(PathBuf::from(percent_decode(strip_suffixes(rest))))
            } else {
                LinkTarget::Nothing
            }
        }
        Some(_) => LinkTarget::App,
        None => {
            let decoded = percent_decode(strip_suffixes(target));
            if decoded.is_empty() {
                return LinkTarget::Nothing;
            }
            let path = PathBuf::from(&decoded);
            if path.is_absolute() {
                local(path)
            } else if let Some(rest) = decoded.strip_prefix("~/")
                && let Some(home) = crate::markdown::url::home()
            {
                local(home.join(rest))
            } else {
                doc_dir.map_or(LinkTarget::Nothing, |dir| local(dir.join(path)))
            }
        }
    }
}

/// A `[[wiki link]]`, which names a note instead of giving its path. Searching folders, it must stay off the main thread.
pub fn classify_wiki(name: &str, doc_dir: Option<&Path>) -> LinkTarget {
    doc_dir
        .and_then(|dir| find_note(dir, name.trim()))
        .map_or(LinkTarget::Nothing, local)
}

/// What has to be there for a link of the preview to lead somewhere.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Destination {
    /// A place in the document itself, by the anchor as the link writes it.
    Anchor(String),
    /// A file or folder.
    Path(PathBuf),
    /// The note a wiki link names, searched for below the document's folder.
    Note {
        /// The document's folder.
        dir: PathBuf,
        /// The name as the link writes it, with `#heading` or `|title` if it has them.
        name: String,
    },
}

/// What `link` of a document in `doc_dir` needs to lead somewhere, or `None` for a link to the web or another app, or one
/// an unsaved document cannot place.
pub fn destination(link: &Link, doc_dir: Option<&Path>) -> Option<Destination> {
    match link {
        Link::WikiPage(name) => {
            let name = name.trim();
            // `[[#Heading]]` and `[[^block]]` name a place in the document itself, which ⌘-click does not look for.
            if name.is_empty() || name.starts_with(['#', '^']) {
                return None;
            }
            Some(Destination::Note {
                dir: doc_dir?.to_path_buf(),
                name: name.to_string(),
            })
        }
        Link::Address(address) => match classify(address, doc_dir) {
            LinkTarget::Anchor(anchor) if !anchor.is_empty() => Some(Destination::Anchor(anchor)),
            LinkTarget::Document(path) | LinkTarget::File(path) => Some(Destination::Path(path)),
            _ => None,
        },
        _ => None,
    }
}

/// Whether nothing lies where `destination` points, or `None` where the disk does not say, as in a folder the App Sandbox
/// keeps closed, and for anchors, which are not on disk. It reads the disk, so it belongs on the thread of the volume.
pub fn missing_on_disk(destination: &Destination) -> Option<bool> {
    match destination {
        Destination::Anchor(_) => None,
        Destination::Path(path) => match std::fs::metadata(path) {
            Ok(_) => Some(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(true),
            Err(_) => None,
        },
        Destination::Note { dir, name } => {
            if find_note(dir, name).is_some() {
                Some(false)
            } else {
                // The sandbox lets Splat look at a path but not list a folder without a grant.
                std::fs::read_dir(dir).is_ok().then_some(true)
            }
        }
    }
}

/// Folders and entries searched below the document's folder for a note named in a wiki link.
const SEARCH_ENTRIES: usize = 20_000;
const SEARCH_DEPTH: usize = 8;

/// The file a wiki link or embed names: Obsidian finds notes by name anywhere in the vault, so after the
/// document's own folder the folders below it are searched. `#heading` and `|title` parts are ignored.
pub fn find_note(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.split(['#', '|', '^']).next()?.trim();
    if name.is_empty() || name.contains("..") || Path::new(name).has_root() {
        return None;
    }
    let root = dir.canonicalize().ok()?;
    let candidates = |base: &Path| {
        let plain = base.join(name);
        let with_md = base.join(format!("{name}.md"));
        [plain, with_md]
            .into_iter()
            .find(|p| p.is_file() && lies_within(p, &root))
    };
    if let Some(found) = candidates(dir) {
        return Some(found);
    }
    let file = Path::new(name).file_name()?.to_os_string();
    let wanted = [file.clone(), {
        let mut f = file;
        f.push(".md");
        f
    }];
    let mut queue = std::collections::VecDeque::from([(dir.to_path_buf(), 0)]);
    let mut seen = 0;
    while let Some((folder, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > SEARCH_ENTRIES {
                return None;
            }
            let entry_name = entry.file_name();
            if entry_name.to_string_lossy().starts_with('.') || entry_name == "node_modules" {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < SEARCH_DEPTH {
                queue.push_back((entry.path(), depth + 1));
            } else if kind.is_file() && wanted.iter().any(|w| w.eq_ignore_ascii_case(&entry_name)) {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Whether `path` is in the canonical folder `root` once its links are followed: a link named like a note may point at a
/// key in the home folder.
fn lies_within(path: &Path, root: &Path) -> bool {
    path.canonicalize().is_ok_and(|real| real.starts_with(root))
}

/// The file an image link names; a picture Obsidian embeds as `![[name.png]]` may sit in any folder below the
/// document's. Only there is it searched for, so a document cannot fish for a file name across the disk.
pub fn locate(path: &Path, doc_dir: Option<&Path>) -> PathBuf {
    let below_document = doc_dir
        .and_then(|dir| path.strip_prefix(dir).ok())
        .is_some_and(|rest| rest.components().all(|c| matches!(c, Component::Normal(_))));
    if path.exists() || !below_document {
        return path.to_path_buf();
    }
    path.parent()
        .zip(path.file_name().and_then(|n| n.to_str()))
        .and_then(|(dir, name)| find_note(dir, name))
        .unwrap_or_else(|| path.to_path_buf())
}

fn local(path: PathBuf) -> LinkTarget {
    let markdown = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MARKDOWN.contains(&e.to_ascii_lowercase().as_str()));
    if markdown {
        LinkTarget::Document(path)
    } else {
        LinkTarget::File(path)
    }
}

/// The path part of a link, without `?query` and `#fragment`.
pub fn strip_suffixes(target: &str) -> &str {
    let end = target.find(['?', '#']).unwrap_or(target.len());
    &target[..end]
}

/// Relative path from `dir` to `target`, falling back to the absolute path.
pub fn relative_path(dir: Option<&Path>, target: &Path) -> String {
    let Some(dir) = dir else {
        return target.to_string_lossy().into_owned();
    };
    let from: Vec<_> = dir.components().collect();
    let to: Vec<_> = target.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common <= 1 {
        return target.to_string_lossy().into_owned();
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for c in &to[common..] {
        out.push(c);
    }
    out.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_with_only_the_root_in_common_stay_absolute() {
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/c/d.png")),
            "/c/d.png"
        );
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/a/d.png")),
            "../d.png"
        );
        assert_eq!(relative_path(Some(Path::new("/a")), Path::new("/a/b/d.png")), "b/d.png");
        assert_eq!(relative_path(None, Path::new("/a/d.png")), "/a/d.png");
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/a/b/img/x.png")),
            "img/x.png"
        );
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/a/c/x.png")),
            "../c/x.png"
        );
        assert_eq!(relative_path(None, Path::new("/a/x.png")), "/a/x.png");
    }

    #[test]
    fn only_web_links_open_directly() {
        let dir = Path::new("/Users/a/docs");
        assert_eq!(
            classify("https://example.com/x", None),
            LinkTarget::Web("https://example.com/x".into())
        );
        assert_eq!(classify("MAILTO:a@b.c", None), LinkTarget::Web("MAILTO:a@b.c".into()));
        assert_eq!(classify("#intro", None), LinkTarget::Anchor("intro".into()));
        for app in [
            "vscode://file/x",
            "smb://evil/share",
            "x-apple.systempreferences:x",
            "javascript:alert(1)",
            "ftp://x",
        ] {
            assert_eq!(classify(app, Some(dir)), LinkTarget::App, "{app}");
        }
    }

    #[test]
    fn files_never_run() {
        let dir = Path::new("/Users/a/docs");
        for link in [
            "file:///Volumes/share/x.app",
            "setup.command",
            "file:///tmp/a%20b.pkg",
            "../tools/run.sh",
            "/Applications/Calculator.app",
        ] {
            assert!(matches!(classify(link, Some(dir)), LinkTarget::File(_)), "{link}");
        }
        assert_eq!(
            classify("file:///tmp/a%20b.pkg", None),
            LinkTarget::File("/tmp/a b.pkg".into())
        );
        assert_eq!(
            classify("other.md#part", Some(dir)),
            LinkTarget::Document("/Users/a/docs/other.md".into())
        );
        assert_eq!(
            classify("Notes.MarkDown", Some(dir)),
            LinkTarget::Document("/Users/a/docs/Notes.MarkDown".into())
        );
        assert_eq!(classify("other.md", None), LinkTarget::Nothing);
        assert_eq!(classify("file://server/share/x.md", None), LinkTarget::Nothing);
    }

    #[test]
    fn notes_and_pictures_are_looked_up_below_the_document_only() {
        let root = std::env::temp_dir().join(format!("splat-lookup-{}", std::process::id()));
        let docs = root.join("docs");
        let deep = docs.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("bild.png"), b"png").unwrap();
        std::fs::write(deep.join("Notiz.md"), "# Notiz").unwrap();
        std::fs::write(docs.join(".env"), "TOKEN=1").unwrap();
        std::fs::write(root.join("bild.png"), b"png").unwrap();
        let outside = root.join("bild.png");
        assert_eq!(find_note(&docs, "Notiz"), Some(deep.join("Notiz.md")));
        assert_eq!(find_note(&docs, &outside.to_string_lossy()), None);
        assert_eq!(find_note(&docs, "a/b/Notiz.md"), Some(deep.join("Notiz.md")));
        assert_eq!(find_note(&docs, "Notiz#Kapitel|Titel"), Some(deep.join("Notiz.md")));
        assert_eq!(find_note(&docs, "../Notiz"), None);
        assert_eq!(find_note(&docs, " "), None);
        assert_eq!(
            classify_wiki("Notiz", Some(&docs)),
            LinkTarget::Document(deep.join("Notiz.md"))
        );
        assert_eq!(
            classify_wiki("bild.png", Some(&docs)),
            LinkTarget::File(deep.join("bild.png"))
        );
        assert_eq!(classify_wiki("Fehlt", Some(&docs)), LinkTarget::Nothing);
        assert_eq!(classify_wiki("Notiz", None), LinkTarget::Nothing);
        assert_eq!(locate(&docs.join("bild.png"), Some(&docs)), deep.join("bild.png"));
        let elsewhere = root.join("other/bild.png");
        assert_eq!(locate(&elsewhere, Some(&docs)), elsewhere);
        assert_eq!(
            locate(&docs.join("../x/bild.png"), Some(&docs)),
            docs.join("../x/bild.png")
        );
        assert_eq!(locate(&deep.join("bild.png"), None), deep.join("bild.png"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_link_needs_a_destination_only_where_the_mac_can_look_for_it() {
        let dir = Path::new("/Users/a/docs");
        let of = |link: Link| destination(&link, Some(dir));
        assert_eq!(of(Link::Address("https://a.de".into())), None);
        assert_eq!(of(Link::Address("zoommtg://x".into())), None);
        assert_eq!(of(Link::Address("#".into())), None);
        assert_eq!(of(Link::Address("#Teil".into())), Some(Destination::Anchor("Teil".into())));
        assert_eq!(
            of(Link::Address("b/c.md#Teil".into())),
            Some(Destination::Path("/Users/a/docs/b/c.md".into()))
        );
        assert_eq!(
            of(Link::WikiPage(" Notiz ".into())),
            Some(Destination::Note {
                dir: dir.into(),
                name: "Notiz".into()
            })
        );
        assert_eq!(of(Link::WikiPage("#Kapitel".into())), None);
        assert_eq!(of(Link::WikiPage("^block".into())), None);
        assert_eq!(of(Link::Tag("#a".into())), None);
        assert_eq!(destination(&Link::Address("c.md".into()), None), None, "an unsaved document");
        assert_eq!(destination(&Link::WikiPage("Notiz".into()), None), None);
        assert_eq!(
            destination(&Link::Address("/tmp/x.md".into()), None),
            Some(Destination::Path("/tmp/x.md".into()))
        );
    }

    #[test]
    fn the_disk_says_which_destinations_are_missing() {
        let root = std::env::temp_dir().join(format!("splat-missing-{}", std::process::id()));
        let deep = root.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("Notiz.md"), "# Notiz").unwrap();
        std::fs::write(root.join("bild.png"), b"png").unwrap();
        let note = |name: &str| Destination::Note {
            dir: root.clone(),
            name: name.into(),
        };
        assert_eq!(missing_on_disk(&Destination::Path(root.join("bild.png"))), Some(false));
        assert_eq!(missing_on_disk(&Destination::Path(root.join("a"))), Some(false));
        assert_eq!(missing_on_disk(&Destination::Path(root.join("fehlt.md"))), Some(true));
        assert_eq!(missing_on_disk(&note("Notiz")), Some(false));
        assert_eq!(missing_on_disk(&note("Notiz#Kapitel")), Some(false));
        assert_eq!(missing_on_disk(&note("Fehlt")), Some(true));
        assert_eq!(missing_on_disk(&note("../Notiz")), Some(true));
        assert_eq!(missing_on_disk(&Destination::Anchor("x".into())), None);
        let gone = Destination::Note {
            dir: root.join("gibt es nicht"),
            name: "Notiz".into(),
        };
        assert_eq!(missing_on_disk(&gone), None, "a folder that cannot be listed says nothing");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_link_named_like_a_note_leads_nowhere_outside_the_folder() {
        let root = std::env::temp_dir().join(format!("splat-linked-{}", std::process::id()));
        let docs = root.join("docs");
        std::fs::create_dir_all(docs.join("a")).unwrap();
        std::fs::write(root.join("id_ed25519"), "PRIVATE KEY").unwrap();
        std::fs::write(docs.join("a/Notiz.md"), "# Notiz").unwrap();
        std::os::unix::fs::symlink(root.join("id_ed25519"), docs.join("Schlüssel.md")).unwrap();
        std::os::unix::fs::symlink(&root, docs.join("oben")).unwrap();
        std::os::unix::fs::symlink(docs.join("a/Notiz.md"), docs.join("Kurz.md")).unwrap();
        assert_eq!(find_note(&docs, "Schlüssel"), None);
        assert_eq!(find_note(&docs, "oben/id_ed25519"), None);
        assert_eq!(find_note(&docs, "Kurz"), Some(docs.join("Kurz.md")));
        assert_eq!(find_note(&root.join("fehlt"), "Notiz"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
