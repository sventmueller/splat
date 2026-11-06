//! What a click on a link, a task checkbox or a footnote marker in the preview means, and where opening a link leads.

use std::path::{Path, PathBuf};

use crate::links::{self, LinkTarget};
use crate::preview::layout::Link;

/// What a click on a link in the preview means.
#[derive(Debug, PartialEq)]
pub enum Click<'a> {
    /// The checkbox of a task changes.
    ToggleTask,
    /// A folding alert box opens or closes.
    ToggleFold,
    /// The definition of the footnote of this name shows.
    ShowFootnote(&'a str),
    /// The metadata form opens.
    EditMetadata,
    /// With ⌘ a link leads somewhere, so that a click to place the caret never does.
    Open(&'a Link),
    /// The caret goes where the click was.
    PlaceCaret,
}

/// What a click on `link` means, with the command key held or not.
pub fn click_meaning(link: &Link, command_held: bool) -> Click<'_> {
    match link {
        Link::Task => Click::ToggleTask,
        Link::Fold => Click::ToggleFold,
        Link::Footnote(name) => Click::ShowFootnote(name),
        Link::Metadata => Click::EditMetadata,
        Link::Address(_) | Link::WikiPage(_) | Link::Tag(_) | Link::Reference(_) if command_held => Click::Open(link),
        Link::Address(_) | Link::WikiPage(_) | Link::Tag(_) | Link::Reference(_) => Click::PlaceCaret,
    }
}

/// What opening a link from the document leads to. Files are only shown in the Finder and other apps' URLs need a
/// confirmation, so a link can never start a program or script.
#[derive(Debug, PartialEq)]
pub enum Opening {
    /// A search for the tag, in the source and in the folder.
    Find(String),
    /// The page of the repository that `@name`, `#12` or `!7` stands for.
    Reference(String),
    /// The note a wiki link names, which has to be searched for.
    WikiPage(String),
    /// The heading with this anchor in the document itself.
    Heading(String),
    /// The browser or mail app.
    Web(String),
    /// Another app, once the user confirms it.
    AskFirst(String),
    /// Nothing may open; the user hears so.
    Refuse,
    /// A file or folder, once a look at the disk says what lies there.
    LookAt(LinkTarget),
    /// The link does nothing when opened.
    Nothing,
}

/// Where opening `link` of a document in `doc_dir` leads.
pub fn opening(link: &Link, doc_dir: Option<&Path>) -> Opening {
    match link {
        Link::Tag(tag) => Opening::Find(tag.to_string()),
        Link::Reference(reference) => Opening::Reference(reference.to_string()),
        Link::WikiPage(name) => Opening::WikiPage(name.to_string()),
        Link::Address(address) => match links::classify(address, doc_dir) {
            LinkTarget::Anchor(anchor) => Opening::Heading(anchor),
            LinkTarget::Web(address) => Opening::Web(address),
            LinkTarget::App => Opening::AskFirst(address.to_string()),
            LinkTarget::Nothing => Opening::Refuse,
            found => Opening::LookAt(found),
        },
        Link::Footnote(_) | Link::Fold | Link::Task | Link::Metadata => Opening::Nothing,
    }
}

/// Where a link to a file goes, by what lies there: whether it is a Markdown file, or `None` for nothing at all. It reads
/// the disk, so it belongs on the thread of the volume.
pub fn found_place(target: LinkTarget) -> Option<(PathBuf, bool)> {
    match target {
        LinkTarget::Document(path) if path.is_file() => Some((path, true)),
        LinkTarget::Document(path) | LinkTarget::File(path) if path.exists() => Some((path, false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_on_what_belongs_to_the_document_acts_without_any_key() {
        for command_held in [false, true] {
            assert_eq!(click_meaning(&Link::Task, command_held), Click::ToggleTask);
            assert_eq!(click_meaning(&Link::Fold, command_held), Click::ToggleFold);
            assert_eq!(click_meaning(&Link::Metadata, command_held), Click::EditMetadata);
            assert_eq!(
                click_meaning(&Link::Footnote("a".into()), command_held),
                Click::ShowFootnote("a")
            );
        }
    }

    #[test]
    fn a_link_that_leads_away_needs_the_command_key() {
        for link in [
            Link::Address("https://a.de".into()),
            Link::WikiPage("Notiz".into()),
            Link::Tag("#a".into()),
            Link::Reference("#1".into()),
        ] {
            assert_eq!(click_meaning(&link, false), Click::PlaceCaret, "{link:?}");
            assert_eq!(click_meaning(&link, true), Click::Open(&link), "{link:?}");
        }
    }

    #[test]
    fn a_link_opens_by_what_it_names_and_never_starts_anything_unasked() {
        let dir = Path::new("/nicht/da");
        let opens = |address: &str| opening(&Link::Address(address.into()), Some(dir));
        assert_eq!(opens("#kapitel"), Opening::Heading("kapitel".into()));
        assert_eq!(opens("https://a.de/b"), Opening::Web("https://a.de/b".into()));
        assert_eq!(opens("mailto:a@b.de"), Opening::Web("mailto:a@b.de".into()));
        assert_eq!(opens("zoommtg://x"), Opening::AskFirst("zoommtg://x".into()));
        assert_eq!(opens("file://server/a"), Opening::Refuse);
        assert_eq!(
            opens("a.md"),
            Opening::LookAt(LinkTarget::Document("/nicht/da/a.md".into()))
        );
        assert_eq!(
            opens("Programm.app"),
            Opening::LookAt(LinkTarget::File("/nicht/da/Programm.app".into()))
        );
        assert_eq!(
            opening(&Link::Address("a.md".into()), None),
            Opening::Refuse,
            "an unsaved document"
        );
        assert_eq!(opening(&Link::Tag("#a".into()), None), Opening::Find("#a".into()));
        assert_eq!(
            opening(&Link::Reference("#1".into()), None),
            Opening::Reference("#1".into())
        );
        assert_eq!(
            opening(&Link::WikiPage("Notiz".into()), None),
            Opening::WikiPage("Notiz".into())
        );
        assert_eq!(opening(&Link::Task, None), Opening::Nothing);
    }

    #[test]
    fn what_a_link_finds_on_disk_decides_where_it_goes() {
        let dir = std::env::temp_dir().join(format!("splat-navigation-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            found_place(LinkTarget::Document(file.clone())),
            Some((file.clone(), true))
        );
        assert_eq!(
            found_place(LinkTarget::Document(dir.clone())),
            Some((dir.clone(), false)),
            "a folder named like a document is only shown"
        );
        assert_eq!(found_place(LinkTarget::File(file.clone())), Some((file.clone(), false)));
        assert_eq!(found_place(LinkTarget::Document(dir.join("fehlt.md"))), None);
        assert_eq!(found_place(LinkTarget::File(dir.join("fehlt"))), None);
        assert_eq!(found_place(LinkTarget::Nothing), None);
        assert_eq!(found_place(LinkTarget::App), None);
        assert_eq!(found_place(LinkTarget::Web("https://a.de".into())), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
