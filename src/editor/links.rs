//! Where a link in a document may lead when it is opened. A document can carry links to apps, scripts and
//! installers; none of them may start from a click.

use std::path::{Path, PathBuf};

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
                && let Some(home) = std::env::var_os("HOME")
            {
                local(PathBuf::from(home).join(rest))
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
    let candidates = |base: &Path| {
        let plain = base.join(name);
        let with_md = base.join(format!("{name}.md"));
        [plain, with_md].into_iter().find(|p| p.is_file())
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
pub(super) fn strip_suffixes(target: &str) -> &str {
    let end = target.find(['?', '#']).unwrap_or(target.len());
    &target[..end]
}

/// Decodes `%XX` escapes; invalid escapes and bytes stay as they are.
pub fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        (b as char).to_digit(16).and_then(|d| u8::try_from(d).ok())
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).and_then(|b| hex(*b)),
                bytes.get(i + 2).and_then(|b| hex(*b)),
            )
        {
            out.push(hi * 16 + lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Relative path from `dir` to `target`, falling back to the absolute path.
pub(super) fn relative_path(dir: Option<&Path>, target: &Path) -> String {
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
    fn embeds_and_pictures_are_looked_up_below_the_document_only() {
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
        assert!(super::super::env::embedded_note(&docs, "Notiz").is_some());
        assert!(super::super::env::embedded_note(&docs, ".env").is_none());
        let images = super::super::images::locate;
        assert_eq!(images(&docs.join("bild.png"), Some(&docs)), deep.join("bild.png"));
        let elsewhere = root.join("other/bild.png");
        assert_eq!(images(&elsewhere, Some(&docs)), elsewhere);
        assert_eq!(
            images(&docs.join("../x/bild.png"), Some(&docs)),
            docs.join("../x/bild.png")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn percent_escapes_decode_on_byte_level() {
        assert_eq!(percent_decode("a%20b%2"), "a b%2");
        assert_eq!(percent_decode("%€.png"), "%€.png");
        assert_eq!(percent_decode("%C3%A4"), "ä");
        assert_eq!(percent_decode("%zz%"), "%zz%");
    }
}
