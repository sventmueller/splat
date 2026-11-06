//! The license texts of everything the app contains, which their licenses ask to go along with it: Splat's own, those
//! of the crates in its program, of the scripts in resources/js and of Sparkle.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::*;

/// The file in Contents/Resources.
pub(crate) const ACKNOWLEDGEMENTS: &str = "Acknowledgements.txt";

/// The disk image's program runs on both, and a crate may be a dependency on one of them alone.
const TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

/// Of the licenses a crate offers as alternatives, Splat takes the first one of these.
const PREFERRED: [&str; 8] = [
    "MIT",
    "Zlib",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "0BSD",
    "Unlicense",
    "Apache-2.0",
];

/// A phrase only the full text of a license holds. BSD stands for both of its kinds.
const MARKS: [(&str, &str); 9] = [
    (
        "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION",
        "Apache-2.0",
    ),
    ("Boost Software License", "BSL-1.0"),
    ("UNICODE LICENSE V3", "Unicode-3.0"),
    ("UNICODE, INC. LICENSE AGREEMENT", "Unicode-DFS-2016"),
    ("Permission is hereby granted, free of charge", "MIT"),
    ("Redistribution and use in source and binary forms", "BSD"),
    ("provided 'as-is'", "Zlib"),
    (
        "free and unencumbered software released into the public domain",
        "Unlicense",
    ),
    (
        "Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee",
        "0BSD",
    ),
];

/// A crate in the program, from a line of `cargo tree` in the format `{p}|{l}|{r}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Crate {
    name: String,
    version: String,
    path: Option<PathBuf>,
    license: String,
    repository: String,
}

fn parse_crate(line: &str) -> Option<Crate> {
    let mut fields = line.split('|');
    let package = fields.next()?;
    let license = fields.next()?.trim().to_string();
    let repository = fields.next().unwrap_or("").trim().to_string();
    let (name, rest) = package.split_once(" v")?;
    let mut rest = rest.splitn(2, ' ');
    let version = rest
        .next()
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))?
        .to_string();
    let path = rest
        .next()
        .and_then(|p| p.strip_prefix('(')?.strip_suffix(')'))
        .filter(|p| p.starts_with('/'))
        .map(PathBuf::from);
    Some(Crate {
        name: name.to_string(),
        version,
        path,
        license,
        repository,
    })
}

/// The crates linked into the program for either processor, without the workspace's own.
fn crates(root: &Path) -> Result<Vec<Crate>, String> {
    let mut found = BTreeSet::new();
    for target in TARGETS {
        let tree = stdout(Command::new(env!("CARGO")).current_dir(root).args([
            "tree",
            "--locked",
            "--package",
            PACKAGE,
            "--edges",
            "normal,no-proc-macro",
            "--target",
            target,
            "--prefix",
            "none",
            "--no-dedupe",
            "--format",
            "{p}|{l}|{r}",
        ]))?;
        for line in tree.lines().filter(|l| !l.trim().is_empty()) {
            found.insert(parse_crate(line).ok_or_else(|| format!("cargo tree: {line:?}"))?);
        }
    }
    let own = |c: &Crate| {
        c.path
            .as_ref()
            .is_some_and(|p| p.starts_with(root) && !p.starts_with(root.join("vendor")))
    };
    Ok(found.into_iter().filter(|c| !own(c)).collect())
}

/// Where cargo unpacked a crate from the registry, or the folder of a path dependency.
fn source(c: &Crate) -> Result<PathBuf, String> {
    if let Some(path) = &c.path {
        return Ok(path.clone());
    }
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")))
        .ok_or("neither CARGO_HOME nor HOME is set")?;
    let registries = home.join("registry/src");
    files::read_dir(&registries)?
        .flatten()
        .map(|registry| registry.path().join(format!("{}-{}", c.name, c.version)))
        .find(|dir| dir.is_dir())
        .ok_or_else(|| format!("{} {}: no source in {}", c.name, c.version, registries.display()))
}

