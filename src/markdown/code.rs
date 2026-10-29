use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeToken {
    Keyword,
    String,
    Comment,
    Number,
}

struct Lang {
    names: &'static [&'static str],
    keywords: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    triple_quotes: bool,
    case_insensitive: bool,
    /// Tag names are keywords and quotes only count inside tags.
    markup: bool,
    /// A word before `:` at the start of a line is a key.
    line_keys: bool,
    /// Labels in node shapes such as `A[Start]` count as strings.
    labels: bool,
    /// Identifiers may start with `\`, as TeX commands do.
    backslash_words: bool,
}

const fn lang(names: &'static [&'static str], keywords: &'static [&'static str]) -> Lang {
    Lang {
        names,
        keywords,
        line_comments: &["//"],
        block_comment: Some(("/*", "*/")),
        quotes: &['"', '\''],
        triple_quotes: false,
        case_insensitive: false,
        markup: false,
        line_keys: false,
        labels: false,
        backslash_words: false,
    }
}

const C_WORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum", "extern", "float",
    "for", "goto", "if", "inline", "int", "long", "register", "return", "short", "signed", "sizeof", "static",
    "struct", "switch", "typedef", "union", "unsigned", "void", "volatile", "while", "bool", "true", "false", "NULL",
    "#include", "#define", "#ifdef", "#ifndef", "#endif", "#if", "#else", "#pragma",
];
const CPP_WORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "bool",
    "true",
    "false",
    "class",
    "namespace",
    "template",
    "typename",
    "public",
    "private",
    "protected",
    "virtual",
    "override",
    "new",
    "delete",
    "this",
    "using",
    "try",
    "catch",
    "throw",
    "nullptr",
    "constexpr",
    "noexcept",
    "std",
    "#include",
    "#define",
    "#ifdef",
    "#ifndef",
    "#endif",
    "#if",
    "#else",
    "#pragma",
];
const CSHARP_WORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "false",
    "finally",
    "float",
    "for",
    "foreach",
    "if",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "out",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "record",
    "ref",
    "return",
    "sealed",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "using",
    "var",
    "virtual",
    "void",
    "while",
];
const GO_WORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "nil",
    "true",
    "false",
    "string",
    "int",
    "bool",
    "error",
    "byte",
    "float64",
    "any",
];
const JAVA_WORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "try",
    "var",
    "void",
    "volatile",
    "while",
    "true",
    "false",
    "null",
];
const JS_WORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "yield",
    "true",
    "false",
    "null",
    "undefined",
];
const TS_WORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "yield",
    "true",
    "false",
    "null",
    "undefined",
    "interface",
    "type",
    "enum",
    "implements",
    "private",
    "public",
    "protected",
    "readonly",
    "as",
    "any",
    "number",
    "string",
    "boolean",
    "unknown",
    "never",
];
const KOTLIN_WORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "data",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "import",
    "in",
    "interface",
    "internal",
    "is",
    "null",
    "object",
    "open",
    "override",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "sealed",
    "super",
    "suspend",
    "this",
    "throw",
    "true",
    "try",
    "catch",
    "finally",
    "val",
    "var",
    "when",
    "while",
];
const PHP_WORDS: &[&str] = &[
    "array",
    "as",
    "break",
    "case",
    "class",
    "const",
    "continue",
    "default",
    "echo",
    "else",
    "elseif",
    "extends",
    "false",
    "for",
    "foreach",
    "function",
    "if",
    "implements",
    "interface",
    "namespace",
    "new",
    "null",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "switch",
    "throw",
    "true",
    "try",
    "catch",
    "use",
    "while",
];
const PYTHON_WORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or",
    "pass", "raise", "return", "True", "try", "while", "with", "yield", "self", "match", "case",
];
const RUBY_WORDS: &[&str] = &[
    "begin",
    "break",
    "case",
    "class",
    "def",
    "do",
    "else",
    "elsif",
    "end",
    "ensure",
    "false",
    "for",
    "if",
    "in",
    "module",
    "next",
    "nil",
    "require",
    "rescue",
    "return",
    "self",
    "then",
    "true",
    "unless",
    "until",
    "when",
    "while",
    "yield",
    "puts",
    "attr_accessor",
];
const RUST_WORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "Some", "None", "Ok",
    "Err",
];
const SHELL_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function",
    "return", "local", "export", "readonly", "declare", "set", "unset", "shift", "exit", "source", "alias", "echo",
    "cd", "true", "false",
];
const SQL_WORDS: &[&str] = &[
    "select",
    "from",
    "where",
    "insert",
    "into",
    "values",
    "update",
    "set",
    "delete",
    "create",
    "table",
    "drop",
    "alter",
    "add",
    "index",
    "primary",
    "key",
    "foreign",
    "references",
    "join",
    "left",
    "right",
    "inner",
    "outer",
    "full",
    "on",
    "group",
    "by",
    "order",
    "having",
    "limit",
    "offset",
    "as",
    "and",
    "or",
    "not",
    "null",
    "is",
    "in",
    "like",
    "between",
    "distinct",
    "union",
    "all",
    "case",
    "when",
    "then",
    "else",
    "end",
    "exists",
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "default",
    "constraint",
    "unique",
    "view",
    "with",
    "asc",
    "desc",
    "returning",
    "true",
    "false",
];
const SWIFT_WORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "extension",
    "false",
    "fileprivate",
    "for",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "internal",
    "let",
    "nil",
    "override",
    "private",
    "protocol",
    "public",
    "repeat",
    "return",
    "self",
    "Self",
    "static",
    "struct",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "var",
    "while",
];
const CSS_WORDS: &[&str] = &[
    "!important",
    "@media",
    "@import",
    "@font-face",
    "@keyframes",
    "@supports",
];
const JSON_WORDS: &[&str] = &["true", "false", "null"];
const YAML_WORDS: &[&str] = &["true", "false", "null", "yes", "no", "on", "off"];
const MERMAID_WORDS: &[&str] = &[
    "flowchart",
    "graph",
    "subgraph",
    "end",
    "direction",
    "TD",
    "TB",
    "LR",
    "RL",
    "BT",
    "sequenceDiagram",
    "participant",
    "actor",
    "loop",
    "alt",
    "else",
    "opt",
    "par",
    "and",
    "Note",
    "note",
    "over",
    "left",
    "right",
    "of",
    "gantt",
    "section",
    "title",
    "dateFormat",
    "axisFormat",
    "excludes",
    "classDiagram",
    "stateDiagram",
    "erDiagram",
    "pie",
    "mindmap",
    "timeline",
    "classDef",
    "class",
    "style",
    "linkStyle",
    "click",
    "activate",
    "deactivate",
    "autonumber",
    "milestone",
    "after",
    "done",
    "active",
    "crit",
];

