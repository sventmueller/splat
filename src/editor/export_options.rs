//! What PDF, printing and Word exports add to the text: a title page, a header and a footer with page numbers, and a Word
//! template. The options hold for every document, as a company's look does; what the title page says comes from the
//! document's metadata.

use std::path::PathBuf;

use objc2_foundation::NSUserDefaults;

use crate::markdown::doc::Doc;
use crate::markdown::front_matter::{self, Value};
use crate::util::ns;

const PAGE_NUMBERS_KEY: &str = "ExportPageNumbers";
const HEADER_KEY: &str = "ExportHeader";
const FOOTER_KEY: &str = "ExportFooter";
const TITLE_PAGE_KEY: &str = "ExportTitlePage";
const WORD_TEMPLATE_KEY: &str = "ExportWordTemplate";

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) struct ExportOptions {
    pub(super) page_numbers: bool,
    pub(super) header: String,
    pub(super) footer: String,
    pub(super) title_page: bool,
    pub(super) word_template: Option<PathBuf>,
}

impl ExportOptions {
    pub(super) fn load() -> ExportOptions {
        let defaults = NSUserDefaults::standardUserDefaults();
        let text = |key: &str| {
            defaults
                .stringForKey(&ns(key))
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let template = text(WORD_TEMPLATE_KEY);
        ExportOptions {
            page_numbers: defaults.boolForKey(&ns(PAGE_NUMBERS_KEY)),
            header: text(HEADER_KEY),
            footer: text(FOOTER_KEY),
            title_page: defaults.boolForKey(&ns(TITLE_PAGE_KEY)),
            word_template: (!template.is_empty()).then(|| PathBuf::from(template)),
        }
    }

    pub(super) fn save(&self) {
        let defaults = NSUserDefaults::standardUserDefaults();
        defaults.setBool_forKey(self.page_numbers, &ns(PAGE_NUMBERS_KEY));
        defaults.setBool_forKey(self.title_page, &ns(TITLE_PAGE_KEY));
        let template = self
            .word_template
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        for (key, value) in [
            (HEADER_KEY, self.header.as_str()),
            (FOOTER_KEY, self.footer.as_str()),
            (WORD_TEMPLATE_KEY, template.as_str()),
        ] {
            // SAFETY: A string is a property list value, as user defaults require.
            unsafe { defaults.setObject_forKey(Some(&ns(value)), &ns(key)) };
        }
    }

    /// Whether printed pages carry anything beside the text.
    pub(super) fn decorates_pages(&self) -> bool {
        self.page_numbers || !self.header.trim().is_empty() || !self.footer.trim().is_empty()
    }
}

/// What the metadata says about the document.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) struct TitleFacts {
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) author: String,
    pub(super) date: String,
}

impl TitleFacts {
    /// `name` stands in for a title the metadata does not give.
    pub(super) fn of(doc: &Doc, name: &str) -> TitleFacts {
        let fields = front_matter::read(doc).map(|fm| fm.fields).unwrap_or_default();
        let text = |wanted: &str| {
            fields
                .iter()
                .find(|field| field.key.eq_ignore_ascii_case(wanted))
                .map(|field| match &field.value {
                    Value::Text(text) => text.clone(),
                    Value::List(items) => items.join(", "),
                    Value::Complex(_) => String::new(),
                })
                .unwrap_or_default()
        };
        let title = text("title");
        let author = {
            let one = text("author");
            if one.is_empty() { text("authors") } else { one }
        };
        TitleFacts {
            title: if title.is_empty() { name.to_string() } else { title },
            subtitle: text("subtitle"),
            author,
            date: text("date"),
        }
    }
}