/// The licenses a crate is used under: the preferred one of each set of alternatives in its SPDX expression, and every
/// license it requires together with another.
fn chosen(expression: &str) -> Result<Vec<String>, String> {
    let not_understood = || format!("license {expression:?} is not understood");
    let required: Vec<&str> = expression.split(" AND ").map(str::trim).collect();
    required
        .iter()
        .map(|part| {
            let alternatives = match part.strip_prefix('(').and_then(|p| p.strip_suffix(')')) {
                Some(inner) => inner,
                // SPDX binds AND before OR, so `A OR B AND C` is not a choice between all three.
                None if required.len() > 1 && part.contains(" OR ") => return Err(not_understood()),
                None => part,
            };
            let options: Vec<&str> = alternatives
                .split(" OR ")
                .flat_map(|o| o.split('/'))
                .map(str::trim)
                .collect();
            let valid = |o: &&str| !o.is_empty() && o.chars().all(|c| c.is_ascii_alphanumeric() || "-.+".contains(c));
            if !options.iter().all(valid) {
                return Err(not_understood());
            }
            let taken = PREFERRED
                .iter()
                .find(|p| options.contains(p))
                .map_or(options[0], |p| *p);
            Ok(taken.to_string())
        })
        .collect()
}

fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Which license a file holds the full text of. The phrase that comes first decides, since a file may go on with the
/// licenses of code its crate took in, and the Boost license repeats MIT's phrase after its name.
fn kind(text: &str) -> Option<&'static str> {
    let text = words(text);
    let (at, license) = MARKS
        .iter()
        .filter_map(|(mark, license)| text.find(mark).map(|at| (at, *license)))
        .min_by_key(|(at, _)| *at)?;
    if license != "BSD" {
        return Some(license);
    }
    let conditions = &text[at..];
    let conditions = conditions
        .find("THIS SOFTWARE IS PROVIDED")
        .map_or(conditions, |end| &conditions[..end]);
    Some(if conditions.contains("Neither the name") {
        "BSD-3-Clause"
    } else {
        "BSD-2-Clause"
    })
}

/// The names in the `authors` of a crate's manifest, without their addresses.
fn authors(manifest: &str) -> Vec<String> {
    let Some((_, rest)) = manifest.split_once("\nauthors = [") else {
        return Vec::new();
    };
    let list = rest.split(']').next().unwrap_or("");
    list.split('"')
        .skip(1)
        .step_by(2)
        .map(|author| author.split(" <").next().unwrap_or(author).trim().to_string())
        .filter(|author| !author.is_empty())
        .collect()
}

/// The texts a crate is used under, from its own license files. A crate that names MIT and ships no text of it gets
/// Splat's MIT text with the holders its manifest names.
fn texts(c: &Crate, dir: &Path, mit_terms: &str) -> Result<Vec<String>, String> {
    let mut paths: Vec<PathBuf> = files::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let mut licenses = Vec::new();
    let mut notices = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default();
        if ["LICENSE", "LICENCE", "COPYING", "UNLICENSE"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            let text = files::read_to_string(&path)?;
            if let Some(license) = kind(&text) {
                licenses.push((license, text));
            }
        } else if name.starts_with("NOTICE") {
            notices.push(files::read_to_string(&path)?);
        }
    }
    // Unicode-3.0 replaced Unicode-DFS-2016, and crates name the one while shipping the other.
    let same = |a: &str, b: &str| a == b || (a.starts_with("Unicode-") && b.starts_with("Unicode-"));
    let mut out: Vec<String> = Vec::new();
    for license in chosen(&c.license)? {
        let before = out.len();
        for (_, text) in licenses.iter().filter(|(l, _)| same(l, &license)) {
            if !out.iter().any(|t| words(t) == words(text)) {
                out.push(text.clone());
            }
        }
        if out.len() == before {
            if license != "MIT" {
                return Err(format!(
                    "{} {}: no {license} text in {}",
                    c.name,
                    c.version,
                    dir.display()
                ));
            }
            let named = authors(&files::read_to_string(dir.join("Cargo.toml"))?);
            let holders = if !named.is_empty() {
                named.join(", ")
            } else if !c.repository.is_empty() {
                format!("the authors of {}", c.repository)
            } else {
                format!("the authors of {}", c.name)
            };
            out.push(format!("Copyright (c) {holders}\n\n{mit_terms}"));
        }
        // Apache 2.0 asks for a NOTICE file to go along; the other licenses do not.
        if license == "Apache-2.0" {
            out.extend(notices.iter().cloned());
        }
    }
    Ok(out)
}