const LANGS: &[Lang] = &[
    Lang {
        line_comments: &["#"],
        block_comment: None,
        ..lang(&["bash", "sh", "shell", "zsh", "console"], SHELL_WORDS)
    },
    lang(&["c", "h"], C_WORDS),
    lang(&["cpp", "c++", "cc", "hpp"], CPP_WORDS),
    lang(&["csharp", "cs", "c#"], CSHARP_WORDS),
    Lang {
        line_comments: &[],
        quotes: &['"', '\''],
        ..lang(&["css", "scss", "less"], CSS_WORDS)
    },
    Lang {
        quotes: &['"', '\'', '`'],
        ..lang(&["go", "golang"], GO_WORDS)
    },
    Lang {
        line_comments: &[],
        block_comment: Some(("<!--", "-->")),
        markup: true,
        ..lang(&["html", "xml", "svg", "xhtml", "vue"], &[])
    },
    lang(&["java"], JAVA_WORDS),
    Lang {
        quotes: &['"', '\'', '`'],
        ..lang(&["javascript", "js", "jsx", "mjs"], JS_WORDS)
    },
    Lang {
        line_comments: &[],
        block_comment: None,
        quotes: &['"'],
        ..lang(&["json", "jsonc"], JSON_WORDS)
    },
    lang(&["kotlin", "kt"], KOTLIN_WORDS),
    Lang {
        line_comments: &["//", "#"],
        ..lang(&["php"], PHP_WORDS)
    },
    Lang {
        line_comments: &["#"],
        block_comment: None,
        triple_quotes: true,
        ..lang(&["python", "py"], PYTHON_WORDS)
    },
    Lang {
        line_comments: &["#"],
        block_comment: None,
        ..lang(&["ruby", "rb"], RUBY_WORDS)
    },
    lang(&["rust", "rs"], RUST_WORDS),
    Lang {
        line_comments: &["--"],
        quotes: &['\''],
        case_insensitive: true,
        ..lang(&["sql", "postgresql", "psql", "mysql", "sqlite"], SQL_WORDS)
    },
    lang(&["swift"], SWIFT_WORDS),
    Lang {
        quotes: &['"', '\'', '`'],
        ..lang(&["typescript", "ts", "tsx"], TS_WORDS)
    },
    Lang {
        line_comments: &["#"],
        block_comment: None,
        line_keys: true,
        ..lang(&["yaml", "yml"], YAML_WORDS)
    },
    Lang {
        line_comments: &["%%"],
        block_comment: None,
        quotes: &['"'],
        labels: true,
        ..lang(&["mermaid"], MERMAID_WORDS)
    },
    Lang {
        line_comments: &["%"],
        block_comment: None,
        quotes: &[],
        backslash_words: true,
        ..lang(&["tex", "latex"], &[])
    },
];

