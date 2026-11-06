//! Random folders of notes, and a note too large to matter.

use super::*;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % n.max(1) as u64).unwrap_or(0)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// What each link of `text` leads to, and whether it is a wiki link; a link in HTML stays out, since it stays as it
/// is.
fn leads_to(tree: &Tree, text: &str, file: &Path) -> Vec<(bool, Option<PathBuf>)> {
    let resolver = Resolver::new(tree);
    let doc = parse(text);
    links_of(&doc)
        .iter()
        .filter(|link| !text[link.whole.clone()].starts_with('<'))
        .map(|link| match &link.says {
            LinkTarget::Wiki { target, .. } => (true, resolver.wiki(target, file)),
            LinkTarget::Address { url, .. } => (
                false,
                resolver
                    .address(url, file)
                    .filter(|path| tree.notes().any(|note| note.path == *path)),
            ),
        })
        .collect()
}

/// A link to the note at `path` from the folder `dir`, in one of the ways of writing it.
fn random_link(rng: &mut Rng, path: &str, dir: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.trim_end_matches(".md");
    let ups = dir.matches('/').count() + usize::from(!dir.is_empty());
    let relative = format!("{}{path}", "../".repeat(ups));
    let encoded = relative
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('ä', "%C3%A4");
    let heading = rng.pick(&["", "#Überschrift", "#h"]);
    let title = rng.pick(&["", " \"Titel\""]);
    match rng.below(10) {
        0 => format!("[[{stem}]]"),
        1 => format!("[[{stem}|Titel]]"),
        2 => format!("[[{stem}{heading}]]"),
        3 => format!("![[{stem}]]"),
        4 => format!("[[{name}{heading}]]"),
        5 => format!("[[{path}]]"),
        6 => format!("[t]({encoded}{heading}{title})"),
        7 => format!(
            "[t](<{}{heading}>{title})",
            relative.replace('%', "%25").replace('#', "%23")
        ),
        8 => format!(
            "[r{}][d{}]\n\n[d{0}]: {encoded}{heading}{title}\n",
            rng.0 % 1000,
            rng.0 % 1000
        ),
        _ => format!("`[[{stem}]]` %%[[{stem}]]%%"),
    }
}

const NOTES: [&str; 12] = [
    "Old.md",
    "other.md",
    "My Note.md",
    "Müller.md",
    "100%.md",
    "C#.md",
    "sub/Old.md",
    "sub/x.md",
    "sub/deep/Old.md",
    "sub/deep/y.md",
    "z/New.md",
    "z/w/täglich.md",
];

/// Writes the notes with links between them in all ways of writing one, and gives their texts.
fn random_notes(rng: &mut Rng, folder: &Folder) -> HashMap<PathBuf, String> {
    let mut texts = HashMap::new();
    for note in NOTES {
        let dir = note.rsplit_once('/').map_or("", |(dir, _)| dir);
        let mut text = String::from(rng.pick(&["", "\u{FEFF}", "---\nt: 1\n---\n\n"]));
        let eol = rng.pick(&["\n", "\r\n"]);
        for _ in 0..rng.below(8) {
            let target = rng.pick(&NOTES);
            text.push_str(&random_link(rng, target, dir));
            text.push_str(rng.pick(&[" ", eol, ""]));
            text.push_str(rng.pick(&["Text ", "`code` ", "<a href=\"Old.md\">html</a> ", ""]));
        }
        folder.note(note, &text);
        texts.insert(folder.path().join(note), text);
    }
    texts
}

