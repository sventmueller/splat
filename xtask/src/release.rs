//! The disk image, its notarization and the update feed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::*;

/// Created once with `xcrun notarytool store-credentials notary --apple-id <Apple ID> --team-id <team ID>`.
pub(crate) const NOTARY_PROFILE: &str = "notary";

/// What the release being built brings, written by hand before `cargo xtask dmg`.
const NOTES: &str = "RELEASE_NOTES.md";

const REPORT_LINE: &str =
    "Something wrong or missing? [Report it on GitHub](https://github.com/sventmueller/splat/issues/new/choose).";

/// Builds the release app signed with the Developer ID, has Apple notarize it, packs it into a disk image with a
/// link to /Applications and has Apple notarize the image too, so that Gatekeeper opens both on other Macs, each with
/// its ticket stapled. Only an image that passed all of this becomes `dist/Splat-<version>.dmg`, the copy that goes
/// into the repository: it replaces older ones there, `dist/appcast.xml` offers it to installed copies with the
/// notes of `RELEASE_NOTES.md`, and the build number goes up.
pub(crate) fn dmg() -> Result<PathBuf, String> {
    let version = version()?;
    let dist = root().join("dist");
    let feed_path = dist.join("appcast.xml");
    let previous_feed = fs::read_to_string(&feed_path).ok();
    let notes_path = root().join(NOTES);
    let written = fs::read_to_string(&notes_path).map_err(|e| format!("{}: {e}", notes_path.display()))?;
    let notes = release_notes(&written, previous_feed.as_deref())?;
    let (identity, name) = developer_id()?;
    // Checks the saved login before the release build, which takes minutes.
    run(Command::new("xcrun")
        .args(["notarytool", "history", "--keychain-profile", NOTARY_PROFILE])
        .stdout(std::process::Stdio::null()))
    .map_err(|e| {
        format!(
            "{e}\nno usable notarization login in the keychain profile \"{NOTARY_PROFILE}\"; create it with \
             `xcrun notarytool store-credentials {NOTARY_PROFILE} --apple-id <Apple ID> --team-id <team ID>`"
        )
    })?;
    let tools = sparkle()?.join("bin");
    let generate_keys = tools.join("generate_keys");
    let key = stdout(Command::new(&generate_keys).arg("-p")).map_err(|e| {
        format!(
            "{e}\nno update signing key in the keychain; import its backup with `{} -f <file>`",
            generate_keys.display()
        )
    })?;
    if key.trim() != UPDATE_KEY {
        return Err(format!(
            "the keychain's update signing key {} is not UPDATE_KEY {UPDATE_KEY}",
            key.trim()
        ));
    }
    let app = assemble(true, false, &version, true)?;
    // A coverage build, as a manual `cargo llvm-cov` run can leave, would write measurement files on users' Macs.
    let load_commands = stdout(
        Command::new("otool")
            .args(["-arch", "all", "-l"])
            .arg(app.join("Contents/MacOS").join(APP)),
    )?;
    if load_commands.contains("__llvm_prf") {
        return Err("the release binary carries coverage counters; run `cargo clean` and try again".into());
    }
    check_universal(&app)?;
    println!("signing {version} with {name}");
    sign(&app, &identity, true, None)?;
    run(Command::new("codesign").args(["--verify", "--strict"]).arg(&app))?;
    // The image's ticket reaches Gatekeeper only when the image is opened in the Finder; a copy taken out otherwise,
    // as Homebrew installs it, would need a network connection on first launch.
    let archive = app.with_extension("zip");
    run(Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&app)
        .arg(&archive))?;
    notarize(&archive)?;
    files::remove_file(&archive)?;
    run(Command::new("xcrun").args(["stapler", "staple"]).arg(&app))?;
    let image = disk_image(&app)?;
    run(Command::new("codesign")
        .args(["--force", "--sign", &identity, "--timestamp"])
        .arg(&image))?;
    notarize(&image)?;
    run(Command::new("xcrun").args(["stapler", "staple"]).arg(&image))?;
    run(Command::new("spctl")
        .args([
            "--assess",
            "--type",
            "open",
            "--context",
            "context:primary-signature",
            "--verbose",
        ])
        .arg(&image))?;
    files::create_dir_all(&dist)?;
    let file = format!("{APP}-{version}.dmg");
    let published = dist.join(&file);
    let signature = stdout(Command::new(tools.join("sign_update")).arg(&image))?;
    let date = stdout(
        Command::new("date")
            .env("LC_ALL", "C")
            .args(["-u", "+%a, %d %b %Y %H:%M:%S +0000"]),
    )?;
    // Copied rather than renamed: target/ may be a link to another volume.
    fs::copy(&image, &published).map_err(|e| format!("{}: {e}", published.display()))?;
    files::remove_file(&image)?;
    // GitHub's raw server caches the feed for up to five minutes, so the image the old feed offers stays one release.
    let offered = previous_feed.as_deref().and_then(offered_image).map(str::to_string);
    let feed = appcast(&version, &file, signature.trim(), date.trim(), &notes)?;
    files::write(&feed_path, feed)?;
    for entry in files::read_dir(&dist)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != file && Some(&name) != offered.as_ref() && image_name_len(&name) == Some(name.len()) {
            run(Command::new("trash").arg(entry.path()))?;
        }
    }
    next_build(&version)?;
    Ok(published)
}