/// Languages offered when inserting a code block: info string and menu title.
pub const MENU: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("c", "C"),
    ("cpp", "C++"),
    ("csharp", "C#"),
    ("css", "CSS"),
    ("go", "Go"),
    ("html", "HTML"),
    ("java", "Java"),
    ("javascript", "JavaScript"),
    ("json", "JSON"),
    ("kotlin", "Kotlin"),
    ("php", "PHP"),
    ("python", "Python"),
    ("ruby", "Ruby"),
    ("rust", "Rust"),
    ("sql", "SQL"),
    ("swift", "Swift"),
    ("typescript", "TypeScript"),
    ("xml", "XML"),
    ("yaml", "YAML"),
];

fn find(info: &str) -> Option<&'static Lang> {
    let name = info.split_whitespace().next()?.to_ascii_lowercase();
    LANGS.iter().find(|l| l.names.contains(&name.as_str()))
}

pub fn known(info: &str) -> bool {
    find(info).is_some()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Keywords, strings, comments and numbers in `code`, for the language named by a code block's info string.
pub fn tokens(info: &str, code: &str) -> Vec<(Range<usize>, CodeToken)> {
    let Some(lang) = find(info) else {
        return vec![];
    };
    let mut out = Vec::new();
    let line_end = |from: usize| code[from..].find('\n').map_or(code.len(), |i| from + i);
    let mut in_tag = false;
    let mut line_start = true;
    let mut i = 0;
    while i < code.len() {
        let rest = &code[i..];
        let c = rest.chars().next().unwrap();
        let prev = code[..i].chars().next_back();
        if c == '\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if let Some(marker) = lang.line_comments.iter().find(|m| rest.starts_with(**m)) {
            let hash_inside_word = *marker == "#" && prev.is_some_and(|p| !p.is_whitespace());
            if !hash_inside_word {
                let end = line_end(i);
                out.push((i..end, CodeToken::Comment));
                i = end;
                continue;
            }
        }
        if let Some((open, close)) = lang.block_comment
            && rest.starts_with(open)
        {
            let end = rest[open.len()..]
                .find(close)
                .map_or(code.len(), |k| i + open.len() + k + close.len());
            out.push((i..end, CodeToken::Comment));
            i = end;
            continue;
        }
        if lang.triple_quotes && (rest.starts_with("\"\"\"") || rest.starts_with("'''")) {
            let fence = &rest[..3];
            let end = rest[3..].find(fence).map_or(code.len(), |k| i + 3 + k + 3);
            out.push((i..end, CodeToken::String));
            i = end;
            continue;
        }
        if lang.markup {
            if c == '<' {
                in_tag = true;
                let name_start = i + 1 + usize::from(rest[1..].starts_with('/'));
                let len = code[name_start..]
                    .find(|ch: char| !(is_word(ch) || ch == '-' || ch == ':'))
                    .unwrap_or(code.len() - name_start);
                if len > 0 {
                    out.push((name_start..name_start + len, CodeToken::Keyword));
                }
                i = name_start + len;
                continue;
            }
            if c == '>' {
                in_tag = false;
            }
        }
        let quoted = lang.quotes.contains(&c) && (!lang.markup || in_tag);
        if quoted {
            let end = string_end(code, i, c);
            if c == '\'' && lang.names.contains(&"rust") && end - i > 6 {
                i += 1;
                continue;
            }
            out.push((i..end, CodeToken::String));
            i = end;
            continue;
        }
        if lang.labels && matches!(c, '[' | '(' | '{' | '|') && prev.is_some_and(|p| is_word(p) || p == '>') {
            let close = match c {
                '[' => ']',
                '(' => ')',
                '{' => '}',
                _ => '|',
            };
            let end = line_end(i);
            if let Some(k) = code[i + 1..end].find(close) {
                let label = i + 1..i + 1 + k;
                if !label.is_empty() {
                    out.push((label, CodeToken::String));
                }
                i += 1 + k + 1;
                continue;
            }
        }
        if c.is_ascii_digit() && !prev.is_some_and(is_word) {
            let len = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_'))
                .unwrap_or(rest.len());
            out.push((i..i + len, CodeToken::Number));
            i += len;
            line_start = false;
            continue;
        }
        let word_start = is_word(c)
            || (c == '#' && lang.keywords.iter().any(|k| k.starts_with('#')))
            || (matches!(c, '@' | '!') && lang.keywords.iter().any(|k| k.starts_with(c)))
            || (c == '\\' && lang.backslash_words);
        if word_start && !prev.is_some_and(is_word) {
            let first = c.len_utf8();
            let len = first
                + rest[first..]
                    .find(|ch: char| !(is_word(ch) || (lang.markup && ch == '-')))
                    .unwrap_or(rest.len() - first);
            let word = &rest[..len];
            let keyword = if c == '\\' {
                len > 1
            } else if lang.case_insensitive {
                lang.keywords.iter().any(|k| k.eq_ignore_ascii_case(word))
            } else {
                lang.keywords.contains(&word)
            };
            let key = lang.line_keys && line_start && code[i + len..].trim_start_matches(' ').starts_with(':');
            if keyword || key {
                out.push((i..i + len, CodeToken::Keyword));
            }
            i += len;
            line_start = false;
            continue;
        }
        if !c.is_whitespace() {
            line_start = false;
        }
        i += c.len_utf8();
    }
    out
}

