//! The app bundle: its property list, Sparkle and the signature.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::*;

/// macOS ties the recent documents list and granted folder access to the app's signature; with a certificate of
/// its own, unlike an ad hoc signature, the app stays the same app across rebuilds.
pub(crate) const SIGNING_IDENTITY: &str = "Splat Local Signing";

/// Sparkle, the update framework release builds carry, from its GitHub release; the checksum pins the archive.
pub(crate) const SPARKLE_VERSION: &str = "2.10.1";

pub(crate) const SPARKLE_SHA256: &str = "1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27";

/// Public half of the EdDSA key that signs updates. Sparkle's `generate_keys` keeps the private half in the login
/// keychain; installed copies accept only updates signed with it, so losing it ends updates for them.
pub(crate) const UPDATE_KEY: &str = "21sUy80rKTKlgQLPk+T7UrAkoqVQ+VV+6rfHKVAL4EU=";

pub(crate) const MINIMUM_SYSTEM: &str = "14.0";

/// What the disk image runs on: each processor's target for cargo and its architecture for `lipo`.
pub(crate) const UNIVERSAL: [(&str, &str); 2] = [("aarch64-apple-darwin", "arm64"), ("x86_64-apple-darwin", "x86_64")];

/// A file type only the self-test bundle declares, so the self-test can make itself the default app for it without
/// touching any other association.
pub(crate) const PROBE_TYPE: &str = "local.splat.selftest.probe";

/// The self-test signs with the local certificate, which has no Team ID; without this, the hardened runtime refuses
/// to load Sparkle.
pub(crate) const SELFTEST_ENTITLEMENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>com.apple.security.cs.disable-library-validation</key>
	<true/>
</dict>
</plist>
"#;

/// Whether a build carries Sparkle, and which feed it reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Updates {
    /// Local builds: the hardened runtime loads Sparkle only with the app's Team ID, which the local certificate
    /// lacks, and an update must not replace a build of one's own.
    Off,
    /// The disk image, signed with the Developer ID.
    Published,
    /// The self-test: a feed that does not exist, no question about automatic checks, and an entitlement that lets
    /// the locally signed app load Sparkle.
    Selftest,
}

