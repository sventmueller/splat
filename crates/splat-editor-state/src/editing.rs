//! What an edit in the preview changed, and what dropping files and addresses on the source inserts.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::links::relative_path;
use crate::markdown::commands;
use crate::markdown::text::{units, utf16_len};
use crate::preview::layout::Layout;
use crate::preview::map::Shown;
use crate::sync::utf16_diff;

/// A change the preview announced before applying it, as range of its old text and replacement.
#[derive(Debug, PartialEq, Eq)]
pub enum Announced {
    /// One range of the old text and what replaces it.
    Edit(Range<Shown>, String),
    /// Several changes or one without text: found by comparing the texts afterwards.
    Unknown,
}

/// The change the preview view announced, as the range of its old text and what replaced it, if that fits the lengths.
pub fn fitting_announcement(
    announced: Option<Announced>,
    shown_len: usize,
    new_len: usize,
) -> Option<(Range<Shown>, String)> {
    match announced {
        Some(Announced::Edit(range, text))
            if range.end.get() <= shown_len && new_len + units(&range) == shown_len + utf16_len(&text) =>
        {
            Some((range, text))
        }
        _ => None,
    }
}

/// The change the user made in the preview found by comparing its old and new text around the caret.
pub fn compared_change(layout: &Layout, new_text: &str, caret: usize) -> (Range<Shown>, String) {
    let new: Vec<u16> = new_text.encode_utf16().collect();
    let old: Vec<u16> = preview_string(layout).encode_utf16().collect();
    let (old_range, new_range) = utf16_diff(&old, &new, caret);
    (
        Shown::new(old_range.start)..Shown::new(old_range.end),
        String::from_utf16_lossy(&new[new_range]),
    )
}

/// The text the preview shows for `layout`.
pub fn preview_string(layout: &Layout) -> String {
    let mut s = String::new();
    for para in layout.blocks.iter().flat_map(|b| &b.paras) {
        for run in &para.runs {
            s.push_str(&run.text);
        }
        s.push('\n');
    }
    s
}

/// What was dropped on the source.
#[derive(Debug, PartialEq, Eq)]
pub enum Dropped {
    /// A file or folder.
    File(PathBuf),
    /// An address of something elsewhere.
    Address(String),
}

/// Markdown for files and addresses dropped on the source, one per line: pictures as images, everything else as
/// links, files relative to `dir`.
pub fn dropped_markdown(dropped: &[Dropped], dir: Option<&Path>) -> String {
    const PICTURES: [&str; 10] = ["png", "jpg", "jpeg", "gif", "svg", "webp", "heic", "tif", "tiff", "bmp"];
    let lines: Vec<String> = dropped
        .iter()
        .map(|item| match item {
            Dropped::File(path) => {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dest = commands::link_destination(&relative_path(dir, path));
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if PICTURES.contains(&ext.as_str()) {
                    format!("![{name}]({dest})")
                } else {
                    format!("[{name}]({dest})")
                }
            }
            Dropped::Address(s) => format!("[{s}]({})", commands::link_destination(s)),
        })
        .collect();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::reader::parse;
    use crate::preview::layout::{NoImages, layout};

    #[test]
    fn an_announced_preview_change_counts_only_when_it_fits_the_lengths() {
        let text = "Ein Wort\n";
        let l = layout(&parse(text), &NoImages, None);
        let shown = utf16_len(&preview_string(&l));
        let fits = Some(Announced::Edit(Shown::new(3)..Shown::new(3), "x".into()));
        assert_eq!(
            fitting_announcement(fits, shown, shown + 1),
            Some((Shown::new(3)..Shown::new(3), "x".into()))
        );
        let misfit = Some(Announced::Edit(Shown::new(0)..Shown::new(0), "yy".into()));
        assert_eq!(fitting_announcement(misfit, shown, shown + 1), None);
        assert_eq!(fitting_announcement(Some(Announced::Unknown), shown, shown - 4), None);
        assert_eq!(
            compared_change(&l, "Einx Wort\n", 4),
            (Shown::new(3)..Shown::new(3), "x".into())
        );
        assert_eq!(
            compared_change(&l, "Ein \n", 4),
            (Shown::new(4)..Shown::new(8), String::new())
        );
    }

    #[test]
    fn dropped_files_become_images_or_links_relative_to_the_document() {
        let dropped = [
            Dropped::File("/Users/a/Notizen/Bilder/Foto 1.PNG".into()),
            Dropped::File("/Users/a/Bericht.pdf".into()),
            Dropped::Address("https://example.com/(x)".into()),
        ];
        assert_eq!(
            dropped_markdown(&dropped, Some(Path::new("/Users/a/Notizen"))),
            "![Foto 1](<Bilder/Foto 1.PNG>)\n[Bericht](../Bericht.pdf)\n[https://example.com/(x)](<https://example.com/(x)>)"
        );
        assert_eq!(
            dropped_markdown(&dropped[1..2], None),
            "[Bericht](/Users/a/Bericht.pdf)"
        );
    }
}
