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
            let part = Part::of(args);
            if fewest_checks(mode, part).is_some() {
                println!("{}", report.summary());
            }
            return verdict(mode, part, status.success(), &report).map_err(|e| format!("selftest {mode}: {e}"));
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
    /// Checks that could not say anything in this run: neither passed nor failed, and listed so nobody takes them for either.
    skipped: Vec<String>,
    done: bool,
}

impl Report {
    pub(crate) fn note(&mut self, line: &str) {
        if line.starts_with("SELFTEST PASS ") {
            self.passed += 1;
        } else if let Some(failure) = line.strip_prefix("SELFTEST FAIL ") {
            self.failed.push(failure.to_string());
        } else if let Some(skip) = line.strip_prefix("SELFTEST SKIP ") {
            self.skipped.push(skip.to_string());
        } else if line.starts_with("SELFTEST DONE ") {
            self.done = true;
        }
    }

    pub(crate) fn summary(&self) -> String {
        let mut summary = format!(
            "{} checks passed, {} failed, {} skipped",
            self.passed,
            self.failed.len(),
            self.skipped.len()
        );
        for skip in &self.skipped {
            summary.push_str("\n  skipped: ");
            summary.push_str(skip);
        }
        summary
    }
}

/// How much of its checks a mode was told to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Whole,
    /// `gui --scenarios`, without the click-through in front.
    Scenarios,
    /// `gui --stop-after`, which ends at a check of the caller's choice.
    StopsEarly,
}

impl Part {
    fn of(args: &[String]) -> Part {
        if args.iter().any(|a| a == "--stop-after") {
            Part::StopsEarly
        } else if args.iter().any(|a| a == "--scenarios") {
            Part::Scenarios
        } else {
            Part::Whole
        }
    }
}

/// About nine tenths of the checks a run makes: an exit code alone would also pass a run that a stray environment variable
/// ended before it began, and `--scenarios` must not be enough for a whole run. Skipped checks do not count, so a run that
/// skips many shows here.
pub(crate) fn fewest_checks(mode: &str, part: Part) -> Option<usize> {
    match (mode, part) {
        ("gui", Part::StopsEarly) => Some(1),
        ("gui", Part::Scenarios) => Some(GUI_SCENARIOS),
        ("gui", Part::Whole) => Some(GUI_WHOLE),
        ("probe", _) => Some(PROBE),
        ("resize", _) => Some(RESIZE),
        _ => None,
    }
}

/// Nine of ten of the checks a run makes, of which an inactive app skips some: 144, 168 and 253.
const PROBE: usize = 130;
const RESIZE: usize = 2;
const GUI_SCENARIOS: usize = 151;
const GUI_WHOLE: usize = 228;

pub(crate) fn verdict(mode: &str, part: Part, exited_well: bool, report: &Report) -> Result<(), String> {
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
    if let Some(fewest) = fewest_checks(mode, part) {
        if !report.done {
            return Err("the app ended before reporting that it was done".into());
        }
        if report.passed < fewest {
            return Err(format!(
                "only {} checks passed, at least {fewest} were expected",
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

    fn report(lines: &str) -> Report {
        let mut r = Report::default();
        lines.lines().for_each(|l| r.note(l));
        r
    }

    fn passes(n: usize) -> String {
        format!("{}SELFTEST DONE failures=0", "SELFTEST PASS x\n".repeat(n))
    }

    #[test]
    fn a_run_passes_only_when_it_says_it_finished_all_its_checks() {
        assert!(verdict("probe", Part::Whole, true, &report(&passes(PROBE))).is_ok());
        let unfinished = "SELFTEST PASS x\n".repeat(PROBE);
        assert!(
            verdict("probe", Part::Whole, true, &report(&unfinished))
                .unwrap_err()
                .contains("before reporting")
        );
        assert!(verdict("probe", Part::Whole, true, &report("MENU a | b")).is_err());
        assert_eq!(
            verdict("probe", Part::Whole, true, &report(&passes(1))).unwrap_err(),
            format!("only 1 checks passed, at least {PROBE} were expected")
        );
        let failed = verdict(
            "probe",
            Part::Whole,
            false,
            &report("SELFTEST FAIL one: why\nSELFTEST FAIL two: how"),
        )
        .unwrap_err();
        assert!(failed.contains("one: why") && failed.contains("two: how"), "{failed}");
        assert_eq!(
            verdict("probe", Part::Whole, false, &report(&passes(PROBE))).unwrap_err(),
            "the app ended with an error"
        );
    }

    #[test]
    fn every_mode_and_part_has_its_own_floor() {
        let at_floor = |mode, part| fewest_checks(mode, part).map(|n| verdict(mode, part, true, &report(&passes(n))));
        let below_floor =
            |mode, part| fewest_checks(mode, part).map(|n| verdict(mode, part, true, &report(&passes(n - 1))));
        for (mode, part) in [
            ("probe", Part::Whole),
            ("resize", Part::Whole),
            ("gui", Part::Whole),
            ("gui", Part::Scenarios),
            ("gui", Part::StopsEarly),
        ] {
            assert_eq!(at_floor(mode, part), Some(Ok(())), "{mode} {part:?}");
            assert!(below_floor(mode, part).is_some_and(|v| v.is_err()), "{mode} {part:?}");
        }
        assert_eq!(fewest_checks("bench", Part::Whole), None);
        let scenarios = fewest_checks("gui", Part::Scenarios).unwrap();
        assert!(
            verdict("gui", Part::Whole, true, &report(&passes(scenarios))).is_err(),
            "what `--scenarios` reaches is not a whole run"
        );
        assert!(
            verdict("resize", Part::Whole, true, &report(&passes(1))).is_err(),
            "a resize run that ended before its second check"
        );
        assert!(
            verdict("gui", Part::StopsEarly, true, &report(&passes(1))).is_ok(),
            "a run told to stop early"
        );
    }

    #[test]
    fn the_arguments_say_which_part_runs() {
        let part = |args: &[&str]| Part::of(&args.iter().map(ToString::to_string).collect::<Vec<_>>());
        assert_eq!(part(&["gui"]), Part::Whole);
        assert_eq!(part(&["gui", "--scenarios"]), Part::Scenarios);
        assert_eq!(part(&["gui", "--stop-after", "a check"]), Part::StopsEarly);
        assert_eq!(
            part(&["gui", "--scenarios", "--stop-after", "a check"]),
            Part::StopsEarly
        );
    }

    #[test]
    fn a_skipped_check_neither_passes_nor_fails() {
        let skipping = format!("SELFTEST SKIP needs the app: not active\n{}", passes(PROBE));
        let run = report(&skipping);
        assert!(verdict("probe", Part::Whole, true, &run).is_ok(), "no failure");
        assert_eq!(
            run.summary(),
            format!("{PROBE} checks passed, 0 failed, 1 skipped\n  skipped: needs the app: not active")
        );
        let only_skips = format!(
            "{}SELFTEST DONE failures=0",
            "SELFTEST SKIP x: not active\n".repeat(PROBE)
        );
        assert!(
            verdict("probe", Part::Whole, true, &report(&only_skips))
                .unwrap_err()
                .contains("only 0 checks passed"),
            "and no pass"
        );
    }
}
