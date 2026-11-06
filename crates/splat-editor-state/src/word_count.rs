//! What the word count in the corner of a window says, and whether it shows.

use crate::i18n::tf;
use crate::markdown::counts::Counts;

/// Whether windows show the count, by whether the user chose to hide it, if ever: it stays hidden until someone asks for it.
pub fn shows(hides: Option<bool>) -> bool {
    hides == Some(false)
}

/// What the corner says. Labels in front of the figures spare the plural forms of 35 languages.
pub fn summary(of_text: Counts, of_selection: Option<Counts>, number: impl Fn(usize) -> String) -> String {
    match of_selection {
        Some(selected) => tf(
            "Auswahl – Wörter: {} von {} · Zeichen: {}",
            &[
                &number(selected.words),
                &number(of_text.words),
                &number(selected.characters),
            ],
        ),
        None => tf(
            "Wörter: {} · Zeichen: {} · Lesezeit: {} Min.",
            &[
                &number(of_text.words),
                &number(of_text.characters),
                &number(of_text.reading_minutes()),
            ],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(words: usize, characters: usize) -> Counts {
        Counts {
            words,
            characters,
            signs: 0,
        }
    }

    #[test]
    fn the_count_shows_only_once_someone_asked_for_it() {
        assert!(!shows(None));
        assert!(!shows(Some(true)));
        assert!(shows(Some(false)));
    }

    #[test]
    fn the_corner_tells_of_the_selection_as_long_as_there_is_one() {
        let plain = |n: usize| n.to_string();
        let whole = summary(counts(460, 2000), None, plain);
        assert!(
            whole.contains("460") && whole.contains("2000") && whole.contains('2'),
            "{whole}"
        );
        let part = summary(counts(460, 2000), Some(counts(3, 11)), plain);
        assert!(
            part.contains('3') && part.contains("460") && part.contains("11"),
            "{part}"
        );
        assert!(!part.contains("2000"), "{part}");
    }
}