/// A header or footer as the page shows it: `{title}`, `{author}`, `{date}`, `{page}` and `{pages}` filled in, the marks
/// themselves replaced at once so that a title holding one stays as it is.
pub(super) fn filled_in(template: &str, facts: &TitleFacts, page: usize, pages: usize) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        let found = [
            ("{title}", facts.title.as_str().to_string()),
            ("{author}", facts.author.clone()),
            ("{date}", facts.date.clone()),
            ("{page}", page.to_string()),
            ("{pages}", pages.to_string()),
        ]
        .into_iter()
        .find(|(mark, _)| rest.starts_with(mark));
        match found {
            Some((mark, value)) => {
                out.push_str(&value);
                rest = &rest[mark.len()..];
            }
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The footer of a page: what the user wrote, with the page number behind it when asked for.
pub(super) fn footer_text(options: &ExportOptions, facts: &TitleFacts, page: usize, pages: usize) -> String {
    let written = filled_in(&options.footer, facts, page, pages);
    let written = written.trim();
    match (
        options.page_numbers && !options.footer.contains("{page}"),
        written.is_empty(),
    ) {
        (true, true) => format!("{page} / {pages}"),
        (true, false) => format!("{written} · {page} / {pages}"),
        (false, _) => written.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::reader::parse;

    fn facts() -> TitleFacts {
        TitleFacts {
            title: "Bericht".to_string(),
            subtitle: String::new(),
            author: "A. Muster".to_string(),
            date: "2026-11-05".to_string(),
        }
    }

    #[test]
    fn marks_are_filled_in_and_everything_else_stays() {
        assert_eq!(
            filled_in("{title} von {author}, {date}: {page}/{pages} {x} {", &facts(), 2, 9),
            "Bericht von A. Muster, 2026-11-05: 2/9 {x} {"
        );
    }

    #[test]
    fn a_title_holding_a_mark_stays_as_it_is() {
        let tricky = TitleFacts {
            title: "{page}".to_string(),
            ..facts()
        };
        assert_eq!(filled_in("{title}", &tricky, 2, 9), "{page}");
    }

    #[test]
    fn the_page_number_comes_once() {
        let numbered = ExportOptions {
            page_numbers: true,
            ..ExportOptions::default()
        };
        assert_eq!(footer_text(&numbered, &facts(), 1, 3), "1 / 3");
        let with_text = ExportOptions {
            footer: "{title}".to_string(),
            ..numbered.clone()
        };
        assert_eq!(footer_text(&with_text, &facts(), 1, 3), "Bericht · 1 / 3");
        let own_mark = ExportOptions {
            footer: "Seite {page}".to_string(),
            ..numbered
        };
        assert_eq!(footer_text(&own_mark, &facts(), 2, 3), "Seite 2");
        assert_eq!(footer_text(&ExportOptions::default(), &facts(), 1, 3), "");
    }

    #[test]
    fn the_title_page_reads_the_metadata_and_falls_back_on_the_name() {
        let doc = parse("---\ntitle: \"Ein Titel\"\nsubtitle: Zweiter\nAuthor: Wer\ndate: 2026-01-02\n---\n\nText\n");
        assert_eq!(
            TitleFacts::of(&doc, "Name"),
            TitleFacts {
                title: "Ein Titel".to_string(),
                subtitle: "Zweiter".to_string(),
                author: "Wer".to_string(),
                date: "2026-01-02".to_string(),
            }
        );
        assert_eq!(TitleFacts::of(&parse("Text\n"), "Name").title, "Name");
        let several = parse("---\nauthors: [A, B]\n---\n");
        assert_eq!(TitleFacts::of(&several, "Name").author, "A, B");
    }

    #[test]
    fn pages_are_decorated_only_when_something_is_asked_for() {
        assert!(!ExportOptions::default().decorates_pages());
        assert!(
            !ExportOptions {
                header: "  ".to_string(),
                ..ExportOptions::default()
            }
            .decorates_pages()
        );
        assert!(
            ExportOptions {
                page_numbers: true,
                ..ExportOptions::default()
            }
            .decorates_pages()
        );
    }
}