/// `old` with the replacements of `rewrite` made, none of which touches anything but its own range.
fn with_only_names_changed(old: &str, rewrite: &Rewrite) -> String {
    let mut sorted = rewrite.replacements.clone();
    sorted.sort_by_key(|r| r.range.start);
    assert_eq!(sorted, rewrite.replacements, "in the order of the text");
    let new = applied(old, &rewrite.replacements);
    let (mut at, mut at_new) = (0, 0);
    for replacement in &rewrite.replacements {
        let same = replacement.range.start - at;
        assert_eq!(
            old[at..replacement.range.start],
            new[at_new..at_new + same],
            "bytes outside"
        );
        at = replacement.range.end;
        at_new += same;
        assert!(new[at_new..].starts_with(&replacement.text));
        at_new += replacement.text.len();
    }
    assert_eq!(old[at..], new[at_new..], "bytes outside");
    new
}

#[test]
fn after_a_rename_every_link_that_led_to_the_note_leads_to_its_new_name_and_no_other_changes() {
    const NEW_NAMES: [&str; 9] = [
        "New.md",
        "Neu mit Leerzeichen.md",
        "Größe.md",
        "50%.md",
        "a#b.md",
        "q?.md",
        "(paren).md",
        "Old.markdown",
        "NEW.MD",
    ];
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for round in 0..60 {
        let folder = Folder::new("property");
        let root = folder.path().to_path_buf();
        let mut texts = random_notes(&mut rng, &folder);
        let from_note = rng.pick(&NOTES);
        let from = root.join(from_note);
        let to = from.with_file_name(rng.pick(&NEW_NAMES));
        if to.exists() {
            continue;
        }
        let tree = scan(&root, &AtomicBool::new(false));
        let before: HashMap<PathBuf, _> = texts
            .iter()
            .map(|(path, text)| (path.clone(), leads_to(&tree, text, path)))
            .collect();
        let rewrites = renames(&tree, &from, &to, &AtomicBool::new(false), &mut |path| {
            texts.get(path).cloned()
        });
        for rewrite in &rewrites {
            let new = with_only_names_changed(&texts[&rewrite.path], rewrite);
            std::fs::write(&rewrite.path, &new).unwrap();
            texts.insert(rewrite.path.clone(), new);
        }
        std::fs::rename(&from, &to).unwrap();
        if let Some(text) = texts.remove(&from) {
            texts.insert(to.clone(), text);
        }
        let later = scan(&root, &AtomicBool::new(false));
        let wiki_holds = wiki_can_hold(to.file_stem().unwrap().to_str().unwrap());
        for (path, text) in &texts {
            let was = &before[if *path == to { &from } else { path }];
            let now = leads_to(&later, text, path);
            let what = || format!("round {round}: {from_note} to {to:?} in {path:?}: {text:?}\n{was:?}\n{now:?}");
            assert_eq!(was.len(), now.len(), "{}", what());
            for ((wiki, was), (_, now)) in was.iter().zip(&now) {
                match was {
                    Some(note) if *note == from && *wiki && !wiki_holds => {
                        assert_eq!(*now, None, "{}", what());
                    }
                    Some(note) if *note == from => assert_eq!(now.as_deref(), Some(&*to), "{}", what()),
                    Some(_) => assert_eq!(now, was, "{}", what()),
                    None => assert!(now.is_none() || now.as_deref() == Some(&*to), "{}", what()),
                }
            }
        }
    }
}

#[test]
fn links_inside_a_note_too_large_to_matter_are_found_in_linear_time() {
    let tree = tree_of(&["from.md", "Old.md"]);
    let run = |count: usize| {
        let text = "[a](Old.md) [[Old]] x\n\n".repeat(count);
        let started = std::time::Instant::now();
        let rewrites = renames(
            &tree,
            Path::new("/v/Old.md"),
            Path::new("/v/New.md"),
            &AtomicBool::new(false),
            &mut |path| (path == Path::new("/v/from.md")).then(|| text.clone()),
        );
        assert_eq!(rewrites[0].replacements.len(), count * 2);
        started.elapsed()
    };
    let (small, large) = (run(5_000), run(20_000));
    assert!(
        large < small * 8 + std::time::Duration::from_millis(200),
        "{small:?}, then {large:?}"
    );
}
