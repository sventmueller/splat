//! What the preview needs from around the text: the user's choice of typographic punctuation, the code host of the
//! repository the document lies in, its bibliography and the notes it embeds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use objc2_foundation::{NSBundle, NSLocale, NSUserDefaults};

use crate::markdown::typography::Quotes;
use crate::util::ns;

/// User defaults key of the typographic punctuation switch in the View menu.
pub const TYPOGRAPHY_KEY: &str = "TypographicPunctuation";
/// Embedded notes larger than this show as links.
const MAX_NOTE_BYTES: u64 = 512 * 1024;
/// How long a note found by name is not searched for again.
const FOUND_FOR: Duration = Duration::from_secs(5);

/// Everything the preview of one document reads from outside its text, gathered on the main thread.
#[derive(Clone, Default)]
pub struct Env {
    pub quotes: Option<Quotes>,
    pub repository: Option<Repository>,
    pub citations: Option<Arc<HashMap<String, String>>>,
    pub doc_dir: Option<PathBuf>,
}

impl Env {
    pub fn new(doc_dir: Option<PathBuf>, text: &str) -> Env {
        Env {
            quotes: typography_on().then(quotes),
            repository: doc_dir.as_deref().and_then(repository),
            citations: doc_dir.as_deref().and_then(|d| citations(d, text)),
            doc_dir,
        }
    }
}

pub fn typography_on() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&ns(TYPOGRAPHY_KEY))
}

pub fn set_typography(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(on, &ns(TYPOGRAPHY_KEY));
}

/// Quotation marks of the interface language, from the system's locale data.
pub fn quotes() -> Quotes {
    let language = NSBundle::mainBundle()
        .preferredLocalizations()
        .firstObject()
        .map_or_else(|| "de".to_string(), |l| l.to_string());
    let locale = NSLocale::localeWithLocaleIdentifier(&ns(&language));
    let first = |s: objc2::rc::Retained<objc2_foundation::NSString>, fallback: char| {
        s.to_string().chars().next().unwrap_or(fallback)
    };
    Quotes {
        open: first(locale.quotationBeginDelimiter(), '“'),
        close: first(locale.quotationEndDelimiter(), '”'),
        open_single: first(locale.alternateQuotationBeginDelimiter(), '‘'),
        close_single: first(locale.alternateQuotationEndDelimiter(), '’'),
    }
}

/// A repository on GitHub or GitLab, which `@name`, `#123` and `!123` point into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    host: String,
    /// `owner/name`, or deeper with GitLab's groups.
    path: String,
    gitlab: bool,
}

impl Repository {
    /// The web page a reference such as `@anna`, `#12` or `!7` stands for.
    pub fn url(&self, reference: &str) -> Option<String> {
        let (kind, name) = reference.split_at_checked(1)?;
        let base = format!("https://{}", self.host);
        match (kind, self.gitlab) {
            ("@", _) => Some(format!("{base}/{name}")),
            ("#", false) => Some(format!("{base}/{}/issues/{name}", self.path)),
            ("#", true) => Some(format!("{base}/{}/-/issues/{name}", self.path)),
            ("!", true) => Some(format!("{base}/{}/-/merge_requests/{name}", self.path)),
            _ => None,
        }
    }
}

static REPOSITORIES: Mutex<Option<HashMap<PathBuf, Option<Repository>>>> = Mutex::new(None);

/// The repository the folder lies in, from the `origin` remote of its Git configuration.
pub fn repository(dir: &Path) -> Option<Repository> {
    if let Ok(mut cache) = REPOSITORIES.lock()
        && let Some(found) = cache.get_or_insert_with(HashMap::new).get(dir)
    {
        return found.clone();
    }
    let found = find_repository(dir);
    if let Ok(mut cache) = REPOSITORIES.lock() {
        cache
            .get_or_insert_with(HashMap::new)
            .insert(dir.to_path_buf(), found.clone());
    }
    found
}