/// Selftest and debug builds are separate apps: they leave the user's settings, recent documents and Markdown file
/// association alone, and macOS cannot mistake an outdated debug build for Splat when it opens a document.
#[expect(clippy::too_many_lines, reason = "the property list, as it reads in the file")]
pub(crate) fn info_plist(version: &str, languages: &[String], variant: Option<&str>, updates: Updates) -> String {
    let (bundle_id, rank) = match variant {
        Some(variant) => (format!("{BUNDLE_ID}.{}", variant.to_lowercase()), "None"),
        None => (BUNDLE_ID.to_string(), "Default"),
    };
    let localizations: String = languages
        .iter()
        .map(|l| format!("\t\t<string>{l}</string>\n"))
        .collect();
    let feed = |url: &str| {
        format!(
            "\t<key>SUFeedURL</key>\n\t<string>{url}</string>\n\t<key>SUPublicEDKey</key>\n\t<string>{UPDATE_KEY}</string>\n"
        )
    };
    let updates = match updates {
        Updates::Off => String::new(),
        Updates::Published => feed(&format!("{DOWNLOADS}/appcast.xml")),
        Updates::Selftest => {
            feed("https://127.0.0.1:9/appcast.xml") + "\t<key>SUEnableAutomaticChecks</key>\n\t<false/>\n"
        }
    };
    let (probe_handler, probe_type) = if variant == Some("Selftest") {
        (
            format!(
                "\t\t<dict>\n\t\t\t<key>CFBundleTypeName</key>\n\t\t\t<string>Self-test Probe</string>\n\
                 \t\t\t<key>CFBundleTypeRole</key>\n\t\t\t<string>Viewer</string>\n\
                 \t\t\t<key>LSHandlerRank</key>\n\t\t\t<string>Owner</string>\n\
                 \t\t\t<key>LSItemContentTypes</key>\n\t\t\t<array>\n\t\t\t\t<string>{PROBE_TYPE}</string>\n\t\t\t</array>\n\
                 \t\t</dict>\n"
            ),
            format!(
                "\t<key>UTExportedTypeDeclarations</key>\n\t<array>\n\t\t<dict>\n\
                 \t\t\t<key>UTTypeIdentifier</key>\n\t\t\t<string>{PROBE_TYPE}</string>\n\
                 \t\t\t<key>UTTypeConformsTo</key>\n\t\t\t<array>\n\t\t\t\t<string>public.data</string>\n\t\t\t</array>\n\
                 \t\t\t<key>UTTypeTagSpecification</key>\n\t\t\t<dict>\n\t\t\t\t<key>public.filename-extension</key>\n\
                 \t\t\t\t<array>\n\t\t\t\t\t<string>splatprobe</string>\n\t\t\t\t</array>\n\t\t\t</dict>\n\
                 \t\t</dict>\n\t</array>\n"
            ),
        )
    } else {
        (String::new(), String::new())
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleLocalizations</key>
	<array>
{localizations}	</array>
	<key>CFBundleExecutable</key>
	<string>{APP}</string>
	<key>CFBundleIdentifier</key>
	<string>{bundle_id}</string>
	<key>CFBundleName</key>
	<string>{APP}</string>
	<key>CFBundleDisplayName</key>
	<string>{APP}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.productivity</string>
	<key>LSMinimumSystemVersion</key>
	<string>{MINIMUM_SYSTEM}</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSPrincipalClass</key>
	<string>NSApplication</string>
	<key>NSHumanReadableCopyright</key>
	<string>Markdown editor</string>
{updates}	<key>CFBundleDocumentTypes</key>
	<array>
		<dict>
			<key>CFBundleTypeName</key>
			<string>Markdown Document</string>
			<key>CFBundleTypeRole</key>
			<string>Editor</string>
			<key>LSHandlerRank</key>
			<string>{rank}</string>
			<key>LSItemContentTypes</key>
			<array>
				<string>net.daringfireball.markdown</string>
			</array>
			<key>NSDocumentClass</key>
			<string>MDDocument</string>
		</dict>
{probe_handler}	</array>
{probe_type}	<key>UTImportedTypeDeclarations</key>
	<array>
		<dict>
			<key>UTTypeIdentifier</key>
			<string>net.daringfireball.markdown</string>
			<key>UTTypeDescription</key>
			<string>Markdown Document</string>
			<key>UTTypeConformsTo</key>
			<array>
				<string>public.plain-text</string>
			</array>
			<key>UTTypeTagSpecification</key>
			<dict>
				<key>public.filename-extension</key>
				<array>
					<string>md</string>
					<string>markdown</string>
					<string>mdown</string>
					<string>mkd</string>
				</array>
				<key>public.mime-type</key>
				<string>text/markdown</string>
			</dict>
		</dict>
	</array>
</dict>
</plist>
"#
    )
}

pub(crate) fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    files::create_dir_all(to)?;
    for entry in files::read_dir(from)? {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            files::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Which app a bundle holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Build {
    /// Built for this Mac without optimisation.
    Debug,
    /// Built for this Mac as it ships, signed with the local certificate.
    Release,
    /// The app with its self-test, optimised or not.
    Selftest { release: bool },
    /// The universal release signed with the Developer ID, the only one that goes out.
    Distribution,
}

impl Build {
    pub(crate) fn release(self) -> bool {
        match self {
            Build::Debug => false,
            Build::Release | Build::Distribution => true,
            Build::Selftest { release } => release,
        }
    }

    pub(crate) fn selftest(self) -> bool {
        matches!(self, Build::Selftest { .. })
    }

    /// The word behind the app's name, which keeps the builds apart in the Finder and in Launch Services.
    pub(crate) fn variant(self) -> Option<&'static str> {
        match self {
            Build::Selftest { .. } => Some("Selftest"),
            Build::Debug => Some("Debug"),
            Build::Release | Build::Distribution => None,
        }
    }

    /// The distribution and the self-test carry Sparkle, see `Updates`.
    pub(crate) fn updates(self) -> Updates {
        match self {
            Build::Distribution => Updates::Published,
            Build::Selftest { .. } => Updates::Selftest,
            Build::Debug | Build::Release => Updates::Off,
        }
    }
}

