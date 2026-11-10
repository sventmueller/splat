//! Keywords, strings, comments and numbers in the code of a code block, for the common languages its info string can
//! name.

use std::ops::Range;

/// What a stretch of code is to its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeToken {
    /// A keyword of the language, a tag name in markup, or a key in front of `:` at the start of a YAML line.
    Keyword,
    /// A quoted string, or a label in a diagram's node shape such as `A[Start]`.
    String,
    /// A line or block comment.
    Comment,
    /// A number literal.
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
    /// A key in front of `=`, at the start of a line or of an entry of an inline table, and a `[table]` or `[[array]]`
    /// header on a line of its own are keywords.
    equals_keys: bool,
    /// Numbers may hold `-`, `:` and `+`, as dates and times do.
    dates: bool,
    /// The quote of strings that know no escapes, as TOML's literal strings.
    literal_quote: Option<char>,
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
        equals_keys: false,
        dates: false,
        literal_quote: None,
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
const TOML_WORDS: &[&str] = &["true", "false", "inf", "nan"];
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
        line_comments: &["#"],
        block_comment: None,
        triple_quotes: true,
        equals_keys: true,
        dates: true,
        literal_quote: Some('\''),
        ..lang(&["toml"], TOML_WORDS)
    },
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

pub(crate) fn known(info: &str) -> bool {
    find(info).is_some()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One info string for each language [`tokens`] knows, the first name it goes by.
pub fn language_names() -> impl Iterator<Item = &'static str> {
    LANGS.iter().map(|lang| lang.names[0])
}