fn find_repository(dir: &Path) -> Option<Repository> {
    let git = dir.ancestors().map(|d| d.join(".git")).find(|g| g.exists())?;
    let git_dir = if git.is_file() {
        // A worktree or submodule names its real Git folder in a file.
        let text = std::fs::read_to_string(&git).ok()?;
        let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        let target = if target.is_absolute() {
            target
        } else {
            git.parent()?.join(target)
        };
        let common = target.join("commondir");
        match std::fs::read_to_string(&common) {
            Ok(rel) => target.join(rel.trim()),
            Err(_) => target,
        }
    } else {
        git
    };
    let config = std::fs::read_to_string(git_dir.join("config")).ok()?;
    remote_repository(&config)
}

/// The repository the `origin` remote of a Git configuration points to.
pub fn remote_repository(config: &str) -> Option<Repository> {
    let mut in_origin = false;
    let mut url = None;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
        } else if in_origin
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            url = Some(value.trim().to_string());
        }
    }
    let url = url?;
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .unwrap_or(&url);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
    let (host, path) = rest.split_once([':', '/'])?;
    // `ssh://host:2222/group/repo`: the port is no part of the web address.
    let path = match path.split_once('/') {
        Some((port, tail)) if port.chars().all(|c| c.is_ascii_digit()) => tail,
        _ => path,
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git").to_string();
    let host = host.to_ascii_lowercase();
    let gitlab = host.contains("gitlab");
    (host == "github.com" || gitlab).then_some(Repository { host, path, gitlab })
}

type Bibliography = (PathBuf, SystemTime, Arc<HashMap<String, String>>);
static BIBLIOGRAPHIES: Mutex<Vec<Bibliography>> = Mutex::new(Vec::new());

/// How each entry of the document's BibTeX bibliography reads in a citation, such as `Doe 2020`. The file is the
/// one front matter names as `bibliography:`, else `references.bib`, `literatur.bib` or `bibliography.bib` beside it.
pub fn citations(dir: &Path, text: &str) -> Option<Arc<HashMap<String, String>>> {
    let named = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---").next())
        .and_then(|front| {
            front.lines().find_map(|l| {
                let value = l.strip_prefix("bibliography:")?.trim().trim_matches(['"', '\'']);
                (!value.is_empty()).then(|| dir.join(value))
            })
        });
    let path = named
        .into_iter()
        .chain(["references.bib", "literatur.bib", "bibliography.bib"].map(|n| dir.join(n)))
        .find(|p| p.is_file())?;
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let mut cache = BIBLIOGRAPHIES.lock().ok()?;
    if let Some((_, _, entries)) = cache.iter().find(|(p, m, _)| *p == path && *m == modified) {
        return Some(entries.clone());
    }
    let entries = Arc::new(parse_bibtex(&std::fs::read_to_string(&path).ok()?));
    cache.retain(|(p, _, _)| *p != path);
    cache.push((path, modified, entries.clone()));
    Some(entries)
}

/// Whether text ending in `before` is inside a braced or quoted value, where a field name is only a word.
fn inside_value(before: &str) -> bool {
    let (mut depth, mut quoted) = (0, false);
    for c in before.chars() {
        match c {
            '{' if !quoted => depth += 1,
            '}' if !quoted && depth > 0 => depth -= 1,
            '"' if depth == 0 => quoted = !quoted,
            _ => {}
        }
    }
    depth > 0 || quoted
}