/// End of a quoted string starting at `start`, honouring backslash escapes; unterminated strings end with the line.
fn string_end(code: &str, start: usize, quote: char) -> usize {
    let mut escaped = false;
    for (k, ch) in code[start + quote.len_utf8()..].char_indices() {
        let at = start + quote.len_utf8() + k;
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return at + ch.len_utf8();
        } else if ch == '\n' && quote != '`' {
            return at;
        }
    }
    code.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens_of<'a>(info: &str, code: &'a str) -> Vec<(&'a str, CodeToken)> {
        tokens(info, code).into_iter().map(|(r, t)| (&code[r], t)).collect()
    }

    #[test]
    fn python_keywords_strings_comments_numbers() {
        let t = tokens_of(
            "python",
            "def f(x):  # Kommentar\n    return 'ä' + \"\"\"doc\"\"\" * 42\n",
        );
        assert_eq!(
            t,
            vec![
                ("def", CodeToken::Keyword),
                ("# Kommentar", CodeToken::Comment),
                ("return", CodeToken::Keyword),
                ("'ä'", CodeToken::String),
                ("\"\"\"doc\"\"\"", CodeToken::String),
                ("42", CodeToken::Number),
            ]
        );
    }

    #[test]
    fn sql_is_case_insensitive_and_rust_lifetimes_are_not_strings() {
        let t = tokens_of("SQL", "SELECT name FROM t -- x\n");
        assert_eq!(t[0], ("SELECT", CodeToken::Keyword));
        assert_eq!(t[1], ("FROM", CodeToken::Keyword));
        assert_eq!(t[2], ("-- x", CodeToken::Comment));
        let t = tokens_of("rust", "fn f<'a>(s: &'a str) -> char { 'x' }\n");
        assert!(t.contains(&("'x'", CodeToken::String)));
        assert!(!t.iter().any(|(s, _)| s.starts_with("'a")));
    }

    #[test]
    fn markup_tags_mermaid_labels_and_tex_commands() {
        let t = tokens_of("html", "<p class=\"x\">it's</p>");
        assert_eq!(
            t,
            vec![
                ("p", CodeToken::Keyword),
                ("\"x\"", CodeToken::String),
                ("p", CodeToken::Keyword)
            ]
        );
        let t = tokens_of("mermaid", "flowchart TD\n    A[Start] -->|Ja| B{Ende} %% c\n");
        assert!(t.contains(&("flowchart", CodeToken::Keyword)));
        assert!(t.contains(&("Start", CodeToken::String)));
        assert!(t.contains(&("Ja", CodeToken::String)));
        assert!(t.contains(&("Ende", CodeToken::String)));
        assert!(t.contains(&("%% c", CodeToken::Comment)));
        let t = tokens_of("tex", "\\frac{1}{2} % x");
        assert_eq!(t[0], ("\\frac", CodeToken::Keyword));
        assert!(t.contains(&("% x", CodeToken::Comment)));
        assert!(tokens("unbekannt", "if x").is_empty());
    }
}
