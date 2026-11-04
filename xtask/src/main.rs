//! Build tasks in the language of the project: bundling, signing, the disk image, the icon, vendoring and running the
//! self-tests.

mod assets;
mod bundle;
mod coverage;
mod release;
mod selftest;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub(crate) use assets::*;
pub(crate) use bundle::*;
pub(crate) use coverage::*;
pub(crate) use release::*;
pub(crate) use selftest::*;

pub(crate) const PACKAGE: &str = "splat-app";

/// The program the package builds.
pub(crate) const PROGRAM: &str = "splat";

pub(crate) const APP: &str = "Splat";

pub(crate) const BUNDLE_ID: &str = "local.splat";

/// Where GitHub serves the committed `dist/` folder: the disk image and the update feed.
pub(crate) const DOWNLOADS: &str = "https://raw.githubusercontent.com/sventmueller/splat/main/dist";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(unknown) = args
        .iter()
        .find(|a| a.starts_with("--") && !FLAGS.contains(&a.as_str()))
    {
        eprintln!("error: unknown option {unknown}\n{USAGE}");
        return ExitCode::FAILURE;
    }
    let release = args.iter().any(|a| a == "--release");
    let selftest = args.iter().any(|a| a == "--selftest");
    let result = match args.first().map(String::as_str) {
        Some("bundle") => bundle(release, selftest).map(|p| println!("{}", p.display())),
        Some("dmg") => dmg().map(|p| println!("{}", p.display())),
        Some("universal") => universal_program().map(|p| println!("{}", p.display())),
        Some("check-library") => check_library(),
        Some("icon") => icon(),
        Some("vendor") => match args.get(1).map(String::as_str) {
            None | Some("check") => vendor(false),
            Some("fetch") => vendor(true),
            Some(_) => Err(USAGE.into()),
        },
        Some("selftest") => run_selftest(&args[1..]),
        Some("coverage") => coverage(args.iter().any(|a| a == "--library")),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

const FLAGS: [&str; 10] = [
    "--only",
    "--release",
    "--selftest",
    "--stop-after",
    "--scenarios",
    "--library",
    "--record",
    "--in-back",
    "--by-hand",
    "--keystroke-ms",
];

pub(crate) const USAGE: &str =
    "usage: cargo xtask bundle [--release] [--selftest] | dmg | universal | check-library | icon | vendor [check|fetch]
       cargo xtask selftest probe | gui [--stop-after <check>] [--scenarios] [--only <group>] [--in-back] | resize | typing [--by-hand] | bench <file.md> [--keystroke-ms <n>], with --record to write what a run made into xtask/checks
       cargo xtask coverage [--library]";

pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// Where cargo builds: `CARGO_TARGET_DIR` if set, as for the measured builds of `coverage`, else `target/`.
pub(crate) fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), |dir| root().join(dir))
}

pub(crate) fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed with {status}"))
    }
}

pub(crate) fn stdout(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("{cmd:?}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{cmd:?} failed with {}", out.status))
    }
}

/// Major, minor and build number, digits only.
pub(crate) fn is_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The version the next build gets, from VERSION: major and minor are raised by hand, the build number by every
/// release build.
pub(crate) fn version() -> Result<String, String> {
    let path = root().join("VERSION");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let version = text.trim();
    if is_version(version) {
        Ok(version.to_string())
    } else {
        Err(format!("{}: {version:?} is not major.minor.build", path.display()))
    }
}

/// Raises the build number in VERSION past `built`, the version just built.
pub(crate) fn next_build(built: &str) -> Result<(), String> {
    let (head, build) = built.rsplit_once('.').ok_or("no build number")?;
    let build: u64 = build.parse().map_err(|e| format!("build number {build}: {e}"))?;
    let next = format!("{head}.{}", build + 1);
    files::write(root().join("VERSION"), format!("{next}\n"))?;
    println!("built {built}, next build {next}");
    Ok(())
}

/// `std::fs` with the path in every error.
pub(crate) mod files {
    use std::fs;
    use std::path::Path;

    fn at<T>(path: &Path, result: std::io::Result<T>) -> Result<T, String> {
        result.map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn write(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<(), String> {
        at(path.as_ref(), fs::write(path.as_ref(), content))
    }

    pub(crate) fn read_to_string(path: impl AsRef<Path>) -> Result<String, String> {
        at(path.as_ref(), fs::read_to_string(path.as_ref()))
    }

    pub(crate) fn read_dir(path: impl AsRef<Path>) -> Result<fs::ReadDir, String> {
        at(path.as_ref(), fs::read_dir(path.as_ref()))
    }

    pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> Result<(), String> {
        at(path.as_ref(), fs::create_dir_all(path.as_ref()))
    }

    pub(crate) fn remove_dir_all(path: impl AsRef<Path>) -> Result<(), String> {
        at(path.as_ref(), fs::remove_dir_all(path.as_ref()))
    }

    pub(crate) fn remove_file(path: impl AsRef<Path>) -> Result<(), String> {
        at(path.as_ref(), fs::remove_file(path.as_ref()))
    }

    pub(crate) fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<u64, String> {
        at(from.as_ref(), fs::copy(from.as_ref(), to.as_ref()))
    }

    pub(crate) fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), String> {
        at(from.as_ref(), fs::rename(from.as_ref(), to.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_distributed_build_looks_for_updates_and_the_feed_offers_the_signed_image() {
        let languages = ["de".to_string()];
        let release = info_plist("1.0.4", &languages, None, Updates::Published);
        assert!(release.contains(&format!("<string>{DOWNLOADS}/appcast.xml</string>")));
        assert!(release.contains(&format!("<key>SUPublicEDKey</key>\n\t<string>{UPDATE_KEY}</string>")));
        assert!(!info_plist("1.0.4", &languages, None, Updates::Off).contains("SUFeedURL"));
        let selftest = info_plist("1.0.4", &languages, Some("Selftest"), Updates::Selftest);
        assert!(selftest.contains("<string>https://127.0.0.1:9/appcast.xml</string>") && selftest.contains(PROBE_TYPE));
        assert!(selftest.contains("<key>SUEnableAutomaticChecks</key>\n\t<false/>"));
        let feed = appcast(
            "1.0.4",
            "Splat-1.0.4.dmg",
            "sparkle:edSignature=\"c2ln\" length=\"42\"",
            "Sat, 31 Oct 2026 12:00:00 +0000",
        )
        .unwrap();
        assert!(feed.contains(&format!(
            "<enclosure url=\"{DOWNLOADS}/Splat-1.0.4.dmg\" sparkle:edSignature=\"c2ln\" length=\"42\" \
             type=\"application/octet-stream\"/>"
        )));
        assert!(feed.contains("<sparkle:version>1.0.4</sparkle:version>"));
        assert_eq!(offered_image(&feed), Some("Splat-1.0.4.dmg"));
        assert_eq!(offered_image("<rss/>"), None);
        assert!(appcast("1.0.4", "Splat-1.0.4.dmg", "ERROR: no key", "").is_err());
    }

    #[test]
    fn a_program_is_universal_only_with_the_code_for_both_processors() {
        assert!(missing_architectures("x86_64 arm64\n").is_empty());
        assert_eq!(missing_architectures("arm64\n"), ["x86_64"]);
        assert_eq!(missing_architectures("arm64e x86_64h\n"), ["arm64", "x86_64"]);
    }

    #[test]
    fn versions_have_three_numbers() {
        assert!(is_version("1.0.1") && is_version("12.3.456"));
        assert!(!is_version("1.0") && !is_version("1.0.x") && !is_version("1..1") && !is_version("1.0.1.2"));
    }
}