/// BibTeX entries as `key → "Author Year"`, from their author or editor and year or date fields.
pub fn parse_bibtex(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for entry in text.split('@').skip(1) {
        let Some(open) = entry.find(['{', '(']) else { continue };
        let body = &entry[open + 1..];
        let Some((key, fields)) = body.split_once(',') else {
            continue;
        };
        let field = |name: &str| -> Option<String> {
            let lower = fields.to_ascii_lowercase();
            let mut from = 0;
            while let Some(at) = lower[from..].find(name) {
                let start = from + at;
                from = start + name.len();
                let before_ok = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
                let after = lower[start + name.len()..].trim_start();
                if !before_ok || !after.starts_with('=') || inside_value(&fields[..start]) {
                    continue;
                }
                let value = fields[fields.len() - after.len() + 1..].trim_start();
                let (close, rest) = match value.chars().next()? {
                    '{' => ('}', &value[1..]),
                    '"' => ('"', &value[1..]),
                    _ => (',', value),
                };
                let mut depth = 0;
                let mut end = rest.len();
                for (i, c) in rest.char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' if depth > 0 => depth -= 1,
                        c if c == close && depth == 0 => {
                            end = i;
                            break;
                        }
                        '\n' if close == ',' => {
                            end = i;
                            break;
                        }
                        _ => {}
                    }
                }
                return Some(rest[..end].replace(['{', '}'], "").trim().to_string());
            }
            None
        };
        let people = field("author").or_else(|| field("editor")).unwrap_or_default();
        let names: Vec<String> = people
            .split(" and ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| match p.split_once(',') {
                Some((last, _)) => last.trim().to_string(),
                None => p.rsplit(' ').next().unwrap_or(p).to_string(),
            })
            .collect();
        let year = field("year").or_else(|| field("date").map(|d| d.chars().take(4).collect()));
        let who = match names.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [a, b] => format!("{a} & {b}"),
            [a, ..] => format!("{a} et al."),
        };
        let shown = [who, year.unwrap_or_default()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !shown.is_empty() {
            out.insert(key.trim().to_string(), shown);
        }
    }
    out
}

type Found = (PathBuf, String, Instant, Option<PathBuf>);
static FOUND: Mutex<Vec<Found>> = Mutex::new(Vec::new());
type Note = (PathBuf, SystemTime, Arc<str>);
static NOTES: Mutex<Vec<Note>> = Mutex::new(Vec::new());

/// Files an embed shows as note; any other file, such as a key or `.env`, stays out of the preview and its exports.
const NOTE_EXTENSIONS: [&str; 4] = ["md", "markdown", "mdown", "mkd"];

