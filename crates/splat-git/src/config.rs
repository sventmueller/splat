//! The little of `config` that says whether a repository can be read at all. No other value of it is ever used, since all of
//! it is what a foreign repository wants to be done with it.

use crate::ReadError;

/// Whether what `config` says about the format of the repository is what this crate can honour. Like Git's own check of the
/// format, this does not follow includes. Git ignores extensions it does not know in version 0, where this is stricter.
pub fn check_format(config: &[u8]) -> Result<(), ReadError> {
    let mut reader = Reader {
        rest: config.strip_prefix(b"\xef\xbb\xbf").unwrap_or(config),
    };
    let mut section = Section::default();
    loop {
        reader.skip_while(|byte| byte.is_ascii_whitespace());
        match reader.rest.first() {
            None => return Ok(()),
            Some(b'#' | b';') => reader.skip_while(|byte| byte != b'\n'),
            Some(b'[') => section = reader.section()?,
            Some(byte) if byte.is_ascii_alphabetic() => {
                let key = reader.key();
                let value = reader.value()?;
                judge(&section, &key, &value)?;
            }
            Some(_) => return Err(ReadError::Damaged),
        }
    }
}

fn judge(section: &Section, key: &[u8], value: &[u8]) -> Result<(), ReadError> {
    let readable = match (section.name.as_slice(), section.has_subsection, key) {
        (b"core", false, b"repositoryformatversion") => matches!(value, b"0" | b"1"),
        (b"extensions", false, b"objectformat") => value == b"sha1",
        (b"extensions", false, b"refstorage") => value == b"files",
        // None of these changes how the objects and refs that are there get read.
        (b"extensions", false, b"noop" | b"noop-v1" | b"preciousobjects" | b"partialclone" | b"worktreeconfig") => true,
        (b"extensions", false, b"relativeworktrees") => true,
        (b"extensions", ..) => false,
        _ => true,
    };
    if readable { Ok(()) } else { Err(ReadError::Unsupported) }
}

#[derive(Default)]
struct Section {
    name: Vec<u8>,
    has_subsection: bool,
}

struct Reader<'a> {
    rest: &'a [u8],
}

