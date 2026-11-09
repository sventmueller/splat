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

/// What the app may do in its sandbox: files the user chose and folders granted once, the network for pictures from the
/// web and for updates, and printing. No file access beyond what the user grants, as in the Mac App Store.
pub(crate) fn app_entitlements(build: Build) -> String {
    let flag = |key: &str| format!("\t<key>{key}</key>\n\t<true/>\n");
    let list = |key: &str, values: &[String]| {
        let values: String = values.iter().map(|v| format!("\t\t<string>{v}</string>\n")).collect();
        format!("\t<key>{key}</key>\n\t<array>\n{values}\t</array>\n")
    };
    let mut keys = [
        "com.apple.security.app-sandbox",
        "com.apple.security.files.user-selected.read-write",
        "com.apple.security.files.bookmarks.app-scope",
        "com.apple.security.network.client",
        "com.apple.security.print",
    ]
    .map(flag)
    .concat();
    if build.updates() != Updates::Off {
        // Sparkle installs through services of its own, which the sandbox lets the app reach by name only.
        let id = bundle_id(build.variant());
        keys += &list(
            "com.apple.security.temporary-exception.mach-lookup.global-name",
            &[format!("{id}-spks"), format!("{id}-spki")],
        );
    }
    if build == Build::AppStore {
        let string = |key: &str, value: &str| format!("\t<key>{key}</key>\n\t<string>{value}</string>\n");
        // Both must match the provisioning profile the bundle carries, or the store refuses the upload.
        keys += &string("com.apple.application-identifier", &format!("{TEAM_ID}.{BUNDLE_ID}"));
        keys += &string("com.apple.developer.team-identifier", TEAM_ID);
    }
    if build.selftest() {
        // The local certificate has no Team ID; without this, the hardened runtime refuses to load Sparkle.
        keys += &flag("com.apple.security.cs.disable-library-validation");
        // The runner puts the documents of a run under target/.
        keys += &list(
            "com.apple.security.temporary-exception.files.absolute-path.read-write",
            &[format!("{}/target/", root().display())],
        );
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n{keys}</dict>\n</plist>\n"
    )
}

/// The copies of unsaved text that Splat kept before it ran in a sandbox move into its container on the first start
/// there; only the app itself takes them, not a debug or self-test build.
const CONTAINER_MIGRATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Move</key>
	<array>
		<string>${ApplicationSupport}/Splat</string>
	</array>
</dict>
</plist>
"#;

/// The decoding service gets a sandbox and nothing else: no network, and no file the user chose. The self-test's may
/// write under target/, where a measured build keeps what ran.
pub(crate) fn decoder_entitlements(build: Build) -> String {
    let measured = if build.selftest() {
        format!(
            "\t<key>com.apple.security.temporary-exception.files.absolute-path.read-write</key>\n\t<array>\n\t\t<string>{}/target/</string>\n\t</array>\n",
            root().display()
        )
    } else {
        String::new()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>com.apple.security.app-sandbox</key>\n\t<true/>\n{measured}</dict>\n</plist>\n"
    )
}

/// The bundle of the decoding service inside the app's, which macOS starts it from.
pub(crate) const DECODER_BUNDLE: &str = "Contents/XPCServices/SplatDecoder.xpc";

/// The identifier of a build of the app; the decoding service's adds `.decoder`, which the app looks for.
fn bundle_id(variant: Option<&str>) -> String {
    match variant {
        Some(variant) => format!("{BUNDLE_ID}.{}", variant.to_lowercase()),
        None => BUNDLE_ID.to_string(),
    }
}