/// Builds and signs a bundle for this Mac. A release build raises the build number; debug and selftest builds leave it.
pub(crate) fn bundle(build: Build) -> Result<PathBuf, String> {
    let version = version()?;
    let app = assemble(build, &version)?;
    let entitlements = if build.selftest() {
        let path = target_dir().join("selftest.entitlements");
        files::write(&path, SELFTEST_ENTITLEMENTS)?;
        Some(path)
    } else {
        None
    };
    sign(&app, signing_identity()?, false, entitlements.as_deref())?;
    if build == Build::Release {
        next_build(&version)?;
    }
    Ok(app)
}

/// Builds the app bundle, unsigned.
pub(crate) fn assemble(build: Build, version: &str) -> Result<PathBuf, String> {
    let root = root();
    let profile = if build.release() { "release" } else { "debug" };
    let target = target_dir().join(profile);
    let program = if build == Build::Distribution {
        universal_program()?
    } else {
        let mut cargo = Command::new(env!("CARGO"));
        cargo.current_dir(&root).args(["build", "--package", PACKAGE]);
        if build.release() {
            cargo.arg("--release");
        }
        if build.selftest() {
            cargo.args(["--features", "selftest"]);
        }
        run(&mut cargo)?;
        target.join(PROGRAM)
    };
    let variant = build.variant();
    let updates = build.updates();
    let app = target.join(match variant {
        Some(variant) => format!("{APP} {variant}.app"),
        None => format!("{APP}.app"),
    });
    if app.exists() {
        files::remove_dir_all(&app)?;
    }
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    files::create_dir_all(&macos)?;
    files::copy(&program, macos.join(APP))?;
    let plist = contents.join("Info.plist");
    // Every resources/<language>.lproj is a localization; macOS picks one from the user's preferred languages.
    let mut languages: Vec<String> = files::read_dir(root.join("resources"))?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".lproj").map(str::to_string))
        .collect();
    languages.sort();
    files::write(&plist, info_plist(version, &languages, variant, updates))?;
    run(Command::new("plutil").arg("-lint").arg("-s").arg(&plist))?;
    copy_dir(&root.join("resources"), &resources)?;
    let icns = root.join("assets").join("AppIcon.icns");
    if icns.exists() {
        files::copy(&icns, resources.join("AppIcon.icns"))?;
    }
    files::write(contents.join("PkgInfo"), "APPL????")?;
    if updates != Updates::Off {
        let frameworks = contents.join("Frameworks");
        files::create_dir_all(&frameworks)?;
        let framework = frameworks.join("Sparkle.framework");
        let sparkle = sparkle()?;
        run(Command::new("ditto")
            .arg(sparkle.join("Sparkle.framework"))
            .arg(&framework))?;
        files::copy(sparkle.join("LICENSE"), resources.join("LICENSE-sparkle"))?;
        // Only a sandboxed app needs Sparkle's XPC services.
        files::remove_dir_all(framework.join("Versions/B/XPCServices"))?;
        files::remove_file(framework.join("XPCServices"))?;
    }
    Ok(app)
}

