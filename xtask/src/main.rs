use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PACKAGE: &str = "splat";
const APP: &str = "Splat";
const BUNDLE_ID: &str = "local.splat";
/// macOS ties the recent documents list and granted folder access to the app's signature; with a certificate of
/// its own, unlike an ad hoc signature, the app stays the same app across rebuilds.
const SIGNING_IDENTITY: &str = "Splat Local Signing";
/// Created once with `xcrun notarytool store-credentials notary --apple-id <Apple ID> --team-id <team ID>`.
const NOTARY_PROFILE: &str = "notary";
const ICON_SIZES: [(u32, &str); 10] = [
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let release = args.iter().any(|a| a == "--release");
    let selftest = args.iter().any(|a| a == "--selftest");
    let result = match args.first().map(String::as_str) {
        Some("bundle") => bundle(release, selftest).map(|p| println!("{}", p.display())),
        Some("dmg") => dmg().map(|p| println!("{}", p.display())),
        Some("icon") => icon(),
        Some("vendor") => vendor(args.get(1).map(String::as_str) == Some("fetch")),
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

const USAGE: &str = "usage: cargo xtask bundle [--release] [--selftest] | dmg | icon | vendor [check|fetch]
       cargo xtask selftest probe | gui [--only <check>] [--scenarios] | resize | bench <file.md>
       cargo xtask coverage [--library]";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// Where cargo builds: `CARGO_TARGET_DIR` if set, as for the measured builds of `coverage`, else `target/`.
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root().join("target"), |dir| root().join(dir))
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed with {status}"))
    }
}