/// The scripts and fonts in resources/js, each with its license as `LICENSE-<name>` there and its version from
/// vendor.lock where the name is a package.
fn scripts(root: &Path) -> Result<Vec<(String, String)>, String> {
    let lock = files::read_to_string(root.join("vendor.lock"))?;
    let entries = lock_entries(&lock)?;
    let mut out = Vec::new();
    for entry in files::read_dir(root.join("resources/js"))?.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        let Some(name) = file.strip_prefix("LICENSE-") else {
            continue;
        };
        let label = match entries.iter().find(|e| e[1] == name) {
            Some(e) => format!("{name} {}", e[2]),
            None => name.to_string(),
        };
        out.push((label, files::read_to_string(entry.path())?));
    }
    Ok(out)
}

/// A license text as written, without the blank lines and trailing spaces around and in it.
fn tidy(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_string()
}

/// The text of `ACKNOWLEDGEMENTS`: Splat's license first, then each license text once, under the components it covers.
/// Sparkle's license comes from `sparkle`, the unpacked release, when the bundle carries the framework.
pub(crate) fn acknowledgements(sparkle: Option<&Path>) -> Result<String, String> {
    let root = root();
    let mit = files::read_to_string(root.join("LICENSE-MIT"))?;
    let mit_terms = mit
        .find("Permission is hereby granted")
        .map(|start| &mit[start..])
        .ok_or("LICENSE-MIT holds no MIT terms")?;
    let mut covered: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in crates(&root)? {
        let dir = source(&c)?;
        let text = texts(&c, &dir, mit_terms)?
            .iter()
            .map(|t| tidy(t))
            .collect::<Vec<_>>()
            .join("\n\n");
        covered
            .entry(text)
            .or_default()
            .push(format!("{} {}", c.name, c.version));
    }
    let mut others = scripts(&root)?;
    if let Some(sparkle) = sparkle {
        others.push((
            format!("Sparkle {SPARKLE_VERSION}"),
            files::read_to_string(sparkle.join("LICENSE"))?,
        ));
    }
    for (label, text) in others {
        covered.entry(tidy(&text)).or_default().push(label);
    }
    let mut sections: Vec<(Vec<String>, String)> = covered
        .into_iter()
        .map(|(text, mut names)| {
            names.sort_by_key(|n| n.to_lowercase());
            (names, text)
        })
        .collect();
    sections.sort_by_key(|(names, _)| names[0].to_lowercase());
    let own = [tidy(&mit), tidy(&files::read_to_string(root.join("LICENSE-APACHE"))?)].join("\n\n");
    let mut out = format!("## {APP}\n\n{own}\n");
    for (names, text) in sections {
        out.push_str(&format!("\n## {}\n\n{text}\n", names.join(", ")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_of_cargo_tree_names_the_crate_its_folder_and_its_license() {
        let registry = parse_crate("objc2 v0.6.5|MIT|https://github.com/madsmtm/objc2").unwrap();
        assert_eq!(
            (
                registry.name.as_str(),
                registry.version.as_str(),
                registry.path,
                registry.license.as_str()
            ),
            ("objc2", "0.6.5", None, "MIT")
        );
        let vendored = parse_crate("comrak v0.59.0 (/r/vendor/comrak)|BSD-2-Clause|").unwrap();
        assert_eq!(vendored.path, Some(PathBuf::from("/r/vendor/comrak")));
        assert!(parse_crate("no version|MIT|").is_none() && parse_crate("objc2").is_none());
    }

    #[test]
    fn of_alternatives_the_preferred_license_is_taken_and_of_requirements_all() {
        assert_eq!(chosen("Apache-2.0 OR MIT").unwrap(), ["MIT"]);
        assert_eq!(chosen("MIT/Apache-2.0").unwrap(), ["MIT"]);
        assert_eq!(chosen("BSD-3-Clause OR Apache-2.0").unwrap(), ["BSD-3-Clause"]);
        assert_eq!(chosen("Apache-2.0 OR BSL-1.0").unwrap(), ["BSL-1.0"]);
        assert_eq!(
            chosen("(MIT OR Apache-2.0) AND Unicode-3.0").unwrap(),
            ["MIT", "Unicode-3.0"]
        );
        assert_eq!(chosen("EPL-2.0 OR GPL-2.0").unwrap(), ["EPL-2.0"]);
        assert!(chosen("MIT OR Apache-2.0 AND Unicode-3.0").is_err());
        assert!(chosen("Apache-2.0 WITH LLVM-exception").is_err());
        assert!(chosen("").is_err());
    }

    #[test]
    fn a_license_file_is_known_by_its_text_and_boost_is_not_taken_for_mit() {
        assert_eq!(
            kind("Permission is hereby granted,\nfree of charge, to any"),
            Some("MIT")
        );
        assert_eq!(
            kind("Boost Software License\n\nPermission is hereby granted, free of charge"),
            Some("BSL-1.0")
        );
        assert_eq!(
            kind(
                "Redistribution and use in source and binary forms\n3. Neither the name of\nTHIS SOFTWARE IS PROVIDED"
            ),
            Some("BSD-3-Clause")
        );
        assert_eq!(
            kind(
                "Redistribution and use in source and binary forms\nTHIS SOFTWARE IS PROVIDED\n-----\n\
                 Permission is hereby granted, free of charge\nNeither the name of"
            ),
            Some("BSD-2-Clause"),
            "a BSD text with the MIT text of code taken in"
        );
        assert_eq!(
            kind("Licensed under the Apache License, Version 2.0 or the MIT license"),
            None
        );
    }

    #[test]
    fn the_authors_of_a_manifest_lose_their_addresses() {
        let manifest =
            "[package]\nname = \"x\"\nauthors = [\n    \"Mads Marquart <mads@marquart.dk>\",\n    \"Mary\",\n]\n";
        assert_eq!(authors(manifest), ["Mads Marquart", "Mary"]);
        assert!(authors("[package]\nname = \"x\"\n").is_empty());
    }

    #[test]
    fn a_license_text_keeps_its_lines_and_loses_the_blank_ones_around_it() {
        assert_eq!(
            tidy("\n\r\n   Apache License  \r\n\n  1. Definitions.\t\n\n"),
            "   Apache License\n\n  1. Definitions."
        );
    }

    #[test]
    fn every_component_of_the_program_comes_with_its_license_text() {
        let text = acknowledgements(None).unwrap();
        assert!(text.starts_with("## Splat\n\nMIT License\n\nCopyright (c) 2026 Sven Müller"));
        let section = |name: &str| {
            text.split("\n## ")
                .find(|s| {
                    s.lines()
                        .next()
                        .is_some_and(|names| names.split(", ").any(|n| n.starts_with(name)))
                })
                .map(words)
                .unwrap_or_else(|| panic!("no section names {name}"))
        };
        let bsd = section("comrak ");
        assert!(bsd.contains("Comrak contributors") && bsd.contains("binary forms, with or without modification"));
        assert!(section("objc2-app-kit ").contains("Copyright (c) the authors of https://github.com/madsmtm/objc2"));
        assert!(section("emojis ").contains("UNICODE LICENSE V3"));
        assert!(section("ryu ").contains("Boost Software License"));
        assert!(section("mathjax ").contains("Apache License"));
        assert!(section("graphviz").contains("Eclipse Public License"));
        let names: Vec<&str> = text
            .lines()
            .filter_map(|l| l.strip_prefix("## "))
            .flat_map(|l| l.split(", "))
            .collect();
        assert!(names.len() > 100, "{} components", names.len());
        assert!(
            !names.iter().any(|n| n.starts_with("splat-")),
            "the workspace's crates are Splat"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("syn ")),
            "a crate of a procedural macro is not in the program"
        );
    }
}