/// The release program for Apple Silicon and Intel in one file, through rustup with the toolchain rust-toolchain.toml
/// names, since Homebrew's Rust ships the standard library for the host processor alone.
pub(crate) fn universal_program() -> Result<PathBuf, String> {
    let mut build = rustup_cargo()?;
    build.args(["build", "--package", PACKAGE, "--release"]);
    let mut join = Command::new("lipo");
    join.arg("-create");
    for (target, _) in UNIVERSAL {
        build.args(["--target", target]);
        join.arg(target_dir().join(target).join("release").join(PROGRAM));
    }
    run(&mut build)?;
    let program = target_dir().join(format!("{PROGRAM}-universal"));
    run(join.arg("-output").arg(&program))?;
    Ok(program)
}

/// That the library builds for a system without AppKit, through rustup for the same reason.
pub(crate) fn check_library() -> Result<(), String> {
    run(rustup_cargo()?.args([
        "check",
        "--package",
        "splat-i18n",
        "--package",
        "splat-file-state",
        "--package",
        "splat-git",
        "--package",
        "splat-markdown",
        "--package",
        "splat-notes",
        "--package",
        "splat-import",
        "--package",
        "splat-preview",
        "--target",
        "x86_64-unknown-linux-gnu",
    ]))
}

/// cargo of the toolchain rust-toolchain.toml names, with its targets.
fn rustup_cargo() -> Result<Command, String> {
    let root = root();
    let rustup = rustup()?;
    run(Command::new(&rustup).current_dir(&root).args(["toolchain", "install"]))?;
    let tool = |name: &str| {
        stdout(Command::new(&rustup).current_dir(&root).args(["which", name])).map(|path| path.trim().to_string())
    };
    let mut cargo = Command::new(tool("cargo")?);
    // Cargo would otherwise run the rustc first on the PATH, which may be Homebrew's.
    cargo.current_dir(&root).env("RUSTC", tool("rustc")?);
    Ok(cargo)
}

/// rustup from the PATH, or from Homebrew, which keeps it off the PATH beside its own Rust.
fn rustup() -> Result<PathBuf, String> {
    let on_path = Command::new("rustup")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if on_path {
        return Ok("rustup".into());
    }
    stdout(Command::new("brew").args(["--prefix", "rustup"]))
        .map(|prefix| Path::new(prefix.trim()).join("bin/rustup"))
        .ok()
        .filter(|path| path.exists())
        .ok_or_else(|| "this needs rustup; install it with `brew install rustup`".into())
}

/// The architectures of `UNIVERSAL` that are not in the output of `lipo -archs`.
pub(crate) fn missing_architectures(listed: &str) -> Vec<&'static str> {
    UNIVERSAL
        .iter()
        .map(|(_, arch)| *arch)
        .filter(|arch| !listed.split_whitespace().any(|name| name == *arch))
        .collect()
}

/// A program without the code for one processor goes unnoticed on the other and keeps the app from starting on that one.
pub(crate) fn check_universal(app: &Path) -> Result<(), String> {
    let sparkle = app.join("Contents/Frameworks/Sparkle.framework/Versions/B");
    for program in [
        app.join("Contents/MacOS").join(APP),
        sparkle.join("Sparkle"),
        sparkle.join("Autoupdate"),
        sparkle.join("Updater.app/Contents/MacOS/Updater"),
    ] {
        let missing = missing_architectures(&stdout(Command::new("lipo").arg("-archs").arg(&program))?);
        if !missing.is_empty() {
            return Err(format!("{} lacks {}", program.display(), missing.join(" and ")));
        }
    }
    Ok(())
}