fn stdout(cmd: &mut Command) -> Result<String, String> {
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
fn is_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The version the next build gets, from VERSION: major and minor are raised by hand, the build number by every
/// release build.
fn version() -> Result<String, String> {
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
fn next_build(built: &str) -> Result<(), String> {
    let (head, build) = built.rsplit_once('.').ok_or("no build number")?;
    let build: u64 = build.parse().map_err(|e| format!("build number {build}: {e}"))?;
    let next = format!("{head}.{}", build + 1);
    fs::write(root().join("VERSION"), format!("{next}\n")).map_err(|e| e.to_string())?;
    println!("built {built}, next build {next}");
    Ok(())
}

/// Selftest and debug builds are separate apps: they leave the user's settings, recent documents and Markdown file
/// association alone, and macOS cannot mistake an outdated debug build for Splat when it opens a document.
fn info_plist(version: &str, languages: &[String], variant: Option<&str>) -> String {
    let (bundle_id, rank) = match variant {
        Some(variant) => (format!("{BUNDLE_ID}.{}", variant.to_lowercase()), "None"),
        None => (BUNDLE_ID.to_string(), "Default"),
    };
    let localizations: String = languages
        .iter()
        .map(|l| format!("\t\t<string>{l}</string>\n"))
        .collect();
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
	<string>14.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSPrincipalClass</key>
	<string>NSApplication</string>
	<key>NSHumanReadableCopyright</key>
	<string>Markdown editor</string>
	<key>CFBundleDocumentTypes</key>
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
	</array>
	<key>UTImportedTypeDeclarations</key>
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

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Release builds raise the build number; debug and selftest builds leave it.
fn bundle(release: bool, selftest: bool) -> Result<PathBuf, String> {
    let version = version()?;
    let app = assemble(release, selftest, &version)?;
    sign(&app, signing_identity()?, false)?;
    if release && !selftest {
        next_build(&version)?;
    }
    Ok(app)
}

/// Builds the app bundle, unsigned.
fn assemble(release: bool, selftest: bool, version: &str) -> Result<PathBuf, String> {
    let root = root();
    let mut build = Command::new(env!("CARGO"));
    build.current_dir(&root).args(["build", "--package", PACKAGE]);
    if release {
        build.arg("--release");
    }
    if selftest {
        build.args(["--features", "selftest"]);
    }
    run(&mut build)?;
    let profile = if release { "release" } else { "debug" };
    let target = target_dir().join(profile);
    let variant = if selftest {
        Some("Selftest")
    } else {
        (!release).then_some("Debug")
    };
    let app = target.join(match variant {
        Some(variant) => format!("{APP} {variant}.app"),
        None => format!("{APP}.app"),
    });
    if app.exists() {
        fs::remove_dir_all(&app).map_err(|e| e.to_string())?;
    }
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
    fs::copy(target.join(PACKAGE), macos.join(APP)).map_err(|e| e.to_string())?;
    let plist = contents.join("Info.plist");
    // Every resources/<language>.lproj is a localization; macOS picks one from the user's preferred languages.
    let mut languages: Vec<String> = fs::read_dir(root.join("resources"))
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".lproj").map(str::to_string))
        .collect();
    languages.sort();
    fs::write(&plist, info_plist(version, &languages, variant)).map_err(|e| e.to_string())?;
    run(Command::new("plutil").arg("-lint").arg("-s").arg(&plist))?;
    copy_dir(&root.join("resources"), &resources)?;
    let icns = root.join("assets").join("AppIcon.icns");
    if icns.exists() {
        fs::copy(&icns, resources.join("AppIcon.icns")).map_err(|e| e.to_string())?;
    }
    fs::write(contents.join("PkgInfo"), "APPL????").map_err(|e| e.to_string())?;
    Ok(app)
}

/// Notarization requires the secure timestamp; local builds skip it, since it needs a request to Apple.
fn sign(app: &Path, identity: &str, timestamp: bool) -> Result<(), String> {
    run(Command::new("codesign")
        // Hardened runtime: no injected libraries, no debugger attaching, no writable code pages.
        .args(["--force", "--sign", identity, "--options", "runtime"])
        .arg(if timestamp { "--timestamp" } else { "--timestamp=none" })
        .arg(app))
}

/// Builds the release app signed with the Developer ID, has Apple notarize it, packs it into a disk image with a
/// link to /Applications and has Apple notarize the image too, so that Gatekeeper opens both on other Macs, each with
/// its ticket stapled. Only an image that passed all of this becomes `dist/Splat-<version>.dmg`, the copy that goes into the repository: it replaces the previous one
/// there and in the links of README.md, and the build number goes up.
fn dmg() -> Result<PathBuf, String> {
    let version = version()?;
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
    let app = assemble(true, false, &version)?;
    // A coverage build in target/release, as a manual `cargo llvm-cov` run leaves, would write measurement files on
    // users' Macs.
    let load_commands = stdout(
        Command::new("otool")
            .arg("-l")
            .arg(app.join("Contents/MacOS").join(APP)),
    )?;
    if load_commands.contains("__llvm_prf") {
        return Err("the release binary carries coverage counters; run `cargo clean --release` and try again".into());
    }
    println!("signing {version} with {name}");
    sign(&app, &identity, true)?;
    run(Command::new("codesign").args(["--verify", "--strict"]).arg(&app))?;
    // The image's ticket reaches Gatekeeper only when the image is opened in the Finder; a copy taken out otherwise,
    // as Homebrew installs it, would need a network connection on first launch.
    let archive = app.with_extension("zip");
    run(Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&app)
        .arg(&archive))?;
    notarize(&archive)?;
    fs::remove_file(&archive).map_err(|e| e.to_string())?;
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
    let dist = root().join("dist");
    fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    let file = format!("{APP}-{version}.dmg");
    let published = dist.join(&file);
    // Copied rather than renamed: target/ may be a link to another volume.
    fs::copy(&image, &published).map_err(|e| format!("{}: {e}", published.display()))?;
    fs::remove_file(&image).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(&dist).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != file && image_name_len(&name) == Some(name.len()) {
            run(Command::new("trash").arg(entry.path()))?;
        }
    }
    let readme = root().join("README.md");
    let text = fs::read_to_string(&readme).map_err(|e| format!("{}: {e}", readme.display()))?;
    fs::write(&readme, relink(&text, &file)).map_err(|e| e.to_string())?;
    next_build(&version)?;
    Ok(published)
}

/// Length of the disk image name `Splat.dmg` or `Splat-<version>.dmg` at the start of `s`.
fn image_name_len(s: &str) -> Option<usize> {
    let rest = s.strip_prefix(APP)?;
    if rest.starts_with(".dmg") {
        return Some(APP.len() + 4);
    }
    let rest = rest.strip_prefix('-')?;
    let end = rest.find(".dmg")?;
    is_version(&rest[..end]).then_some(APP.len() + 1 + end + 4)
}

