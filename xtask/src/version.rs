//! The one version of the project: `[workspace.package]` in the root manifest, which every package inherits and from
//! which the bundles, the feed and the release commit take theirs.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::*;

/// The root manifest, relative to the repository.
pub(crate) const MANIFEST: &str = "Cargo.toml";

const LOCKFILE: &str = "Cargo.lock";

/// What a release changes and commits besides the image and the feed in `dist/`.
pub(crate) const VERSIONED: [&str; 2] = [MANIFEST, LOCKFILE];

/// Major, minor and build number, digits only.
pub(crate) fn is_version(v: &str) -> bool {
    numbers(v).is_some()
}

fn numbers(v: &str) -> Option<[u64; 3]> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3
        || !parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some([parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?])
}

/// The version of the project as the root manifest sets it.
pub(crate) fn version() -> Result<String, String> {
    let path = root().join(MANIFEST);
    let manifest = files::read_to_string(&path)?;
    let version =
        workspace_version(&manifest).ok_or_else(|| format!("{}: no version in [workspace.package]", path.display()))?;
    if is_version(version) {
        Ok(version.to_string())
    } else {
        Err(format!("{}: {version:?} is not major.minor.build", path.display()))
    }
}

/// The value of `key = "…"` in the section `[section]` of a manifest, with its byte offset.
fn manifest_value<'a>(manifest: &'a str, section: &str, key: &str) -> Option<(usize, &'a str)> {
    let mut offset = 0;
    let mut inside = false;
    for line in manifest.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == format!("[{section}]");
        } else if inside
            && let Some(rest) = trimmed.strip_prefix(key)
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim();
            let quoted = value.strip_prefix('"').and_then(|v| v.strip_suffix('"'));
            let start = quoted
                .and_then(|q| line.find(&format!("\"{q}\"")))
                .map_or(0, |at| at + 1);
            return Some((offset + start, quoted.unwrap_or(value)));
        }
        offset += line.len();
    }
    None
}

fn workspace_version(manifest: &str) -> Option<&str> {
    manifest_value(manifest, "workspace.package", "version").map(|(_, v)| v)
}

/// The manifest with `version` in place of the one `[workspace.package]` holds; nothing else changes.
pub(crate) fn with_version(manifest: &str, version: &str) -> Option<String> {
    let (at, old) = manifest_value(manifest, "workspace.package", "version")?;
    Some(format!("{}{version}{}", &manifest[..at], &manifest[at + old.len()..]))
}

/// The version a release gets: the set one when its major or minor number was raised by hand past the published
/// one, the next build number after the published one when both are the same.
pub(crate) fn release_version(set: &str, published: Option<&str>) -> Result<String, String> {
    let [major, minor, build] = numbers(set).ok_or_else(|| format!("{set:?} is not major.minor.build"))?;
    let Some(published) = published else {
        return Ok(set.to_string());
    };
    let shipped = numbers(published).ok_or_else(|| format!("the feed offers {published:?}"))?;
    if [major, minor] > [shipped[0], shipped[1]] {
        Ok(set.to_string())
    } else if [major, minor, build] == shipped {
        Ok(format!("{major}.{minor}.{}", build + 1))
    } else {
        Err(format!(
            "{MANIFEST} sets {set}, the feed offers {published}: set it back to {published}, or raise major or minor \
             past it"
        ))
    }
}

/// The version an update feed offers.
pub(crate) fn published_version(appcast: &str) -> Option<&str> {
    appcast.split("<sparkle:version>").nth(1)?.split('<').next()
}

/// The version set in the manifest until `keep` or a drop sets it back: a release that fails or is interrupted
/// leaves manifest and lockfile as they were.
pub(crate) struct Raised {
    before: Vec<(std::path::PathBuf, Vec<u8>)>,
    kept: bool,
}

impl Raised {
    pub(crate) fn to(version: &str) -> Result<Self, String> {
        let root = root();
        let before = VERSIONED
            .iter()
            .map(|name| Ok((root.join(name), files::read(root.join(name))?)))
            .collect::<Result<Vec<_>, String>>()?;
        let raised = Raised { before, kept: false };
        if self::version()? != version {
            let path = root.join(MANIFEST);
            let manifest = files::read_to_string(&path)?;
            let changed = with_version(&manifest, version).ok_or("no version in [workspace.package]")?;
            files::write(&path, changed)?;
            run(Command::new(env!("CARGO")).current_dir(&root).args([
                "update",
                "--workspace",
                "--offline",
                "--quiet",
            ]))?;
            println!("version {version}");
        }
        Ok(raised)
    }

