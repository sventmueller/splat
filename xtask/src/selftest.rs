//! Runs the app with the self-test built in.

use std::fs;
use std::process::Command;

use crate::*;

pub(crate) const LSREGISTER: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// Builds the selftest bundle and runs one of its checks. `probe` runs without a window; `gui` and `bench` open
/// one and type into it, so they need the Mac to themselves for their duration.
pub(crate) fn run_selftest(args: &[String]) -> Result<(), String> {
    let mode = args.first().map(String::as_str).ok_or(USAGE)?;
    let app = bundle(true, true)?;
    // Launched by its path, the app would stay unknown to Launch Services, and with it the probe type.
    run(Command::new(LSREGISTER).arg("-f").arg(&app))?;
    let binary = app.join("Contents/MacOS").join(APP);
    let work = root().join("target/selftest");
    let _ = fs::remove_dir_all(&work);
    files::create_dir_all(&work)?;
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
            files::copy(root().join("resources/de.lproj/QuickReference.md"), &file)?;
            cmd.env("SPLAT_SELFTEST_FILE", &file).env("SPLAT_SELFTEST_OUT", &work);
            if let Some(i) = args.iter().position(|a| a == "--stop-after") {
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
            files::write(&file, text)?;
            cmd.env("SPLAT_RESIZE_PROBE", "1").env("SPLAT_SELFTEST_FILE", &file);
        }
        "bench" => {
            let file = args.get(1).ok_or(USAGE)?;
            cmd.env("SPLAT_SELFTEST_FILE", file).env("SPLAT_BENCH", "1");
        }
        _ => return Err(USAGE.into()),
    }
    let mut child = cmd
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    let output = child.stderr.take().ok_or("the selftest has no output")?;
    let report = std::thread::spawn(move || {
        let mut report = Report::default();
        for line in std::io::BufRead::lines(std::io::BufReader::new(output)).map_while(Result::ok) {
            eprintln!("{line}");
            report.note(&line);
        }
        report
    });
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("{}: {e}", binary.display()))? {
            println!("output in {}", work.display());
            let report = report.join().map_err(|_| "reading the selftest's output failed")?;
            return verdict(mode, status.success(), &report).map_err(|e| format!("selftest {mode}: {e}"));
        }
        if started.elapsed() > std::time::Duration::from_secs(600) {
            let _ = child.kill();
            return Err(format!("selftest {mode} did not finish within 10 minutes"));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// What a selftest said about itself.
#[derive(Default, Debug)]
pub(crate) struct Report {
    passed: usize,
    failed: Vec<String>,
    done: bool,
}

impl Report {
    pub(crate) fn note(&mut self, line: &str) {
        if line.starts_with("SELFTEST PASS ") {
            self.passed += 1;
        } else if let Some(failure) = line.strip_prefix("SELFTEST FAIL ") {
            self.failed.push(failure.to_string());
        } else if line.starts_with("SELFTEST DONE ") {
            self.done = true;
        }
    }
}

/// The checks a whole run makes at least, far below their number: an exit code alone would also pass a run that a stray
/// environment variable ended before it began.
pub(crate) fn fewest_checks(mode: &str) -> Option<usize> {
    match mode {
        "gui" if std::env::var_os("SPLAT_SELFTEST_STOP_AFTER").is_none() => Some(150),
        "probe" => Some(20),
        _ => None,
    }
}

pub(crate) fn verdict(mode: &str, exited_well: bool, report: &Report) -> Result<(), String> {
    if !report.failed.is_empty() {
        return Err(format!(
            "{} checks failed:\n  {}",
            report.failed.len(),
            report.failed.join("\n  ")
        ));
    }
    if !exited_well {
        return Err("the app ended with an error".into());
    }
    if let Some(fewest) = fewest_checks(mode) {
        if !report.done {
            return Err("the app ended before reporting that it was done".into());
        }
        if report.passed < fewest {
            return Err(format!(
                "only {} checks ran, at least {fewest} were expected",
                report.passed
            ));
        }
    }
    Ok(())
}

/// The login window's lock state, as the window server reports it to the I/O registry.
pub(crate) fn screen_locked() -> bool {
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
    fn a_run_passes_only_when_it_says_it_finished_all_its_checks() {
        let report = |lines: &str| {
            let mut r = Report::default();
            lines.lines().for_each(|l| r.note(l));
            r
        };
        let many = "SELFTEST PASS x\n".repeat(20);
        assert!(verdict("probe", true, &report(&format!("{many}SELFTEST DONE failures=0"))).is_ok());
        assert!(
            verdict("probe", true, &report(&many))
                .unwrap_err()
                .contains("before reporting")
        );
        assert!(verdict("probe", true, &report("MENU a | b")).is_err());
        assert!(
            verdict("probe", true, &report("SELFTEST PASS x\nSELFTEST DONE failures=0"))
                .unwrap_err()
                .contains("only 1 checks")
        );
        let failed = verdict(
            "probe",
            false,
            &report("SELFTEST FAIL one: why\nSELFTEST FAIL two: how"),
        )
        .unwrap_err();
        assert!(failed.contains("one: why") && failed.contains("two: how"), "{failed}");
        assert!(verdict("probe", false, &report(&format!("{many}SELFTEST DONE failures=0"))).is_err());
        assert!(verdict("resize", true, &report("")).is_ok());
    }
}