/// Keywords, strings, comments and numbers in `code`, as byte ranges in it, for the language named by a code block's
/// info string; nothing for a language it does not know.
#[expect(clippy::too_many_lines, reason = "one tokenizer loop with a case per kind of token")]
pub fn tokens(info: &str, code: &str) -> Vec<(Range<usize>, CodeToken)> {
    let Some(lang) = find(info) else {
        return vec![];
    };
    let mut out = Vec::new();
    let line_end = |from: usize| code[from..].find('\n').map_or(code.len(), |i| from + i);
    let mut in_tag = false;
    let mut line_start = true;
    // How far each closing mark of a label is known to be missing: a long line of opening marks would otherwise be
    // searched once per mark.
    let mut no_closer_until = [0usize; 4];
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
        if lang.equals_keys
            && let Some(len) = toml_key_or_header(code, i, line_start)
        {
            out.push((i..i + len, CodeToken::Keyword));
            i += len;
            line_start = false;
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
        if quoted && c == '\'' && lang.names.contains(&"rust") {
            match rust_char_len(rest) {
                Some(len) => {
                    out.push((i..i + len, CodeToken::String));
                    i += len;
                }
                None => i += 1,
            }
            continue;
        }
        if quoted {
            let end = string_end(code, i, c, lang.literal_quote != Some(c));
            out.push((i..end, CodeToken::String));
            i = end;
            continue;
        }
        if lang.labels && matches!(c, '[' | '(' | '{' | '|') && prev.is_some_and(|p| is_word(p) || p == '>') {
            let (close, closer) = match c {
                '[' => (']', 0),
                '(' => (')', 1),
                '{' => ('}', 2),
                _ => ('|', 3),
            };
            if i >= no_closer_until[closer] {
                let end = line_end(i);
                if let Some(k) = code[i + 1..end].find(close) {
                    let label = i + 1..i + 1 + k;
                    if !label.is_empty() {
                        out.push((label, CodeToken::String));
                    }
                    i += 1 + k + 1;
                    continue;
                }
                no_closer_until[closer] = end;
            }
        }
        if c.is_ascii_digit() && !prev.is_some_and(is_word) {
            let date_mark = |ch: char| lang.dates && matches!(ch, '-' | ':' | '+');
            let len = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || date_mark(ch)))
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

/// How long the Rust character literal is that `rest` starts with, `'` included; `None` where the `'` starts a lifetime
/// or a label, as `'a`, `'static` or `'outer:`, which hold no closing `'` behind one character or escape.
fn rust_char_len(rest: &str) -> Option<usize> {
    let body = rest.strip_prefix('\'')?;
    let len = match body.chars().next()? {
        '\\' => {
            let escape = &body[1..];
            1 + if let Some(hex) = escape.strip_prefix("u{") {
                let digits = hex.find('}').filter(|&n| (1..=6).contains(&n))?;
                hex[..digits]
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                    .then_some("u{}".len() + digits)?
            } else if let Some(hex) = escape.strip_prefix('x') {
                hex.get(..2).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
                3
            } else {
                matches!(escape.chars().next()?, 'n' | 'r' | 't' | '\\' | '0' | '\'' | '"').then_some(1)?
            }
        }
        '\'' | '\n' | '\r' => return None,
        c => c.len_utf8(),
    };
    body[len..].starts_with('\'').then_some(1 + len + 1)
}

/// End of a quoted string starting at `start`, honouring backslash escapes where the string has them; unterminated
/// strings end with the line.
fn string_end(code: &str, start: usize, quote: char, escapes: bool) -> usize {
    let mut escaped = false;
    for (k, ch) in code[start + quote.len_utf8()..].char_indices() {
        let at = start + quote.len_utf8() + k;
        if escaped {
            escaped = false;
        } else if ch == '\\' && escapes {
            escaped = true;
        } else if ch == quote {
            return at + ch.len_utf8();
        } else if ch == '\n' && quote != '`' {
            return at;
        }
    }
    code.len()
}

/// How long the TOML key or table header is that starts at `at`: a key where `=` follows it, at the start of a line or
/// of an entry of an inline table, or a header that has its line to itself but for a comment.
fn toml_key_or_header(code: &str, at: usize, line_start: bool) -> Option<usize> {
    let rest = &code[at..];
    if line_start && rest.starts_with('[') {
        let open = if rest.starts_with("[[") { 2 } else { 1 };
        let inner = rest[open..].trim_start_matches([' ', '\t']);
        let key_at = rest.len() - inner.len();
        let key_end = key_at + toml_key_len(inner)?;
        let close_at = key_end + (rest[key_end..].len() - rest[key_end..].trim_start_matches([' ', '\t']).len());
        let len = close_at + open;
        let closed = rest[close_at..].starts_with(&"]]"[..open]);
        let after = rest[len.min(rest.len())..].trim_start_matches([' ', '\t', '\r']);
        return (closed && (after.is_empty() || after.starts_with(['\n', '#']))).then_some(len);
    }
    let starts_key = rest.starts_with(|ch: char| toml_bare(ch) || ch == '"' || ch == '\'');
    if !starts_key || !(line_start || code[..at].trim_end_matches([' ', '\t']).ends_with(['{', ','])) {
        return None;
    }
    let len = toml_key_len(rest)?;
    let after = rest[len..].trim_start_matches([' ', '\t']);
    (after.starts_with('=') && !after.starts_with("==")).then_some(len)
}

/// How long the dotted key is that `rest` starts with: bare and quoted parts, the dots between them and the blanks
/// around the dots, all on one line.
fn toml_key_len(rest: &str) -> Option<usize> {
    let mut at = 0;
    loop {
        let part = &rest[at..];
        at += match part.chars().next()? {
            quote @ ('"' | '\'') => toml_quoted_len(part, quote)?,
            ch if toml_bare(ch) => part.find(|ch: char| !toml_bare(ch)).unwrap_or(part.len()),
            _ => return None,
        };
        let after = rest[at..].trim_start_matches([' ', '\t']);
        let Some(next) = after.strip_prefix('.') else {
            return Some(at);
        };
        at = rest.len() - next.trim_start_matches([' ', '\t']).len();
    }
}

fn toml_bare(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

/// How long the quoted key part is that `part` starts with, if it closes on its line; only `"` knows escapes.
fn toml_quoted_len(part: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (k, ch) in part.char_indices().skip(1) {
        match ch {
            '\n' => return None,
            _ if escaped => escaped = false,
            '\\' if quote == '"' => escaped = true,
            _ if ch == quote => return Some(k + 1),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_is_found_behind_opening_marks_of_other_kinds_without_a_closing_mark() {
        let found = tokens_of("mermaid", "a[b(c[d]");
        assert_eq!(found, [("b(c[d", CodeToken::String)]);
        let unclosed = tokens_of("mermaid", "a[b\nc(d)");
        assert_eq!(unclosed, [("d", CodeToken::String)]);
    }

    fn tokens_of<'a>(info: &str, code: &'a str) -> Vec<(&'a str, CodeToken)> {
        tokens(info, code).into_iter().map(|(r, t)| (&code[r], t)).collect()
    }

    #[test]
    fn each_family_of_languages_finds_its_keywords() {
        for (info, code, keyword) in [
            ("bash", "if true; then x; fi", "if"),
            ("sh", "for a in b; do c; done", "for"),
            ("go", "func main() {}", "func"),
            ("golang", "func main() {}", "func"),
            ("js", "const a = 1", "const"),
            ("javascript", "function f() {}", "function"),
            ("py", "import os", "import"),
            ("rs", "let x = 1;", "let"),
            ("sql", "SELECT a FROM b", "SELECT"),
            ("mysql", "select a from b", "select"),
            ("mermaid", "flowchart TD", "flowchart"),
            ("toml", "a = true", "true"),
        ] {
            assert!(
                tokens_of(info, code).contains(&(keyword, CodeToken::Keyword)),
                "{info}: {:?}",
                tokens_of(info, code)
            );
        }
    }

    #[test]
    fn a_language_is_named_by_the_first_word_of_the_info_string_in_any_case() {
        assert!(known("RUST title=\"x\""));
        assert!(known("python"));
        assert!(!known("gibtsnicht"));
        assert!(!known(""));
        assert!(tokens("gibtsnicht", "fn x").is_empty());
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
    fn rust_tells_lifetimes_and_labels_from_character_literals() {
        let strings = |code: &str| -> Vec<String> {
            tokens_of("rust", code)
                .into_iter()
                .filter(|(_, t)| *t == CodeToken::String)
                .map(|(s, _)| s.to_string())
                .collect()
        };
        for code in [
            "struct S<'a, 'b> { x: &'a str }",
            "fn f(x: &'static str) {}",
            "'outer: loop { break 'outer; }",
            "impl<'a> T for &'a U {}",
            r"'\u{1234567}'",
            r"'\xZZ'",
            r"'\q'",
            "''",
        ] {
            assert_eq!(strings(code), Vec::<String>::new(), "{code}");
        }
        for (code, literal) in [
            ("let c = 'a';", "'a'"),
            ("let c = 'ä';", "'ä'"),
            (r"let c = '\n';", r"'\n'"),
            (r"let c = '\'';", r"'\''"),
            (r"let c = '\\';", r"'\\'"),
            (r"let c = '\x7F';", r"'\x7F'"),
            (r"let c = '\u{1F600}';", r"'\u{1F600}'"),
            ("let b = b'x';", "'x'"),
            ("f::<'a>('y')", "'y'"),
        ] {
            assert_eq!(strings(code), [literal], "{code}");
        }
        assert_eq!(rust_char_len("'"), None);
        assert_eq!(rust_char_len(r"'\u{}'"), None);
        assert_eq!(rust_char_len(r"'\x7"), None);
    }

    #[test]
    fn toml_keys_and_tables_are_keywords_and_dates_are_numbers() {
        let t = tokens_of(
            "toml",
            "[server] # Haupt\nhost = \"localhost\"\nport = 8080\nwhen = 1979-05-27T07:32:00-08:00\non = true\n[[bin]]\n\
             site.\"example.org\" = { name = 'x', max-age = 1e-3 }\n",
        );
        assert_eq!(
            t,
            vec![
                ("[server]", CodeToken::Keyword),
                ("# Haupt", CodeToken::Comment),
                ("host", CodeToken::Keyword),
                ("\"localhost\"", CodeToken::String),
                ("port", CodeToken::Keyword),
                ("8080", CodeToken::Number),
                ("when", CodeToken::Keyword),
                ("1979-05-27T07:32:00-08:00", CodeToken::Number),
                ("on", CodeToken::Keyword),
                ("true", CodeToken::Keyword),
                ("[[bin]]", CodeToken::Keyword),
                ("site.\"example.org\"", CodeToken::Keyword),
                ("name", CodeToken::Keyword),
                ("'x'", CodeToken::String),
                ("max-age", CodeToken::Keyword),
                ("1e-3", CodeToken::Number),
            ]
        );
    }

    #[test]
    fn toml_literal_strings_know_no_escapes_and_triple_quotes_span_lines() {
        let t = tokens_of(
            "toml",
            "path = 'C:\\dir\\' # c\ntext = \"\"\"\nsays \"hi\"\n\"\"\"\nraw = '''\n\\n\n'''\n",
        );
        assert!(t.contains(&("'C:\\dir\\'", CodeToken::String)), "{t:?}");
        assert!(t.contains(&("# c", CodeToken::Comment)), "{t:?}");
        assert!(t.contains(&("\"\"\"\nsays \"hi\"\n\"\"\"", CodeToken::String)), "{t:?}");
        assert!(t.contains(&("'''\n\\n\n'''", CodeToken::String)), "{t:?}");
    }

    #[test]
    fn toml_rows_of_an_array_are_neither_keys_nor_tables() {
        let t = tokens_of("toml", "x = [\n  [1, 2],\n  \"a\", { b = 1 },\n]\n[t] x\n");
        assert_eq!(
            t,
            vec![
                ("x", CodeToken::Keyword),
                ("1", CodeToken::Number),
                ("2", CodeToken::Number),
                ("\"a\"", CodeToken::String),
                ("b", CodeToken::Keyword),
                ("1", CodeToken::Number),
            ]
        );
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