/// The text of the note `![[target]]` names and the folder it lies in; a note too large to show gives `None`.
pub fn embedded_note(dir: &Path, target: &str) -> Option<(String, String)> {
    let found = {
        let mut cache = FOUND.lock().ok()?;
        cache.retain(|(_, _, at, _)| at.elapsed() < FOUND_FOR);
        match cache.iter().find(|(d, t, _, _)| d == dir && t == target) {
            Some((_, _, _, path)) => path.clone(),
            None => {
                let path = super::links::find_note(dir, target);
                cache.push((dir.to_path_buf(), target.to_string(), Instant::now(), path.clone()));
                path
            }
        }
    }?;
    let extension = found.extension()?.to_str()?.to_ascii_lowercase();
    if !NOTE_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let meta = std::fs::metadata(&found).ok()?;
    if meta.len() > MAX_NOTE_BYTES {
        return None;
    }
    let modified = meta.modified().ok()?;
    let folder = found.parent()?.to_string_lossy().into_owned();
    let mut notes = NOTES.lock().ok()?;
    if let Some((_, _, text)) = notes.iter().find(|(p, m, _)| *p == found && *m == modified) {
        return Some((text.to_string(), folder));
    }
    let text: Arc<str> = std::fs::read_to_string(&found).ok()?.into();
    notes.retain(|(p, _, _)| *p != found);
    notes.push((found, modified, text.clone()));
    Some((text.to_string(), folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_lead_to_their_code_host() {
        let config = |url: &str| format!("[core]\n\tbare = false\n[remote \"origin\"]\n\turl = {url}\n");
        let github = remote_repository(&config("git@github.com:anna/splat.git")).unwrap();
        assert_eq!(
            github.url("#12").as_deref(),
            Some("https://github.com/anna/splat/issues/12")
        );
        assert_eq!(github.url("@bob").as_deref(), Some("https://github.com/bob"));
        assert_eq!(github.url("!3"), None);
        let gitlab = remote_repository(&config("https://gitlab.example.com/team/sub/app.git")).unwrap();
        assert_eq!(
            gitlab.url("!7").as_deref(),
            Some("https://gitlab.example.com/team/sub/app/-/merge_requests/7")
        );
        let port = remote_repository(&config("ssh://git@gitlab.example.com:2222/team/app.git")).unwrap();
        assert_eq!(
            port.url("#1").as_deref(),
            Some("https://gitlab.example.com/team/app/-/issues/1")
        );
        assert_eq!(remote_repository(&config("https://example.com/x/y.git")), None);
    }

    #[test]
    fn worktrees_and_bibliographies_are_found_beside_the_document() {
        let root = std::env::temp_dir().join(format!("splat-env-{}", std::process::id()));
        let main_git = root.join("main/.git");
        let worktree = root.join("worktree");
        std::fs::create_dir_all(main_git.join("worktrees/wt")).unwrap();
        std::fs::create_dir_all(worktree.join("docs")).unwrap();
        std::fs::write(
            main_git.join("config"),
            "[remote \"origin\"]\n\turl = git@github.com:anna/splat.git\n",
        )
        .unwrap();
        std::fs::write(main_git.join("worktrees/wt/commondir"), "../..\n").unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
        let found = repository(&worktree.join("docs")).unwrap();
        assert_eq!(
            found.url("#1").as_deref(),
            Some("https://github.com/anna/splat/issues/1")
        );
        assert_eq!(repository(&worktree.join("docs")), Some(found));

        let docs = worktree.join("docs");
        std::fs::write(docs.join("quellen.bib"), "@book{k, author = {Kant, I.}, year = 1781}").unwrap();
        std::fs::write(docs.join("references.bib"), "@book{r, author = {Roe, R.}, year = 2000}").unwrap();
        let named = citations(&docs, "---\nbibliography: \"quellen.bib\"\n---\n# Text").unwrap();
        assert_eq!(named["k"], "Kant 1781");
        assert_eq!(citations(&docs, "# Ohne Angabe").unwrap()["r"], "Roe 2000");
        assert!(Arc::ptr_eq(
            &named,
            &citations(&docs, "---\nbibliography: quellen.bib\n---\n").unwrap()
        ));

        std::fs::write(docs.join("Notiz.md"), "# Notiz").unwrap();
        std::fs::write(docs.join("Groß.md"), "x".repeat(MAX_NOTE_BYTES as usize + 1)).unwrap();
        let (text, folder) = embedded_note(&docs, "Notiz").unwrap();
        assert_eq!((text.as_str(), folder.as_str()), ("# Notiz", docs.to_str().unwrap()));
        assert_eq!(embedded_note(&docs, "Notiz").unwrap().0, "# Notiz");
        assert!(embedded_note(&docs, "Groß").is_none());
        assert!(embedded_note(&docs, "Fehlt").is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn typographic_quotes_follow_the_setting_and_the_language() {
        let before = typography_on();
        set_typography(!before);
        assert_eq!(typography_on(), !before);
        set_typography(before);
        let q = quotes();
        assert!(q.open != q.close || q.open == '"', "{q:?}");
    }

    #[test]
    fn bibtex_entries_read_as_author_and_year() {
        let entries = parse_bibtex(
            "@article{doe2020,\n  author = {Doe, Jane and Roe, Rick},\n  year = 2020,\n  title = {A {B} c}\n}\n\
             @book{solo, author = \"Max Mustermann\", date = {2019-04-01}}\n\
             @misc{many, author = {A, a and B, b and C, c}, year = {2021}}",
        );
        assert_eq!(entries["doe2020"], "Doe & Roe 2020");
        assert_eq!(entries["solo"], "Mustermann 2019");
        assert_eq!(entries["many"], "A et al. 2021");
        let odd = parse_bibtex(
            "@comment no braces\n@misc{nofields}\n@misc{bare, author = Plain,\n year = 1999\n}\n\
             @misc{nested, title = {author = no}, editor = {Ed, E.}, year = {2001}}",
        );
        assert_eq!(odd["bare"], "Plain 1999");
        assert_eq!(odd["nested"], "Ed 2001");
        assert!(!odd.contains_key("nofields"));
    }
}