/// The property list of the decoding service. launchd starts it with nothing of the app's environment, so a measured
/// build names its measurement file here.
pub(crate) fn decoder_plist(version: &str, variant: Option<&str>, measured: Option<&str>) -> String {
    let bundle_id = bundle_id(variant);
    let environment = measured.map_or_else(String::new, |file| {
        format!(
            "\t\t<key>EnvironmentVariables</key>\n\t\t<dict>\n\t\t\t<key>LLVM_PROFILE_FILE</key>\n\t\t\t<string>{file}</string>\n\t\t</dict>\n"
        )
    });
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleExecutable</key>
	<string>{DECODER}</string>
	<key>CFBundleIdentifier</key>
	<string>{bundle_id}.decoder</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>SplatDecoder</string>
	<key>CFBundlePackageType</key>
	<string>XPC!</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSMinimumSystemVersion</key>
	<string>{MINIMUM_SYSTEM}</string>
	<key>XPCService</key>
	<dict>
{environment}		<key>ServiceType</key>
		<string>Application</string>
	</dict>
</dict>
</plist>
"#
    )
}

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
    let (bundle_id, rank) = (bundle_id(variant), if variant.is_some() { "None" } else { "Default" });
    let localizations: String = languages
        .iter()
        .map(|l| format!("\t\t<string>{l}</string>\n"))
        .collect();
    let feed = |url: &str| {
        format!(
            "\t<key>SUFeedURL</key>\n\t<string>{url}</string>\n\t<key>SUPublicEDKey</key>\n\t<string>{UPDATE_KEY}</string>\n\t<key>SUEnableInstallerLauncherService</key>\n\t<true/>\n"
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
	<string>© 2026 Sven Müller</string>
	<key>ITSAppUsesNonExemptEncryption</key>
	<false/>
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
    /// The universal release signed with the Developer ID, for the disk image and Homebrew.
    Distribution,
    /// The universal release for the Mac App Store, signed with Apple Distribution and without an updater of its own.
    AppStore,
}

impl Build {
    pub(crate) fn release(self) -> bool {
        match self {
            Build::Debug => false,
            Build::Release | Build::Distribution | Build::AppStore => true,
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
            Build::Release | Build::Distribution | Build::AppStore => None,
        }
    }

    /// The distribution and the self-test carry Sparkle, see `Updates`.
    pub(crate) fn updates(self) -> Updates {
        match self {
            Build::Distribution => Updates::Published,
            Build::Selftest { .. } => Updates::Selftest,
            // The Mac App Store allows no updates but its own.
            Build::Debug | Build::Release | Build::AppStore => Updates::Off,
        }
    }
}

/// Builds and signs a bundle for this Mac. A release build raises the build number; debug and selftest builds leave it.
pub(crate) fn bundle(build: Build) -> Result<PathBuf, String> {
    let version = version()?;
    let app = assemble(build, &version)?;
    sign(&app, signing_identity()?, false, build)?;
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
    let (program, decoder) = if matches!(build, Build::Distribution | Build::AppStore) {
        universal_programs()?
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
        (target.join(PROGRAM), target.join(DECODER))
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
    let service = app.join(DECODER_BUNDLE).join("Contents");
    files::create_dir_all(service.join("MacOS"))?;
    files::copy(&decoder, service.join("MacOS").join(DECODER))?;
    let measured = std::env::var("LLVM_PROFILE_FILE").ok().filter(|_| build.selftest());
    files::write(
        service.join("Info.plist"),
        decoder_plist(version, variant, measured.as_deref()),
    )?;
    run(Command::new("plutil")
        .arg("-lint")
        .arg("-s")
        .arg(service.join("Info.plist")))?;
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
    if variant.is_none() {
        files::write(resources.join("container-migration.plist"), CONTAINER_MIGRATION)?;
    }
    let mut bundled_sparkle = None;
    if updates != Updates::Off {
        let frameworks = contents.join("Frameworks");
        files::create_dir_all(&frameworks)?;
        let framework = frameworks.join("Sparkle.framework");
        let sparkle = sparkle()?;
        run(Command::new("ditto")
            .arg(sparkle.join("Sparkle.framework"))
            .arg(&framework))?;
        files::copy(sparkle.join("LICENSE"), resources.join("LICENSE-sparkle"))?;
        // The sandboxed app installs through Sparkle's installer service; it downloads with its own network access.
        files::remove_dir_all(framework.join("Versions/B/XPCServices/Downloader.xpc"))?;
        bundled_sparkle = Some(sparkle);
    }
    files::write(
        resources.join(ACKNOWLEDGEMENTS),
        acknowledgements(bundled_sparkle.as_deref())?,
    )?;
    Ok(app)
}

/// The release programs, the app's and the decoding service's, for Apple Silicon and Intel in one file each, through
/// rustup with the toolchain rust-toolchain.toml names, since Homebrew's Rust ships the standard library for the host
/// processor alone.
pub(crate) fn universal_programs() -> Result<(PathBuf, PathBuf), String> {
    let mut build = rustup_cargo()?;
    build.args(["build", "--package", PACKAGE, "--release"]);
    for (target, _) in UNIVERSAL {
        build.args(["--target", target]);
    }
    run(&mut build)?;
    let join = |name: &str| -> Result<PathBuf, String> {
        let mut join = Command::new("lipo");
        join.arg("-create");
        for (target, _) in UNIVERSAL {
            join.arg(target_dir().join(target).join("release").join(name));
        }
        let program = target_dir().join(format!("{name}-universal"));
        run(join.arg("-output").arg(&program))?;
        Ok(program)
    };
    Ok((join(PROGRAM)?, join(DECODER)?))
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
        "splat-ooxml",
        "--package",
        "splat-preview",
        "--package",
        "splat-editor-state",
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
    let mut programs = vec![
        app.join("Contents/MacOS").join(APP),
        app.join(DECODER_BUNDLE).join("Contents/MacOS").join(DECODER),
    ];
    if sparkle.exists() {
        programs.extend([
            sparkle.join("Sparkle"),
            sparkle.join("Autoupdate"),
            sparkle.join("Updater.app/Contents/MacOS/Updater"),
        ]);
    }
    for program in programs {
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
pub(crate) fn sign(app: &Path, identity: &str, timestamp: bool, build: Build) -> Result<(), String> {
    let sparkle = app.join("Contents/Frameworks/Sparkle.framework");
    let entitlements = target_dir().join(format!("{}.entitlements", build.variant().unwrap_or(APP)));
    files::write(&entitlements, app_entitlements(build))?;
    let service_entitlements = target_dir().join("decoder.entitlements");
    files::write(&service_entitlements, decoder_entitlements(build))?;
    let mut parts = vec![app.join(DECODER_BUNDLE)];
    if sparkle.exists() {
        parts.extend([
            sparkle.join("Versions/B/XPCServices/Installer.xpc"),
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
        if part == app {
            codesign.arg("--entitlements").arg(&entitlements);
        }
        if part == app.join(DECODER_BUNDLE) {
            codesign.arg("--entitlements").arg(&service_entitlements);
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
