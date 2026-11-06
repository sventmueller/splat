//! Line coverage of all tests, held to a floor per module.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::*;

/// Line coverage every module must reach, and all modules together.
#[derive(Clone, Copy)]
pub(crate) struct Floors {
    module: f64,
    total: f64,
}

/// `make coverage`, with the window code and the self-tests.
pub(crate) const FULL: Floors = Floors {
    module: 84.0,
    total: 96.0,
};

/// `make coverage-library`, for the modules without a window, by `cargo test` alone.
pub(crate) const LIBRARY_ONLY: Floors = Floors {
    module: 90.0,
    total: 97.0,
};

/// Measured along with the app, but held to no floor.
pub(crate) const TEST_CODE: [&str; 5] = [
    "src/editor/selftest",
    "crates/splat-testkit/",
    "tests/",
    "vendor/",
    "xtask/",
];

/// Modules held to no floor of their own, and why.
pub(crate) const NO_FLOOR: [(&str, &str); 1] = [(
    "src/main.rs",
    "its panic hook runs only in a crash, which ends the app before it writes measurements",
)];

/// The modules that need no window, which `cargo test` measures alone; CI holds them to the floors.
pub(crate) const LIBRARY: [&str; 7] = [
    "crates/splat-file-state/",
    "crates/splat-git/",
    "crates/splat-import/",
    "crates/splat-markdown/",
    "crates/splat-notes/",
    "crates/splat-preview/",
    "crates/splat-i18n/",
];

/// Measures line coverage of `cargo test` and the self-tests, or with `library` of `cargo test` alone for the
/// library modules, and fails when a module or the whole falls below its floor. The measured builds go to
/// target/coverage, so measuring code never reaches target/release and a disk image.
pub(crate) fn coverage(library: bool) -> Result<(), String> {
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
        let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
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
        let _ = fs::remove_dir_all(crate::selftest::skipped_dir());
        for mode in [&["probe"][..], &["gui", "--in-back"], &["resize"], &["typing"]] {
            let args: Vec<&str> = ["xtask", "selftest"].iter().chain(mode).copied().collect();
            run(cargo(&args, &env))?;
        }
    }
    let lcov = target.join("lcov.info");
    let lcov_path = lcov.to_str().ok_or("target path is not UTF-8")?;
    run(cargo(
        &[
            "llvm-cov",
            "report",
            "--workspace",
            "--release",
            "--lcov",
            "--output-path",
            lcov_path,
        ],
        &env,
    ))?;
    let text = fs::read_to_string(&lcov).map_err(|e| format!("{}: {e}", lcov.display()))?;
    let root_prefix = format!("{}/", root.display());
    let (report, shortfalls) = judge(&measured_lines(&text, &root_prefix), library);
    print!("{report}");
    if !library {
        print!(
            "{}",
            skipped_report(&crate::selftest::skipped_by_mode(&["probe", "gui", "resize", "typing"]))
        );
    }
    if shortfalls.is_empty() {
        Ok(())
    } else {
        Err(format!("coverage below the floor: {}", shortfalls.join(", ")))
    }
}

/// The self-test checks that said nothing in this run: the verdict holds without them, so it names them.
fn skipped_report(skipped: &[(String, Vec<String>)]) -> String {
    let total: usize = skipped.iter().map(|(_, names)| names.len()).sum();
    if total == 0 {
        return "self-tests: no check skipped\n".to_string();
    }
    let mut out = format!("self-tests: {total} checks skipped, not part of this verdict\n");
    for (mode, names) in skipped {
        for name in names {
            out.push_str(&format!("  {mode}: {name}\n"));
        }
    }
    out
}