/// What the update dialog shows: the notes written for this release and the way to report a problem.
pub(crate) fn release_notes(written: &str, previous_feed: Option<&str>) -> Result<String, String> {
    let written = written.trim();
    if written.is_empty() {
        return Err(format!("{NOTES} is empty; write what this release brings"));
    }
    if written.contains("]]>") {
        return Err(format!("{NOTES} contains ]]>, which would end the feed's text early"));
    }
    if previous_feed.is_some_and(|feed| feed.contains(written)) {
        return Err(format!("{NOTES} still holds the notes of the previous release"));
    }
    Ok(format!("{written}\n\n{REPORT_LINE}"))
}

/// The update feed Sparkle reads: the newest disk image with the EdDSA signature and length `sign_update` printed for
/// it. Older Macs than `MINIMUM_SYSTEM` are not offered it.
pub(crate) fn appcast(version: &str, image: &str, signature: &str, date: &str, notes: &str) -> Result<String, String> {
    if !signature.starts_with("sparkle:edSignature=\"") || !signature.contains(" length=\"") {
        return Err(format!("sign_update printed {signature:?}"));
    }
    Ok(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>{APP}</title>
    <item>
      <title>{APP} {version}</title>
      <pubDate>{date}</pubDate>
      <sparkle:version>{version}</sparkle:version>
      <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>{MINIMUM_SYSTEM}</sparkle:minimumSystemVersion>
      <description sparkle:format="markdown"><![CDATA[{notes}]]></description>
      <enclosure url="{DOWNLOADS}/{image}" {signature} type="application/octet-stream"/>
    </item>
  </channel>
</rss>
"#
    ))
}

/// The name of the disk image an update feed offers.
pub(crate) fn offered_image(appcast: &str) -> Option<&str> {
    let url = appcast.split("<enclosure url=\"").nth(1)?.split('"').next()?;
    url.strip_prefix(DOWNLOADS)?.strip_prefix('/')
}

/// Length of the disk image name `Splat.dmg` or `Splat-<version>.dmg` at the start of `s`.
pub(crate) fn image_name_len(s: &str) -> Option<usize> {
    let rest = s.strip_prefix(APP)?;
    if rest.starts_with(".dmg") {
        return Some(APP.len() + 4);
    }
    let rest = rest.strip_prefix('-')?;
    let end = rest.find(".dmg")?;
    is_version(&rest[..end]).then_some(APP.len() + 1 + end + 4)
}

/// SHA-1 hash and name of the first valid Developer ID Application identity in the keychains.
pub(crate) fn developer_id() -> Result<(String, String), String> {
    let listed = stdout(Command::new("security").args(["find-identity", "-v", "-p", "codesigning"]))?;
    listed
        .lines()
        .find_map(|line| {
            let name = line.split('"').nth(1)?;
            let hash = line.split_whitespace().nth(1)?;
            name.starts_with("Developer ID Application:")
                .then(|| (hash.to_string(), name.to_string()))
        })
        .ok_or_else(|| "no Developer ID Application certificate in the keychain".into())
}

/// Packs the app with a link to /Applications into a compressed, read-only disk image.
pub(crate) fn disk_image(app: &Path) -> Result<PathBuf, String> {
    let out = app.parent().ok_or("the app has no parent directory")?;
    let staging = out.join("dmg-staging");
    let writable = out.join(format!("{APP}-writable.dmg"));
    let image = out.join(format!("{APP}.dmg"));
    for file in [&writable, &image] {
        if file.exists() {
            files::remove_file(file)?;
        }
    }
    if staging.exists() {
        files::remove_dir_all(&staging)?;
    }
    files::create_dir_all(&staging)?;
    run(Command::new("ditto").arg(app).arg(staging.join(format!("{APP}.app"))))?;
    std::os::unix::fs::symlink("/Applications", staging.join("Applications"))
        .map_err(|e| format!("{}: {e}", staging.display()))?;
    run(Command::new("hdiutil")
        .args(["create", "-volname", APP, "-format", "UDRW", "-ov", "-srcfolder"])
        .arg(&staging)
        .arg(&writable)
        .stdout(std::process::Stdio::null()))?;
    let attached = stdout(
        Command::new("hdiutil")
            .args(["attach", "-readwrite", "-noverify", "-noautoopen"])
            .arg(&writable),
    )?;
    let mount = attached
        .lines()
        .find_map(|l| l.split('\t').map(str::trim).find(|f| f.starts_with("/Volumes/")))
        .map(PathBuf::from)
        .ok_or("hdiutil attach reported no mount point")?;
    let laid_out = lay_out(&mount);
    let detached = run(Command::new("hdiutil")
        .arg("detach")
        .arg(&mount)
        .stdout(std::process::Stdio::null()));
    if detached.is_err() {
        run(Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(&mount)
            .stdout(std::process::Stdio::null()))?;
    }
    if let Err(e) = laid_out {
        println!("Finder layout skipped, the image opens with default icon positions: {e}");
    }
    run(Command::new("hdiutil")
        .args(["convert", "-format", "UDZO", "-imagekey", "zlib-level=9", "-o"])
        .arg(&image)
        .arg(&writable)
        .stdout(std::process::Stdio::null()))?;
    files::remove_file(&writable)?;
    files::remove_dir_all(&staging)?;
    Ok(image)
}

