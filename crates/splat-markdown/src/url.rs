//! What an address in a document may be: one reading of it for the preview and both exports.

/// The scheme of an address, in lowercase, as a browser reads it: it drops tabs and line breaks anywhere in an address
/// and control characters around it, so `java&#9;script:` has a scheme to it. A relative path has none.
pub fn scheme(url: &str) -> Option<String> {
    let cleaned: String = url
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let (scheme, _) = cleaned.split_once(':')?;
    let mut chars = scheme.chars();
    (chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then(|| scheme.to_ascii_lowercase())
}

/// Whether the address leads to the web, or holds its own content.
pub fn is_web_or_data(url: &str) -> bool {
    matches!(scheme(url).as_deref(), Some("http" | "https" | "data"))
}

/// Whether the address leads to the web.
pub fn is_web(url: &str) -> bool {
    matches!(scheme(url).as_deref(), Some("http" | "https"))
}

/// What stands behind `file://`, if the address starts with that.
pub fn behind_file_scheme(url: &str) -> Option<&str> {
    let scheme = url.get(.."file://".len())?;
    scheme.eq_ignore_ascii_case("file://").then(|| &url[scheme.len()..])
}

/// Whether the address leads to the web or to a mail app.
pub(crate) fn is_web_or_mail(url: &str) -> bool {
    matches!(scheme(url).as_deref(), Some("http" | "https" | "mailto"))
}

/// Whether the address leads to a server that Splat would ask for a picture.
pub fn is_https(url: &str) -> bool {
    scheme(url).as_deref() == Some("https")
}

/// Whether a link or picture in embedded HTML may use the address: nothing but the web, mail and relative paths.
pub fn is_harmless(url: &str) -> bool {
    scheme(url).is_none() || is_web_or_mail(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_has_the_scheme_a_browser_sees() {
        for (url, wanted) in [
            ("https://a.de", Some("https")),
            ("JavaScript:x", Some("javascript")),
            ("java\tscript:x", Some("javascript")),
            ("\u{1}javascript:x", Some("javascript")),
            (" javascript:x", Some("javascript")),
            ("mailto:a@b.c", Some("mailto")),
            ("img/a.png", None),
            ("#anker", None),
            ("1a:b", None),
            ("a b:c", None),
        ] {
            assert_eq!(scheme(url).as_deref(), wanted, "{url:?}");
        }
    }

    #[test]
    fn the_case_of_a_scheme_does_not_matter() {
        assert!(is_https("HTTPS://a.de") && is_web("HtTp://a.de") && is_web_or_data("DATA:x"));
        assert!(!is_https("http://a.de") && !is_web("a.png") && !is_web_or_data("a.png"));
        assert_eq!(behind_file_scheme("FILE:///a.png"), Some("/a.png"));
        assert_eq!(behind_file_scheme("file:///a.png"), Some("/a.png"));
        assert_eq!(behind_file_scheme("/a.png"), None);
        assert_eq!(behind_file_scheme("ä"), None);
    }

    #[test]
    fn only_the_web_mail_and_paths_are_harmless() {
        for url in [
            "https://a.de",
            "HTTP://a.de",
            "mailto:a@b.c",
            "bild.png",
            "../a/b.md",
            "#x",
        ] {
            assert!(is_harmless(url), "{url}");
        }
        for url in [
            "javascript:x",
            "java\tscript:x",
            "\u{1}javascript:x",
            "file:///etc/passwd",
            "data:text/html,x",
        ] {
            assert!(!is_harmless(url), "{url:?}");
        }
        assert!(is_web_or_mail("https://a.de") && !is_web_or_mail("bild.png"));
    }
}
