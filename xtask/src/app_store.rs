//! The build for the Mac App Store: universal, without Sparkle, signed with Apple Distribution and packed into an
//! installer package that Transporter uploads.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::*;

/// Where Xcode and the developer portal's downloads keep provisioning profiles.
const PROFILE_DIRS: [&str; 2] = [
    "Library/Developer/Xcode/UserData/Provisioning Profiles",
    "Library/MobileDevice/Provisioning Profiles",
];

/// Builds `target/Splat-<version>.pkg` for App Store Connect. The build number stays: the store needs the version it
/// was given, and a second upload of the same version needs a higher one in `VERSION`.
pub(crate) fn app_store() -> Result<PathBuf, String> {
    let version = version()?;
    let (signer, name) = identity(
        "codesigning",
        &["Apple Distribution:", "3rd Party Mac Developer Application:"],
    )?;
    let (installer, _) = identity(
        "basic",
        &["3rd Party Mac Developer Installer:", "Mac Installer Distribution:"],
    )?;
    let profile = store_profile()?;
    let app = assemble(Build::AppStore, &version)?;
    check_no_coverage(&app)?;
    check_universal(&app)?;
    // The profile is part of what the signature seals.
    files::copy(&profile, app.join("Contents/embedded.provisionprofile"))?;
    println!("signing {version} with {name}");
    sign(&app, &signer, true, Build::AppStore)?;
    run(Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app))?;
    let package = target_dir().join(format!("{APP}-{version}.pkg"));
    run(Command::new("productbuild")
        .arg("--component")
        .arg(&app)
        .arg("/Applications")
        .args(["--sign", &installer])
        .arg(&package))?;
    run(Command::new("pkgutil").arg("--check-signature").arg(&package))?;
    Ok(package)
}

/// The profile named by `PROFILE`, or the newest Mac App Store profile for the app in the usual folders.
fn store_profile() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PROFILE") {
        let path = PathBuf::from(path);
        return if is_store_profile(&decoded_profile(&path)?) {
            Ok(path)
        } else {
            Err(format!(
                "{} is no Mac App Store profile for {TEAM_ID}.{BUNDLE_ID}",
                path.display()
            ))
        };
    }
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = PROFILE_DIRS
        .iter()
        .filter_map(|dir| std::fs::read_dir(home.join(dir)).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "provisionprofile"))
        .filter(|path| decoded_profile(path).is_ok_and(|text| is_store_profile(&text)))
        .filter_map(|path| Some((path.metadata().ok()?.modified().ok()?, path)))
        .collect();
    found.sort();
    found.pop().map(|(_, path)| path).ok_or_else(|| {
        format!(
            "no Mac App Store provisioning profile for {TEAM_ID}.{BUNDLE_ID} in ~/{}; download it from the developer \
             portal there, or name it with PROFILE=<file>",
            PROFILE_DIRS[0]
        )
    })
}

/// The property list inside the signed profile.
fn decoded_profile(path: &Path) -> Result<String, String> {
    stdout(Command::new("security").args(["cms", "-D", "-i"]).arg(path))
}

/// Whether a decoded profile lets the store take this app: its identifier, and no list of test devices, which only
/// development and ad hoc profiles carry.
pub(crate) fn is_store_profile(plist: &str) -> bool {
    let value_of = |key: &str| {
        let after = plist.split(&format!("<key>{key}</key>")).nth(1)?;
        let start = after.find("<string>")? + "<string>".len();
        let end = after[start..].find("</string>")?;
        Some(after[start..start + end].trim().to_string())
    };
    value_of("com.apple.application-identifier").as_deref() == Some(&format!("{TEAM_ID}.{BUNDLE_ID}"))
        && !plist.contains("<key>ProvisionedDevices</key>")
        && !plist.contains("<key>ProvisionsAllDevices</key>")
}