/// `text` with every disk image name in it replaced by `image`.
fn relink(text: &str, image: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(APP) {
        out.push_str(&rest[..at]);
        match image_name_len(&rest[at..]) {
            Some(len) => {
                out.push_str(image);
                rest = &rest[at + len..];
            }
            None => {
                out.push_str(APP);
                rest = &rest[at + APP.len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// SHA-1 hash and name of the first valid Developer ID Application identity in the keychains.
fn developer_id() -> Result<(String, String), String> {
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
fn disk_image(app: &Path) -> Result<PathBuf, String> {
    let out = app.parent().ok_or("the app has no parent directory")?;
    let staging = out.join("dmg-staging");
    let writable = out.join(format!("{APP}-writable.dmg"));
    let image = out.join(format!("{APP}.dmg"));
    for file in [&writable, &image] {
        if file.exists() {
            fs::remove_file(file).map_err(|e| e.to_string())?;
        }
    }
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    run(Command::new("ditto").arg(app).arg(staging.join(format!("{APP}.app"))))?;
    std::os::unix::fs::symlink("/Applications", staging.join("Applications")).map_err(|e| e.to_string())?;
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
    fs::remove_file(&writable).map_err(|e| e.to_string())?;
    fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    Ok(image)
}

/// Finder keeps window size, icon positions and background in the image's .DS_Store, so only Finder itself can
/// write them, on the mounted image. Needs the Automation permission for Finder.
fn lay_out(mount: &Path) -> Result<(), String> {
    let background = root().join("assets").join("dmg-background.png");
    fs::create_dir_all(mount.join(".background")).map_err(|e| e.to_string())?;
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
fn notarize(file: &Path) -> Result<(), String> {
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
fn json_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    rest.split('"').next()
}

/// The local signing certificate, created in the login keychain on first use.
fn signing_identity() -> Result<&'static str, String> {
    let listed = Command::new("security")
        .args(["find-identity", "-p", "codesigning"])
        .output()
        .map_err(|e| e.to_string())?;
    if String::from_utf8_lossy(&listed.stdout).contains(&format!("\"{SIGNING_IDENTITY}\"")) {
        return Ok(SIGNING_IDENTITY);
    }
    let dir = std::env::temp_dir().join(format!("{PACKAGE}-signing-{}", std::process::id()));
    std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700)
        .create(&dir)
        .map_err(|e| e.to_string())?;
    let created = create_identity(&dir);
    let _ = fs::remove_dir_all(&dir);
    created.map(|()| SIGNING_IDENTITY)
}

fn create_identity(dir: &Path) -> Result<(), String> {
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

/// Builds assets/AppIcon.icns and the README icon from assets/icon-1024.png, which must follow Apple's macOS icon
/// template outline.
fn icon() -> Result<(), String> {
    let assets = root().join("assets");
    let master = assets.join("icon-1024.png");
    if !master.exists() {
        return Err(format!("{} is missing", master.display()));
    }
    let iconset = std::env::temp_dir().join(format!("{PACKAGE}-AppIcon.iconset"));
    if iconset.exists() {
        fs::remove_dir_all(&iconset).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&iconset).map_err(|e| e.to_string())?;
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
    fs::remove_dir_all(&iconset).map_err(|e| e.to_string())?;
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

fn sha256(path: &Path) -> Result<String, String> {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| format!("no checksum for {}", path.display()))
}

/// Turns an ES module into a classic script that sets its exports as one global: the render page loads its scripts
/// from files, and WebKit refuses module imports from file URLs. The module keeps its own scope and strict mode.
fn classic_script(path: &Path) -> Result<(), String> {
    if path.extension().is_none_or(|e| e != "js") {
        return Ok(());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
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
    fs::write(path, classic).map_err(|e| e.to_string())
}

/// Checks the vendored scripts against `vendor.lock`, or downloads them from the recorded URLs first.
fn vendor(fetch: bool) -> Result<(), String> {
    let root = root();
    let lock = root.join("vendor.lock");
    let content = fs::read_to_string(&lock).map_err(|e| e.to_string())?;
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
                .args(["-fsSL", "--proto", "=https", "-o"])
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
        fs::write(&lock, out).map_err(|e| e.to_string())?;
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!("changed since vendor.lock: {}", mismatches.join(", ")))
    }
}

/// Builds the selftest bundle and runs one of its checks. `probe` runs without a window; `gui` and `bench` open
/// one and type into it, so they need the Mac to themselves for their duration.
fn run_selftest(args: &[String]) -> Result<(), String> {
    let mode = args.first().map(String::as_str).ok_or(USAGE)?;
    let app = bundle(true, true)?;
    let binary = app.join("Contents/MacOS").join(APP);
    let work = root().join("target/selftest");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&binary);
    // The checks expect the German interface, the default window frame and no windows of an earlier run.
    cmd.args([
        "-AppleLanguages",
        "(de)",
        "-ApplePersistenceIgnoreState",
        "YES",
        "-DocumentWindowFrame",
        "",
    ]);
    match mode {
        "probe" => {
            cmd.env("SPLAT_RENDER_PROBE", "1");
        }
        "gui" => {
            if screen_locked() {
                return Err("the screen is locked: popovers do not close and screen captures fail".into());
            }
            // A copy, since the test edits the document and autosave would write the changes back.
            let file = work.join("QuickReference.md");
            fs::copy(root().join("resources/de.lproj/QuickReference.md"), &file).map_err(|e| e.to_string())?;
            cmd.env("SPLAT_SELFTEST_FILE", &file).env("SPLAT_SELFTEST_OUT", &work);
            if let Some(i) = args.iter().position(|a| a == "--only") {
                cmd.env("SPLAT_SELFTEST_STOP_AFTER", args.get(i + 1).ok_or(USAGE)?);
            }
            if args.iter().any(|a| a == "--scenarios") {
                cmd.env("SPLAT_SELFTEST_SCENARIOS", "1");
            }
        }
        "resize" => {
            if screen_locked() {
                return Err("the screen is locked: the window cannot be resized".into());
            }
            let file = work.join("Breite.md");
            let paragraph =
                "Ein langer Absatz, der in der Spalte umbricht und bei jeder neuen Breite anders. ".repeat(4);
            let text: String = (1..=80)
                .map(|i| format!("## Abschnitt {i}\n\n{paragraph}\n\n"))
                .collect();
            fs::write(&file, text).map_err(|e| e.to_string())?;
            cmd.env("SPLAT_RESIZE_PROBE", "1").env("SPLAT_SELFTEST_FILE", &file);
        }
        "bench" => {
            let file = args.get(1).ok_or(USAGE)?;
            cmd.env("SPLAT_SELFTEST_FILE", file).env("SPLAT_BENCH", "1");
        }
        _ => return Err(USAGE.into()),
    }
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", binary.display()))?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            println!("output in {}", work.display());
            return if status.success() {
                Ok(())
            } else {
                Err(format!("selftest {mode} failed with {status}"))
            };
        }
        if started.elapsed() > std::time::Duration::from_secs(600) {
            let _ = child.kill();
            return Err(format!("selftest {mode} did not finish within 10 minutes"));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Line coverage every module must reach, and all modules together.
const MODULE_FLOOR: f64 = 80.0;
const TOTAL_FLOOR: f64 = 85.0;
/// Measured along with the app, but held to no floor.
const TEST_CODE: [&str; 4] = ["src/editor/selftest", "src/fuzz.rs", "tests/", "xtask/"];
/// Modules held to no floor of their own, and why.
const NO_FLOOR: [(&str, &str); 1] = [(
    "src/main.rs",
    "its panic hook runs only in a crash, which ends the app before it writes measurements",
)];
/// The modules that need no window, which `cargo test` measures alone; CI holds them to the floors.
const LIBRARY: [&str; 3] = ["src/markdown/", "src/preview/", "src/i18n.rs"];

/// Measures line coverage of `cargo test` and the self-tests, or with `library` of `cargo test` alone for the
/// library modules, and fails when a module or the whole falls below its floor. The measured builds go to
/// target/coverage, so measuring code never reaches target/release and a disk image.
fn coverage(library: bool) -> Result<(), String> {
    if !library && screen_locked() {
        return Err("the screen is locked, and the window self-tests need it".into());
    }
    let root = root();
    let target = root.join("target/coverage");
    let mut env = vec![("CARGO_TARGET_DIR".to_string(), target.display().to_string())];
    // Homebrew's Rust lacks the llvm-tools component; then Homebrew's LLVM reads the measurements, while the major
    // versions match.
    if std::env::var_os("LLVM_COV").is_none()
        && !rust_has_llvm_tools()
        && let Ok(prefix) = stdout(Command::new("brew").args(["--prefix", "llvm"]))
        && Path::new(prefix.trim()).join("bin/llvm-cov").exists()
    {
        let bin = Path::new(prefix.trim()).join("bin");
        env.push(("LLVM_COV".into(), bin.join("llvm-cov").display().to_string()));
        env.push(("LLVM_PROFDATA".into(), bin.join("llvm-profdata").display().to_string()));
    }
    let cargo = |args: &[&str], env: &[(String, String)]| {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.current_dir(&root).args(args).envs(env.iter().map(|(k, v)| (k, v)));
        cmd
    };
    let shown = stdout(&mut cargo(&["llvm-cov", "show-env"], &env))?;
    // Without the measuring environment, which would fill the message.
    let run = |mut cmd: Command| {
        let status = cmd.status().map_err(|e| e.to_string())?;
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("`cargo {}` failed with {status}", args.join(" ")))
    };
    env.extend(shown.lines().filter_map(|line| {
        let (key, value) = line.trim_start_matches("export ").split_once('=')?;
        Some((key.to_string(), value.trim_matches('\'').to_string()))
    }));
    run(cargo(&["llvm-cov", "clean", "--workspace"], &env))?;
    run(cargo(&["test", "--release", "--workspace"], &env))?;
    if !library {
        for mode in ["probe", "gui", "resize"] {
            run(cargo(&["xtask", "selftest", mode], &env))?;
        }
    }
    let lcov = target.join("lcov.info");
    let lcov_path = lcov.to_str().ok_or("target path is not UTF-8")?;
    run(cargo(
        &["llvm-cov", "report", "--release", "--lcov", "--output-path", lcov_path],
        &env,
    ))?;
    let text = fs::read_to_string(&lcov).map_err(|e| format!("{}: {e}", lcov.display()))?;
    let root_prefix = format!("{}/", root.display());
    let (report, shortfalls) = judge(&measured_lines(&text, &root_prefix), library);
    print!("{report}");
    if shortfalls.is_empty() {
        Ok(())
    } else {
        Err(format!("coverage below the floor: {}", shortfalls.join(", ")))
    }
}

/// Whether the Rust toolchain brings its own `llvm-cov`, as rustup's llvm-tools component does.
fn rust_has_llvm_tools() -> bool {
    let sysroot = stdout(Command::new("rustc").args(["--print", "sysroot"]));
    let host = stdout(Command::new("rustc").arg("-vV"))
        .ok()
        .and_then(|v| v.lines().find_map(|l| l.strip_prefix("host: ").map(str::to_string)));
    match (sysroot, host) {
        (Ok(sysroot), Some(host)) => Path::new(sysroot.trim())
            .join("lib/rustlib")
            .join(host)
            .join("bin/llvm-cov")
            .exists(),
        _ => false,
    }
}

/// Lines found and lines run per source file of the workspace, from an LCOV report.
fn measured_lines(lcov: &str, root_prefix: &str) -> Vec<(String, u64, u64)> {
    let mut files = Vec::new();
    let mut current: Option<(String, u64, u64)> = None;
    for line in lcov.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            current = path.strip_prefix(root_prefix).map(|p| (p.to_string(), 0, 0));
        } else if let (Some(found), Some(file)) = (line.strip_prefix("LF:"), current.as_mut()) {
            file.1 = found.parse().unwrap_or(0);
        } else if let (Some(hit), Some(file)) = (line.strip_prefix("LH:"), current.as_mut()) {
            file.2 = hit.parse().unwrap_or(0);
        } else if line == "end_of_record"
            && let Some(file) = current.take()
        {
            files.push(file);
        }
    }
    files
}

/// The report, lowest coverage first, and what falls below a floor.
fn judge(files: &[(String, u64, u64)], library: bool) -> (String, Vec<String>) {
    let percent = |found: u64, hit: u64| {
        if found == 0 {
            100.0
        } else {
            hit as f64 * 100.0 / found as f64
        }
    };
    let mut judged: Vec<&(String, u64, u64)> = files
        .iter()
        .filter(|(path, ..)| !TEST_CODE.iter().any(|t| path.starts_with(t)))
        .filter(|(path, ..)| !library || LIBRARY.iter().any(|l| path.starts_with(l)))
        .collect();
    judged.sort_by(|a, b| percent(a.1, a.2).total_cmp(&percent(b.1, b.2)));
    let mut report = String::new();
    let mut shortfalls = Vec::new();
    for (path, found, hit) in &judged {
        let p = percent(*found, *hit);
        let exempt = NO_FLOOR.iter().find(|(m, _)| m == path);
        let mark = match exempt {
            Some((_, why)) => format!("  no floor: {why}"),
            None if p < MODULE_FLOOR => {
                shortfalls.push(format!("{path} {p:.1} %"));
                format!("  below {MODULE_FLOOR} %")
            }
            None => String::new(),
        };
        report.push_str(&format!("{p:6.1} %  {found:5}  {path}{mark}\n"));
    }
    let (found, hit) = judged.iter().fold((0, 0), |(f, h), file| (f + file.1, h + file.2));
    let total = percent(found, hit);
    report.push_str(&format!("{total:6.1} %  {found:5}  all modules\n"));
    if total < TOTAL_FLOOR {
        shortfalls.push(format!("all modules {total:.1} % (floor {TOTAL_FLOOR} %)"));
    }
    (report, shortfalls)
}

/// The login window's lock state, as the window server reports it to the I/O registry.
fn screen_locked() -> bool {
    Command::new("ioreg")
        .args(["-n", "Root", "-d1", "-a"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .and_then(|plist| {
            let at = plist.find("<key>CGSSessionScreenIsLocked</key>")?;
            Some(
                plist[at..]
                    .trim_start_matches("<key>CGSSessionScreenIsLocked</key>")
                    .trim_start()
                    .starts_with("<true/>"),
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_holds_every_module_to_the_floor_and_skips_test_code() {
        let lcov = "SF:/r/src/a.rs\nDA:1,1\nLF:10\nLH:9\nend_of_record\nSF:/r/src/editor/b.rs\nLF:10\nLH:7\n\
                    end_of_record\nSF:/r/src/editor/selftest.rs\nLF:50\nLH:0\nend_of_record\n\
                    SF:/r/src/main.rs\nLF:10\nLH:1\nend_of_record\nSF:/std/lib.rs\nLF:5\nLH:0\nend_of_record\n";
        let files = measured_lines(lcov, "/r/");
        assert_eq!(files.len(), 4);
        let (report, shortfalls) = judge(&files, false);
        assert_eq!(
            shortfalls,
            ["src/editor/b.rs 70.0 %", "all modules 56.7 % (floor 85 %)"]
        );
        assert!(report.contains("no floor") && !report.contains("selftest"), "{report}");
        let (_, library) = judge(
            &measured_lines("SF:/r/src/markdown/x.rs\nLF:4\nLH:4\nend_of_record\n", "/r/"),
            true,
        );
        assert!(library.is_empty());
    }

    #[test]
    fn versions_have_three_numbers() {
        assert!(is_version("1.0.1") && is_version("12.3.456"));
        assert!(!is_version("1.0") && !is_version("1.0.x") && !is_version("1..1") && !is_version("1.0.1.2"));
    }

    #[test]
    fn readme_links_follow_the_new_image_and_leave_the_rest() {
        let text = "[dl](dist/Splat.dmg?raw=true) **[Download Splat-1.0.3.dmg](https://raw.githubusercontent.com/a/splat/main/dist/Splat-1.0.3.dmg)** \
                    Splat.app, `dist/Splat-<version>.dmg`, Splat-1.0.dmg";
        assert_eq!(
            relink(text, "Splat-1.0.4.dmg"),
            "[dl](dist/Splat-1.0.4.dmg?raw=true) **[Download Splat-1.0.4.dmg](https://raw.githubusercontent.com/a/splat/main/dist/Splat-1.0.4.dmg)** \
             Splat.app, `dist/Splat-<version>.dmg`, Splat-1.0.dmg"
        );
        assert_eq!(image_name_len("Splat-1.0.3.dmg"), Some(15));
        assert_eq!(image_name_len("Splat.dmg"), Some(9));
        assert_eq!(image_name_len("Splat-notes.dmg"), None);
    }
}