/// Whether the Rust toolchain brings its own `llvm-cov`, as rustup's llvm-tools component does.
pub(crate) fn rust_has_llvm_tools() -> bool {
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
pub(crate) fn measured_lines(lcov: &str, root_prefix: &str) -> Vec<(String, u64, u64)> {
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
pub(crate) fn judge(files: &[(String, u64, u64)], library: bool) -> (String, Vec<String>) {
    let floors = if library { LIBRARY_ONLY } else { FULL };
    let percent = |found: u64, hit: u64| {
        if found == 0 {
            100.0
        } else {
            hit as f64 * 100.0 / found as f64
        }
    };
    let mut judged: Vec<&(String, u64, u64)> = files
        .iter()
        .filter(|(path, ..)| !TEST_CODE.iter().any(|t| path.starts_with(t)) && !path.contains("/tests/"))
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
            None if p < floors.module => {
                shortfalls.push(format!("{path} {p:.1} %"));
                format!("  below {} %", floors.module)
            }
            None => String::new(),
        };
        report.push_str(&format!("{p:6.1} %  {found:5}  {path}{mark}\n"));
    }
    let (found, hit) = judged.iter().fold((0, 0), |(f, h), file| (f + file.1, h + file.2));
    // A report that names none of the sources says nothing about them.
    if found == 0 {
        shortfalls.push("no measured line in any module".to_string());
    }
    let total = percent(found, hit);
    report.push_str(&format!("{total:6.1} %  {found:5}  all modules\n"));
    if total < floors.total {
        shortfalls.push(format!("all modules {total:.1} % (floor {} %)", floors.total));
    }
    (report, shortfalls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_without_measured_lines_fails() {
        let (_, shortfalls) = judge(
            &measured_lines("SF:/elsewhere/a.rs\nLF:5\nLH:5\nend_of_record\n", "/r/"),
            false,
        );
        assert_eq!(shortfalls, ["no measured line in any module"]);
    }

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
            ["src/editor/b.rs 70.0 %", "all modules 56.7 % (floor 96 %)"]
        );
        assert!(report.contains("no floor") && !report.contains("selftest"), "{report}");
    }

    #[test]
    fn the_library_alone_is_held_to_higher_floors() {
        let judged = |lcov: &str, library: bool| judge(&measured_lines(lcov, "/r/"), library).1;
        let full_marks = "SF:/r/crates/splat-markdown/src/x.rs\nLF:4\nLH:4\nend_of_record\n";
        assert!(judged(full_marks, true).is_empty());
        let weak_module = "SF:/r/crates/splat-markdown/src/x.rs\nLF:100\nLH:89\nend_of_record\n\
                           SF:/r/crates/splat-preview/src/y.rs\nLF:900\nLH:900\nend_of_record\n";
        assert_eq!(judged(weak_module, true), ["crates/splat-markdown/src/x.rs 89.0 %"]);
        assert!(
            judged(weak_module, false).is_empty(),
            "the full measurement keeps its floors"
        );
        let weak_total = "SF:/r/crates/splat-markdown/src/x.rs\nLF:100\nLH:96\nend_of_record\n";
        assert_eq!(judged(weak_total, true), ["all modules 96.0 % (floor 97 %)"]);
        let at_the_floors = "SF:/r/crates/splat-markdown/src/x.rs\nLF:100\nLH:90\nend_of_record\n\
                             SF:/r/crates/splat-preview/src/y.rs\nLF:900\nLH:880\nend_of_record\n";
        assert!(judged(at_the_floors, true).is_empty());
        let window_code = "SF:/r/src/editor/w.rs\nLF:100\nLH:0\nend_of_record\n";
        assert_eq!(
            judged(window_code, true),
            ["no measured line in any module"],
            "the library measurement looks at nothing else"
        );
    }

    #[test]
    fn the_verdict_names_what_the_self_tests_skipped() {
        assert_eq!(skipped_report(&[]), "self-tests: no check skipped\n");
        let skipped = [
            (
                "gui".to_string(),
                vec!["a: not active".to_string(), "b: no pdftotext".to_string()],
            ),
            ("typing".to_string(), vec!["c: not in front".to_string()]),
        ];
        assert_eq!(
            skipped_report(&skipped),
            "self-tests: 3 checks skipped, not part of this verdict\n  gui: a: not active\n  gui: b: no pdftotext\n  typing: c: not in front\n"
        );
    }
}
