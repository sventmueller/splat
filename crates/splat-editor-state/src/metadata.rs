//! The rows of the metadata form as values of the front matter: a list as one line of text, and what a line comes to
//! again.

use crate::markdown::front_matter::Value;

/// Fields that static site generators and Obsidian read as lists, so that a new one with commas becomes one.
const LIST_KEYS: [&str; 6] = ["tags", "aliases", "keywords", "categories", "authors", "cssclasses"];

/// A list as one line: its values with commas between them, those holding a comma or a quote in quotes.
pub fn line_of(value: &Value) -> String {
    match value {
        Value::Text(text) | Value::Complex(text) => text.clone(),
        Value::List(items) => items
            .iter()
            .map(|item| {
                if item.contains(',') || item.contains('"') {
                    format!("\"{}\"", item.replace('"', "\"\""))
                } else {
                    item.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The values of such a line.
fn items_of(line: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                item.push('"');
                chars.next();
            }
            '"' if item.trim().is_empty() || quoted => {
                quoted = !quoted;
                if quoted {
                    item.clear();
                }
            }
            ',' if !quoted => {
                items.push(item.trim().to_string());
                item.clear();
            }
            c => item.push(c),
        }
    }
    items.push(item.trim().to_string());
    items.retain(|item| !item.is_empty());
    items
}

/// What a row of the form comes to: a field keeps what it was, and a list that stays as it was has no change at all.
fn value_of(old: Option<&Value>, key: &str, line: &str) -> Value {
    match old {
        Some(Value::Complex(_)) => old.cloned().unwrap_or_else(|| Value::Text(line.to_string())),
        Some(Value::List(_)) => Value::List(items_of(line)),
        Some(Value::Text(_)) => Value::Text(line.to_string()),
        None if LIST_KEYS.contains(&key.to_ascii_lowercase().as_str()) => Value::List(items_of(line)),
        None => Value::Text(line.to_string()),
    }
}

/// The fields a form's rows ask for, or `None` if two rows name the same field.
pub fn wanted_fields(rows: &[(String, String, Option<Value>)]) -> Option<Vec<(String, Value)>> {
    let mut wanted: Vec<(String, Value)> = Vec::with_capacity(rows.len());
    for (key, line, old) in rows {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        if wanted.iter().any(|(other, _)| other == key) {
            return None;
        }
        wanted.push((key.to_string(), value_of(old.as_ref(), key, line)));
    }
    Some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_reads_back_from_its_line() {
        let list = Value::List(vec!["a".to_string(), "b, c".to_string(), "d\"e".to_string()]);
        let line = line_of(&list);
        assert_eq!(line, "a, \"b, c\", \"d\"\"e\"");
        assert_eq!(value_of(Some(&list), "tags", &line), list);
        assert_eq!(items_of(" a ,, b "), ["a", "b"]);
        assert_eq!(items_of(""), Vec::<String>::new());
    }

    #[test]
    fn a_row_stays_the_kind_of_field_it_was() {
        let text = Value::Text("alt".to_string());
        assert_eq!(value_of(Some(&text), "title", "a, b"), Value::Text("a, b".to_string()));
        let complex = Value::Complex("x:\n  y: 1".to_string());
        assert_eq!(value_of(Some(&complex), "x", "was anderes"), complex);
    }

    #[test]
    fn a_new_field_with_a_name_for_lists_becomes_one() {
        assert_eq!(
            value_of(None, "Tags", "a, b"),
            Value::List(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(value_of(None, "title", "a, b"), Value::Text("a, b".to_string()));
    }

    #[test]
    fn a_row_without_a_name_is_gone_and_two_of_a_name_are_refused() {
        let row = |key: &str, line: &str| (key.to_string(), line.to_string(), None);
        assert_eq!(
            wanted_fields(&[row("a", "1"), row(" ", "2"), row(" b ", "3")]),
            Some(vec![
                ("a".to_string(), Value::Text("1".to_string())),
                ("b".to_string(), Value::Text("3".to_string()))
            ])
        );
        assert_eq!(wanted_fields(&[row("a", "1"), row("a", "2")]), None);
    }
}