impl Reader<'_> {
    fn skip_while(&mut self, keep_going: impl Fn(u8) -> bool) {
        let end = self
            .rest
            .iter()
            .position(|&byte| !keep_going(byte))
            .unwrap_or(self.rest.len());
        self.rest = &self.rest[end..];
    }

    fn next(&mut self) -> Option<u8> {
        let (&first, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(first)
    }

    /// `[name]` or `[name "subsection"]`; the subsection only counts for being there.
    fn section(&mut self) -> Result<Section, ReadError> {
        self.next();
        let mut name: Vec<u8> = self
            .rest
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
            .map(u8::to_ascii_lowercase)
            .collect();
        self.rest = &self.rest[name.len()..];
        let dot = name.iter().position(|&byte| byte == b'.');
        let has_subsection = dot.is_some() || self.subsection()?;
        name.truncate(dot.unwrap_or(name.len()));
        if self.next() != Some(b']') {
            return Err(ReadError::Damaged);
        }
        Ok(Section { name, has_subsection })
    }

    fn subsection(&mut self) -> Result<bool, ReadError> {
        self.skip_while(|byte| matches!(byte, b' ' | b'\t'));
        if self.rest.first() != Some(&b'"') {
            return Ok(false);
        }
        self.next();
        loop {
            match self.next() {
                Some(b'"') => return Ok(true),
                Some(b'\\') => {
                    self.next();
                }
                Some(b'\n') | None => return Err(ReadError::Damaged),
                Some(_) => {}
            }
        }
    }

    fn key(&mut self) -> Vec<u8> {
        let key: Vec<u8> = self
            .rest
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'-')
            .map(u8::to_ascii_lowercase)
            .collect();
        self.rest = &self.rest[key.len()..];
        key
    }

    /// What stands behind a key: nothing at all, or `=` and a value that may hold quotes, escapes, continued lines and a
    /// comment, with white space around it dropped.
    fn value(&mut self) -> Result<Vec<u8>, ReadError> {
        self.skip_while(|byte| matches!(byte, b' ' | b'\t' | b'\r'));
        match self.rest.first() {
            Some(b'=') => {
                self.next();
            }
            Some(b'\n' | b'#' | b';') | None => return Ok(Vec::new()),
            Some(_) => return Err(ReadError::Damaged),
        }
        self.skip_while(|byte| matches!(byte, b' ' | b'\t'));
        let mut value = Vec::new();
        let mut kept = 0;
        let mut quoted = false;
        loop {
            match self.next() {
                None | Some(b'\n') if quoted => return Err(ReadError::Damaged),
                None | Some(b'\n') => break,
                Some(b'"') => {
                    quoted = !quoted;
                    kept = value.len();
                }
                Some(b'\\') => {
                    match self.next() {
                        Some(b'\n') => {}
                        Some(b'n') => value.push(b'\n'),
                        Some(b't') => value.push(b'\t'),
                        Some(b'b') => value.push(8),
                        Some(byte @ (b'"' | b'\\')) => value.push(byte),
                        _ => return Err(ReadError::Damaged),
                    }
                    kept = value.len();
                }
                Some(b'#' | b';') if !quoted => {
                    self.skip_while(|byte| byte != b'\n');
                    break;
                }
                Some(byte) => {
                    value.push(byte);
                    if quoted || !byte.is_ascii_whitespace() {
                        kept = value.len();
                    }
                }
            }
        }
        value.truncate(kept);
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readable(config: &str) -> Result<(), ReadError> {
        check_format(config.as_bytes())
    }

    #[test]
    fn what_git_init_writes_is_readable() {
        let plain =
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n";
        assert_eq!(readable(plain), Ok(()));
        assert_eq!(readable(""), Ok(()));
    }

    #[test]
    fn a_sha256_repository_cannot_be_read() {
        let config = "[core]\n\trepositoryformatversion = 1\n[extensions]\n\tobjectformat = sha256\n";
        assert_eq!(readable(config), Err(ReadError::Unsupported));
    }

    #[test]
    fn sha1_may_be_named_as_the_format() {
        assert_eq!(readable("[extensions]\nobjectformat = sha1\n"), Ok(()));
    }

    #[test]
    fn newer_format_versions_cannot_be_read() {
        assert_eq!(readable("[core]\nrepositoryformatversion = 1\n"), Ok(()));
        assert_eq!(
            readable("[core]\nrepositoryformatversion = 2\n"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(
            readable("[core]\nrepositoryformatversion = many\n"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(
            readable("[core]\nrepositoryformatversion\n"),
            Err(ReadError::Unsupported)
        );
    }

    #[test]
    fn refs_in_a_reftable_cannot_be_read() {
        assert_eq!(
            readable("[extensions]\nrefstorage = reftable\n"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(readable("[extensions]\nrefstorage = files\n"), Ok(()));
    }

    #[test]
    fn extensions_that_ask_nothing_of_a_reader_are_fine_and_unknown_ones_are_not() {
        for known in [
            "noop",
            "noop-v1",
            "preciousObjects",
            "partialClone",
            "worktreeConfig",
            "relativeWorktrees",
        ] {
            assert_eq!(readable(&format!("[extensions]\n{known} = true\n")), Ok(()), "{known}");
        }
        assert_eq!(
            readable("[extensions]\nsomethingNew = true\n"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(
            readable("[extensions \"sub\"]\nnoop = true\n"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(readable("[extensions.sub]\nnoop = true\n"), Err(ReadError::Unsupported));
    }

    #[test]
    fn section_and_key_names_ignore_case() {
        assert_eq!(
            readable("[EXTENSIONS]\nOBJECTFORMAT = sha256\n"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(
            readable("[Core]\nRepositoryFormatVersion = 2\n"),
            Err(ReadError::Unsupported)
        );
    }

    #[test]
    fn a_value_may_be_quoted_escaped_continued_or_commented() {
        for spelled in [
            "\"sha256\"",
            "sha\"256\"",
            "sha\\\n256",
            "sha256 # comment",
            "sha256 ; comment",
            "sha256\t\r",
            "  sha256",
        ] {
            let config = format!("[extensions]\nobjectformat = {spelled}\n");
            assert_eq!(readable(&config), Err(ReadError::Unsupported), "{spelled}");
        }
        assert_eq!(readable("[extensions]\nobjectformat = \"sha1\"\n"), Ok(()));
        assert_eq!(readable("[extensions]\nobjectformat = sha\\\n1 # comment\n"), Ok(()));
    }

    #[test]
    fn white_space_inside_quotes_counts_and_outside_it_does_not() {
        assert_eq!(
            readable("[extensions]\nobjectformat = \"sha1 \"\n"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(readable("[extensions]\nobjectformat = sha1 \n"), Ok(()));
    }

    #[test]
    fn escapes_are_read_as_git_reads_them() {
        for value in ["a\\nb", "a\\tb", "a\\bb", "a\\\"b", "a\\\\b"] {
            assert_eq!(readable(&format!("[other]\nkey = {value}\n")), Ok(()), "{value}");
        }
        assert_eq!(readable("[other]\nkey = a\\qb\n"), Err(ReadError::Damaged));
        assert_eq!(readable("[other]\nkey = a\\"), Err(ReadError::Damaged));
    }

    #[test]
    fn several_things_may_stand_on_a_line_and_comments_anywhere() {
        assert_eq!(
            readable("[core] repositoryformatversion = 2"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(readable("# [extensions]\n; objectformat = sha256\n[core]\n"), Ok(()));
        assert_eq!(
            readable("\u{feff}[extensions] objectformat = sha256"),
            Err(ReadError::Unsupported)
        );
        assert_eq!(
            readable("[extensions] noop # true\nobjectformat = sha256"),
            Err(ReadError::Unsupported)
        );
    }

    #[test]
    fn other_sections_with_any_values_do_not_matter() {
        let hostile =
            "[core]\n\tfsmonitor = /tmp/evil\n\tsshCommand = evil\n[remote \"a\\\"b\"]\n\turl = x\n[alias]\n\ta\n";
        assert_eq!(readable(hostile), Ok(()));
    }

    #[test]
    fn what_is_no_config_is_damaged() {
        for config in [
            "!",
            "=value",
            "[core",
            "[core\n]",
            "[core \"sub]",
            "[core \"sub\n\"]",
            "[core \"sub\" x]",
            "[other]\nkey ! value\n",
            "[other]\nkey = \"open\n",
            "[other]\nkey = \"open",
        ] {
            assert_eq!(readable(config), Err(ReadError::Damaged), "{config}");
        }
    }

    #[test]
    fn a_key_at_the_end_of_the_file_is_read() {
        assert_eq!(readable("[other]\nkey"), Ok(()));
        assert_eq!(readable("[other]\nkey ; comment\n"), Ok(()));
        assert_eq!(readable("[other]\nkey ="), Ok(()));
    }

    #[test]
    fn an_escape_in_a_subsection_may_hide_a_quote() {
        assert_eq!(readable("[other \"a\\\"b\"]\nkey = 1\n"), Ok(()));
    }
}
