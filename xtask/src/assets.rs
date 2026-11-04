//! The icon and the vendored web libraries.

use std::path::Path;
use std::process::Command;

use crate::*;

pub(crate) const ICON_SIZES: [(u32, &str); 10] = [
    (16, "16x16"),
    (32, "16x16@2x"),
    (32, "32x32"),
    (64, "32x32@2x"),
    (128, "128x128"),
    (256, "128x128@2x"),
    (256, "256x256"),
    (512, "256x256@2x"),
    (512, "512x512"),
    (1024, "512x512@2x"),
];

/// Builds assets/AppIcon.icns and the README icon from assets/icon-1024.png, which must follow Apple's macOS icon
/// template outline.
pub(crate) fn icon() -> Result<(), String> {
    let assets = root().join("assets");
    let master = assets.join("icon-1024.png");
    if !master.exists() {
        return Err(format!("{} is missing", master.display()));
    }
    let iconset = std::env::temp_dir().join(format!("{PROGRAM}-AppIcon.iconset"));
    if iconset.exists() {
        files::remove_dir_all(&iconset)?;
    }
    files::create_dir_all(&iconset)?;
    for (size, name) in ICON_SIZES {
        let out = iconset.join(format!("icon_{name}.png"));
        run(Command::new("sips")
            .args(["-z", &size.to_string(), &size.to_string()])
            .arg(&master)
            .arg("--out")
            .arg(&out)
            .stdout(std::process::Stdio::null()))?;
    }
    run(Command::new("iconutil")
        .args(["-c", "icns"])
        .arg(&iconset)
        .arg("-o")
        .arg(assets.join("AppIcon.icns")))?;
    files::remove_dir_all(&iconset)?;
    // Markdown cannot size an image, so the README shows one drawn at the size it should appear. Apple's template
    // places the shape at 100 px with a transparent margin, which would indent the icon; its shadow reaches to 82 px
    // at the sides and 94 px at the top.
    let readme = assets.join("readme-icon.png");
    run(Command::new("sips")
        .args(["--cropToHeightWidth", "860", "860", "--cropOffset", "94", "82"])
        .arg(&master)
        .arg("--out")
        .arg(&readme)
        .stdout(std::process::Stdio::null()))?;
    run(Command::new("sips")
        .args(["-z", "168", "168"])
        .arg(&readme)
        .stdout(std::process::Stdio::null()))
}

pub(crate) fn sha256(path: &Path) -> Result<String, String> {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|e| format!("shasum: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| format!("no checksum for {}", path.display()))
}

/// Turns an ES module into a classic script that sets its exports as one global: the render page loads its scripts
/// from files, and WebKit refuses module imports from file URLs. The module keeps its own scope and strict mode.
pub(crate) fn classic_script(path: &Path) -> Result<(), String> {
    if path.extension().is_none_or(|e| e != "js") {
        return Ok(());
    }
    let text = files::read_to_string(path)?;
    let trimmed = text.trim_end();
    let Some(start) = trimmed.rfind("export{").or_else(|| trimmed.rfind("export {")) else {
        return Ok(());
    };
    let Some(list) = trimmed[start..]
        .split_once('{')
        .and_then(|(_, r)| r.split_once('}'))
        .map(|(l, _)| l)
    else {
        return Ok(());
    };
    let globals: Vec<String> = list
        .split(',')
        .filter_map(|item| {
            let mut parts = item.split(" as ").map(str::trim);
            let local = parts.next()?;
            let name = parts.next().unwrap_or(local);
            Some(format!("{name}:{local}"))
        })
        .collect();
    let package = path.file_stem().and_then(|s| s.to_str()).unwrap_or("module");
    let global = if package == "plantuml" { "PlantUML" } else { package };
    let classic = format!(
        "(function(){{\"use strict\";\n{}\nself.{global}={{{}}};\n}}).call(undefined);\n",
        &trimmed[..start],
        globals.join(",")
    );
    files::write(path, classic)
}

/// The entries of `vendor.lock`, one line of five fields each; anything else but a comment there is a mistake, and a
/// lock without entries would check nothing.
pub(crate) fn lock_entries(content: &str) -> Result<Vec<[&str; 5]>, String> {
    let mut entries = Vec::new();
    for (n, line) in content.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: [&str; 5] = line
            .split('\t')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| format!("vendor.lock line {}: expected five fields: {line}", n + 1))?;
        entries.push(fields);
    }
    if entries.is_empty() {
        return Err("vendor.lock lists nothing to check".into());
    }
    Ok(entries)
}

/// Checks the vendored scripts against `vendor.lock`, or downloads them from the recorded URLs first.
pub(crate) fn vendor(fetch: bool) -> Result<(), String> {
    let root = root();
    let lock = root.join("vendor.lock");
    let content = files::read_to_string(&lock)?;
    lock_entries(&content)?;
    let mut out = String::new();
    let mut mismatches = Vec::new();
    for line in content.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if line.starts_with('#') || fields.len() != 5 {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let (path, url, recorded) = (root.join(fields[0]), fields[3], fields[4]);
        if fetch {
            run(Command::new("curl")
                .args(["-fsSL", "--proto", "=https", "--proto-redir", "=https", "-o"])
                .arg(&path)
                .arg(url))?;
            classic_script(&path)?;
        }
        let actual = sha256(&path)?;
        if fetch {
            println!("{} {} {}", fields[1], fields[2], fields[0]);
        } else if actual != recorded {
            mismatches.push(fields[0].to_string());
        }
        out.push_str(&[fields[0], fields[1], fields[2], fields[3], &actual].join("\t"));
        out.push('\n');
    }
    if fetch {
        files::write(&lock, out)?;
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!("changed since vendor.lock: {}", mismatches.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_that_lists_nothing_or_something_broken_fails() {
        let entry = "resources/js/a.js\ta\t1\thttps://x/a.js\tabc";
        assert_eq!(lock_entries(&format!("# comment\n{entry}\n")).unwrap().len(), 1);
        assert!(lock_entries("# only a comment\n").is_err());
        assert!(lock_entries("").is_err());
        let broken = lock_entries(&format!("{entry}\nresources/js/b.js\tb\n")).unwrap_err();
        assert!(broken.contains("line 2"), "{broken}");
    }
}