    pub(crate) fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for Raised {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        for (path, content) in &self.before {
            if let Err(e) = fs::write(path, content) {
                eprintln!(
                    "error: {}: {e}; restore it with `git checkout -- {}`",
                    path.display(),
                    path.display()
                );
            }
        }
    }
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn note_interrupt(_: i32) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

unsafe extern "C" {
    fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
}

/// Turns Control-C, a closed terminal and `kill` into an error of the running step, so that `Raised` can set the
/// version back; the programs xtask starts still stop, as a caught signal returns to its default in them.
pub(crate) fn catch_interrupts() {
    const SIGHUP: i32 = 1;
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    for sig in [SIGHUP, SIGINT, SIGTERM] {
        // SAFETY: the handler only stores into an atomic, which is safe in a signal handler.
        unsafe { signal(sig, note_interrupt) };
    }
}

pub(crate) fn interrupted() -> Result<(), String> {
    if INTERRUPTED.load(Ordering::SeqCst) {
        Err("interrupted".into())
    } else {
        Ok(())
    }
}

/// Stops a release whose commit would take along changes that are not its own.
pub(crate) fn require_committed(paths: &[&str]) -> Result<(), String> {
    let changed = stdout(
        Command::new("git")
            .current_dir(root())
            .args(["status", "--porcelain", "--"])
            .args(paths),
    )?;
    if changed.trim().is_empty() {
        Ok(())
    } else {
        Err(format!(
            "commit or discard these first, the release commits them with its version:\n{changed}"
        ))
    }
}

/// Commits the raised version and what the release wrote into `dist/`, and nothing else.
pub(crate) fn commit_release(version: &str) -> Result<(), String> {
    let paths = [MANIFEST, LOCKFILE, "dist"];
    run(Command::new("git")
        .current_dir(root())
        .args(["add", "--all", "--"])
        .args(paths))?;
    run(Command::new("git")
        .current_dir(root())
        .args([
            "commit",
            "--quiet",
            "--message",
            &format!("Release {APP} {version}"),
            "--",
        ])
        .args(paths))
}

/// Every place in the repository that names a version other than the manifest's, each as a line.
pub(crate) fn deviations(root: &Path) -> Result<Vec<String>, String> {
    let manifest = files::read_to_string(root.join(MANIFEST))?;
    let version = workspace_version(&manifest).ok_or("no version in [workspace.package]")?;
    let mut found = Vec::new();
    if root.join("VERSION").exists() {
        found.push("VERSION: the version is set in Cargo.toml alone".to_string());
    }
    let members = members(root)?;
    for (name, path) in &members {
        let text = files::read_to_string(path)?;
        let inherits = manifest_value(&text, "package", "version.workspace").is_some_and(|(_, v)| v == "true");
        if !inherits || manifest_value(&text, "package", "version").is_some() {
            found.push(format!(
                "{}: {name} does not take version.workspace = true",
                path.display()
            ));
        }
    }
    let fuzz = root.join("fuzz").join(MANIFEST);
    if let Ok(text) = fs::read_to_string(&fuzz)
        && let Some((_, v)) = manifest_value(&text, "package", "version")
    {
        found.push(format!(
            "{}: version {v:?}; outside the workspace it can only hold a copy",
            fuzz.display()
        ));
    }
    let lock = files::read_to_string(root.join(LOCKFILE))?;
    for entry in lock.split("[[package]]").skip(1) {
        let field = |key: &str| {
            entry
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{key} = \"")).and_then(|v| v.strip_suffix('"')))
        };
        if let (Some(name), Some(locked), None) = (field("name"), field("version"), field("source"))
            && members.iter().any(|(m, _)| m == name)
            && locked != version
        {
            found.push(format!("{LOCKFILE}: {name} {locked}"));
        }
    }
    let feed_path = root.join("dist/appcast.xml");
    if let Ok(feed) = fs::read_to_string(&feed_path) {
        let published = published_version(&feed).unwrap_or_default();
        for named in [
            format!("<title>{APP} {published}</title>"),
            format!("<sparkle:shortVersionString>{published}</sparkle:shortVersionString>"),
            format!("{DOWNLOADS}/{APP}-{published}.dmg\""),
        ] {
            if !feed.contains(&named) {
                found.push(format!("{}: offers {published} but lacks {named}", feed_path.display()));
            }
        }
        if !root.join("dist").join(format!("{APP}-{published}.dmg")).exists() {
            found.push(format!("dist/{APP}-{published}.dmg, which the feed offers, is missing"));
        }
        if published != version && release_version(version, Some(published)).as_deref() != Ok(version) {
            found.push(format!(
                "{MANIFEST}: {version}, but the feed offers {published}; only a raised major or minor number may \
                 stand ahead of it"
            ));
        }
    }
    Ok(found)
}