/// Sparkle's release archive, downloaded once into target/, also for measured builds elsewhere, checked against its
/// pinned checksum and unpacked: the framework and its `bin/` tools for signing updates.
pub(crate) fn sparkle() -> Result<PathBuf, String> {
    let target = root().join("target");
    let dir = target.join(format!("sparkle-{SPARKLE_VERSION}"));
    if dir.join("Sparkle.framework").exists() {
        return Ok(dir);
    }
    let archive = target.join(format!("Sparkle-{SPARKLE_VERSION}.tar.xz"));
    let url = format!(
        "https://github.com/sparkle-project/Sparkle/releases/download/{SPARKLE_VERSION}/Sparkle-{SPARKLE_VERSION}.tar.xz"
    );
    run(Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--proto-redir", "=https", "-o"])
        .arg(&archive)
        .arg(&url))?;
    let actual = sha256(&archive)?;
    if actual != SPARKLE_SHA256 {
        files::remove_file(&archive)?;
        return Err(format!("{url} has checksum {actual}, expected {SPARKLE_SHA256}"));
    }
    let unpacking = target.join(format!("sparkle-{SPARKLE_VERSION}.partial"));
    if unpacking.exists() {
        files::remove_dir_all(&unpacking)?;
    }
    files::create_dir_all(&unpacking)?;
    run(Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&unpacking))?;
    files::rename(&unpacking, &dir)?;
    files::remove_file(&archive)?;
    Ok(dir)
}

/// Notarization requires the secure timestamp; local builds skip it, since it needs a request to Apple. Sparkle's
/// helpers are signed first, inside out, as its documentation prescribes instead of `--deep`.
pub(crate) fn sign(app: &Path, identity: &str, timestamp: bool, entitlements: Option<&Path>) -> Result<(), String> {
    let sparkle = app.join("Contents/Frameworks/Sparkle.framework");
    let mut parts = Vec::new();
    if sparkle.exists() {
        parts.extend([
            sparkle.join("Versions/B/Autoupdate"),
            sparkle.join("Versions/B/Updater.app"),
            sparkle,
        ]);
    }
    parts.push(app.to_path_buf());
    for part in parts {
        let mut codesign = Command::new("codesign");
        // Hardened runtime: no injected libraries, no debugger attaching, no writable code pages.
        codesign
            .args(["--force", "--sign", identity, "--options", "runtime"])
            .arg(if timestamp { "--timestamp" } else { "--timestamp=none" });
        if let Some(entitlements) = entitlements.filter(|_| part == app) {
            codesign.arg("--entitlements").arg(entitlements);
        }
        run(codesign.arg(&part))?;
    }
    Ok(())
}

/// The local signing certificate, created in the login keychain on first use.
pub(crate) fn signing_identity() -> Result<&'static str, String> {
    let listed = Command::new("security")
        .args(["find-identity", "-p", "codesigning"])
        .output()
        .map_err(|e| format!("security: {e}"))?;
    if String::from_utf8_lossy(&listed.stdout).contains(&format!("\"{SIGNING_IDENTITY}\"")) {
        return Ok(SIGNING_IDENTITY);
    }
    let dir = std::env::temp_dir().join(format!("{PROGRAM}-signing-{}", std::process::id()));
    std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700)
        .create(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let created = create_identity(&dir);
    let _ = fs::remove_dir_all(&dir);
    created.map(|()| SIGNING_IDENTITY)
}

pub(crate) fn create_identity(dir: &Path) -> Result<(), String> {
    let (key, cert, p12) = (dir.join("key.pem"), dir.join("cert.pem"), dir.join("identity.p12"));
    let password = format!(
        "{}{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    // The system's LibreSSL, since `security import` only reads PKCS#12 files with the older ciphers it writes.
    let openssl = "/usr/bin/openssl";
    run(Command::new(openssl)
        .args(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "7300"])
        .args(["-subj", &format!("/CN={SIGNING_IDENTITY}")])
        .args(["-addext", "extendedKeyUsage=critical,codeSigning"])
        .args(["-addext", "keyUsage=critical,digitalSignature"])
        .arg("-keyout")
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .stderr(std::process::Stdio::null()))?;
    run(Command::new(openssl)
        .args(["pkcs12", "-export", "-inkey"])
        .arg(&key)
        .arg("-in")
        .arg(&cert)
        .arg("-out")
        .arg(&p12)
        .args(["-passout", &format!("pass:{password}")]))?;
    run(Command::new("security")
        .arg("import")
        .arg(&p12)
        .args(["-P", &password, "-T", "/usr/bin/codesign"]))
}