/// Finder keeps window size, icon positions and background in the image's .DS_Store, so only Finder itself can
/// write them, on the mounted image. Needs the Automation permission for Finder.
pub(crate) fn lay_out(mount: &Path) -> Result<(), String> {
    let background = root().join("assets").join("dmg-background.png");
    files::create_dir_all(mount.join(".background"))?;
    fs::copy(&background, mount.join(".background/background.png"))
        .map_err(|e| format!("{}: {e}", background.display()))?;
    let disk = mount
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("mount point without a name")?;
    let script = format!(
        r#"tell application "Finder"
  tell disk "{disk}"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set bounds of container window to {{400, 200, 900, 500}}
    set viewOptions to the icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 96
    set background picture of viewOptions to file ".background:background.png"
    set position of item "{APP}.app" of container window to {{120, 150}}
    set position of item "Applications" of container window to {{380, 150}}
    close
    open
    update without registering applications
    delay 1
    close
  end tell
end tell"#
    );
    run(Command::new("osascript").args(["-e", &script]))
}

/// Uploads a disk image or zip archive to Apple's notary service and waits for the verdict.
pub(crate) fn notarize(file: &Path) -> Result<(), String> {
    println!("waiting for Apple's notary service, usually a few minutes");
    let mut submit = Command::new("xcrun");
    submit
        .args([
            "notarytool",
            "submit",
            "--keychain-profile",
            NOTARY_PROFILE,
            "--wait",
            "--output-format",
            "json",
        ])
        .arg(file)
        .stderr(std::process::Stdio::inherit());
    let out = submit.output().map_err(|e| format!("{submit:?}: {e}"))?;
    let result = String::from_utf8_lossy(&out.stdout);
    let status = json_string(&result, "status");
    if status == Some("Accepted") && out.status.success() {
        return Ok(());
    }
    if let Some(id) = json_string(&result, "id") {
        let _ = run(Command::new("xcrun").args(["notarytool", "log", id, "--keychain-profile", NOTARY_PROFILE]));
    }
    Err(format!(
        "notarization failed: status {}, notarytool {}",
        status.unwrap_or("unknown"),
        out.status
    ))
}

/// The value of a top-level string field in notarytool's flat JSON output.
pub(crate) fn json_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    rest.split('"').next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_image_is_named_with_or_without_a_version() {
        assert_eq!(image_name_len("Splat-1.0.3.dmg"), Some(15));
        assert_eq!(image_name_len("Splat.dmg"), Some(9));
        assert_eq!(image_name_len("Splat-notes.dmg"), None);
    }

    #[test]
    fn every_readme_downloads_the_fixed_name_in_the_latest_release() {
        let languages = files::read_dir(root().join("resources"))
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "lproj"))
            .count();
        let mut readmes = vec![root().join("README.md")];
        readmes.extend(
            files::read_dir(root().join("assets/readme"))
                .unwrap()
                .flatten()
                .map(|entry| entry.path().join("README.md"))
                .filter(|readme| readme.is_file()),
        );
        assert_eq!(readmes.len(), languages);
        for readme in readmes {
            let text = fs::read_to_string(&readme).unwrap();
            assert!(
                text.contains("https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg"),
                "{}",
                readme.display()
            );
            assert!(
                !text.contains("/main/dist/Splat") && !text.contains("Splat-1."),
                "{}",
                readme.display()
            );
        }
    }

    #[test]
    fn the_feed_carries_the_notes_of_this_release_and_the_way_to_report_a_problem() {
        let notes = release_notes("- New.\n", None).unwrap();
        assert_eq!(notes, format!("- New.\n\n{REPORT_LINE}"));
        let feed = appcast(
            "1.0.4",
            "Splat-1.0.4.dmg",
            "sparkle:edSignature=\"c2ln\" length=\"42\"",
            "",
            &notes,
        )
        .unwrap();
        assert!(feed.contains("<description sparkle:format=\"markdown\"><![CDATA[- New.\n\nSomething wrong"));
        assert!(release_notes(" \n", None).is_err());
        assert!(release_notes("a ]]> b", None).is_err());
        assert!(release_notes("- New.\n", Some(&feed)).is_err());
        assert!(release_notes("- Newer.\n", Some(&feed)).is_ok());
    }
}