/// Name and manifest of each workspace member: the root package and what `members` of `[workspace]` names, with
/// `dir/*` for each folder in `dir` that holds a manifest.
fn members(root: &Path) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    let manifest = files::read_to_string(root.join(MANIFEST))?;
    let (_, listed) = manifest_value(&manifest, "workspace", "members").ok_or("no members in [workspace]")?;
    let mut paths = vec![root.join(MANIFEST)];
    for member in listed
        .trim_matches(['[', ']'])
        .split(',')
        .map(|m| m.trim().trim_matches('"'))
    {
        match member.strip_suffix("/*") {
            Some(dir) => {
                let mut found: Vec<_> = files::read_dir(root.join(dir))?
                    .flatten()
                    .map(|e| e.path().join(MANIFEST))
                    .filter(|p| p.exists())
                    .collect();
                found.sort();
                paths.extend(found);
            }
            None if !member.is_empty() => paths.push(root.join(member).join(MANIFEST)),
            None => {}
        }
    }
    paths
        .into_iter()
        .map(|path| {
            let text = files::read_to_string(&path)?;
            let (_, name) = manifest_value(&text, "package", "name")
                .ok_or_else(|| format!("{}: no package name", path.display()))?;
            Ok((name.to_string(), path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_have_three_numbers() {
        assert!(is_version("1.0.1") && is_version("12.3.456"));
        assert!(!is_version("1.0") && !is_version("1.0.x") && !is_version("1..1") && !is_version("1.0.1.2"));
    }

    #[test]
    fn every_place_in_the_repository_names_the_manifests_version() {
        let found = deviations(&root()).unwrap();
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    #[test]
    fn the_version_is_read_and_replaced_in_the_workspace_section_alone() {
        let manifest =
            "[package]\nversion = \"9.9.9\"\n\n[workspace.package]\nversion = \"1.0.17\"\nedition = \"2024\"\n";
        assert_eq!(workspace_version(manifest), Some("1.0.17"));
        assert_eq!(
            with_version(manifest, "1.0.18").unwrap(),
            manifest.replace("\"1.0.17\"", "\"1.0.18\"")
        );
        assert_eq!(workspace_version("[package]\nversion = \"1.0.0\"\n"), None);
        assert_eq!(
            manifest_value(
                "[package]
version.workspace = true
",
                "package",
                "version.workspace"
            )
            .map(|(_, v)| v),
            Some("true")
        );
    }

    #[test]
    fn a_release_raises_the_build_number_unless_major_or_minor_were_raised_by_hand() {
        assert_eq!(release_version("1.0.17", Some("1.0.17")).unwrap(), "1.0.18");
        assert_eq!(release_version("1.1.0", Some("1.0.17")).unwrap(), "1.1.0");
        assert_eq!(release_version("2.0.0", Some("1.9.40")).unwrap(), "2.0.0");
        assert_eq!(release_version("1.0.0", None).unwrap(), "1.0.0");
        assert!(release_version("1.0.18", Some("1.0.17")).is_err());
        assert!(release_version("1.0.16", Some("1.0.17")).is_err());
        assert!(release_version("1.0", Some("1.0.17")).is_err());
        assert_eq!(
            published_version("<sparkle:version>1.0.4</sparkle:version>"),
            Some("1.0.4")
        );
    }
}
