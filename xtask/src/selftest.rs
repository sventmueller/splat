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
    let app = bundle(Build::Selftest { release: true })?;
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
    set_up_mode(&mut cmd, mode, args, &work)?;
    let mut child = cmd
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    let output = child.stderr.take().ok_or("the selftest has no output")?;
    let profile = work.join("profile.txt");
    let mut sampler = if mode == "bench" && args.iter().any(|a| a == "--profile") {
        // A millisecond between samples; `-mayDie` keeps what it has when the app quits before the ten minutes.
        let sampler = Command::new("sample")
            .arg(child.id().to_string())
            .args(["600", "1", "-mayDie", "-file"])
            .arg(&profile)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("sample: {e}"))?;
        Some(sampler)
    } else {
        None
    };
    let report = std::thread::spawn(move || {
        let mut report = Report::default();
        for line in std::io::BufRead::lines(std::io::BufReader::new(output)).map_while(Result::ok) {
            eprintln!("{line}");
            report.note(&line);
        }
        report
    });
    let started = std::time::Instant::now();
    let mut asked_to_front = 0;
    let mut installer_offered = None;
    loop {
        ask_launchd(&mut installer_offered, mode, started, child.id());
        // Started from a shell, the app cannot bring itself to the front while another is there.
        if mode == "typing" && asked_to_front < 8 && started.elapsed().as_secs() > asked_to_front {
            asked_to_front += 1;
            let _ = Command::new("open").arg("-a").arg(&app).status();
        }
        if let Some(status) = child.try_wait().map_err(|e| format!("{}: {e}", binary.display()))? {
            if mode == "probe" {
                let _ = hdiutil(&["detach", "-force"], &work.join(FULL_DISK));
            }
            println!("output in {}", work.display());
            if let Some(sampler) = sampler.as_mut() {
                sampler.wait().map_err(|e| format!("sample: {e}"))?;
                println!("call tree of the run in {}", profile.display());
            }
            let report = report.join().map_err(|_| "reading the selftest's output failed")?;
            let part = Part::of(args);
            if listed_checks(mode, part).is_some() {
                println!("{}", report.summary());
            }
            files::create_dir_all(skipped_dir())?;
            files::write(skipped_dir().join(format!("{mode}.txt")), report.skipped.join("\n"))?;
            if let Some(budget) = keystroke_budget(args)? {
                within_budget(&report, budget)?;
            }
            if args.iter().any(|a| a == "--record")
                && report.done
                && report.failed.is_empty()
                && let Some(file) = list_file(mode, part)
            {
                // A run of the whole lists on its own only what the scenarios, which it runs as well, do not.
                let also_listed = if (mode, part) == ("gui", Part::Whole) {
                    include_str!("../checks/scenarios.txt")
                } else {
                    ""
                };
                files::write(root().join("xtask/checks").join(file), recorded(&report, also_listed))?;
                if mode == "gui" {
                    keep_click_through_entries(&work)?;
                }
                return Ok(());
            }
            installer_offered
                .map_or(Ok(()), |(_, r)| r)
                .map_err(|e| format!("selftest {mode}: {e}"))?;
            return verdict(mode, part, status.success(), &report).map_err(|e| format!("selftest {mode}: {e}"));
        }
        if started.elapsed() > std::time::Duration::from_secs(600) {
            let _ = child.kill();
            return Err(format!("selftest {mode} did not finish within 10 minutes"));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// The programs a self-test may have the runner start, which the sandboxed app cannot.
const RUN_FOR_THE_APP: [&str; 3] = ["git", "/opt/homebrew/bin/pdftotext", "/usr/local/bin/pdftotext"];

/// Runs a command the app asked for, in the folder of the file the answer goes to: the exit status on the first line, then
/// what the command wrote. The file appears whole or not at all.
fn run_for_the_app(request: &str) {
    let mut parts = request.split('\u{1f}');
    let (Some(answer), Some(program)) = (parts.next(), parts.next()) else {
        return;
    };
    let answer = std::path::Path::new(answer);
    let Some(dir) = answer.parent().filter(|_| RUN_FOR_THE_APP.contains(&program)) else {
        return;
    };
    let text = match Command::new(program).args(parts).current_dir(dir).output() {
        Ok(output) => format!(
            "{}\n{}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout)
        ),
        Err(_) => "-1\n".to_string(),
    };
    let partial = answer.with_extension("partial");
    if fs::write(&partial, text).is_ok() {
        let _ = fs::rename(&partial, answer);
    }
}

/// Asks launchd about the installer service once a second of the probe until it is offered, since launchd learns an
/// app's services only some time after its start.
fn ask_launchd(answer: &mut Option<(u64, Result<(), String>)>, mode: &str, started: std::time::Instant, pid: u32) {
    let second = started.elapsed().as_secs();
    let due = match answer {
        None => true,
        Some((asked, result)) => result.is_err() && *asked < second,
    };
    if mode == "probe" && due {
        *answer = Some((second, launchd_offers_installer(pid)));
    }
}

/// Whether launchd lets the process reach Sparkle's installer service, without which no update installs.
fn launchd_offers_installer(pid: u32) -> Result<(), String> {
    let listed = stdout(Command::new("launchctl").arg("print").arg(format!("pid/{pid}")))?;
    if listed.contains("org.sparkle-project.InstallerLauncher") {
        Ok(())
    } else {
        Err(
            "launchd does not offer the app Sparkle's installer service, so no update could install; see \"Updates \
             at a used path\" in docs/development.md"
                .into(),
        )
    }
}

/// Ends a process of the app's as a crash would, which the sandboxed app cannot do itself.
fn end_process(pid: &str) {
    if pid.parse::<u32>().is_ok_and(|pid| pid > 0) {
        let _ = Command::new("/bin/kill").args(["-KILL", pid]).status();
    }
}

/// Where the small disk the probe saves to until it is full is mounted, under the run's folder.
const FULL_DISK: &str = "Klein";

fn hdiutil(args: &[&str], path: &std::path::Path) -> bool {
    Command::new("/usr/bin/hdiutil")
        .args(args)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A disk image of 2 MB, mounted under `work`: the sandboxed app can make none, and the probe fills it while saving.
fn full_disk(work: &std::path::Path) -> Option<std::path::PathBuf> {
    let image = work.join("Klein.dmg");
    let mount = work.join(FULL_DISK);
    let made = hdiutil(
        &["create", "-size", "2m", "-fs", "HFS+", "-volname", "SplatProbe"],
        &image,
    ) && Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-noautoopen", "-mountpoint"])
        .arg(&mount)
        .arg(&image)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    made.then_some(mount)
}

/// The texts the click-through groups start from, as a recording run wrote them, for the next build to take in.
fn keep_click_through_entries(work: &std::path::Path) -> Result<(), String> {
    let written = work.join("click-through");
    if !written.exists() {
        return Ok(());
    }
    let kept = root().join("xtask/checks/click-through");
    files::create_dir_all(&kept)?;
    for entry in files::read_dir(&written)?.flatten() {
        files::copy(entry.path(), kept.join(entry.file_name()))?;
    }
    Ok(())
}

/// Where each mode leaves the names of the checks it skipped, for a run of several modes to report them together.
pub(crate) fn skipped_dir() -> std::path::PathBuf {
    root().join("target/selftest-skipped")
}

/// The checks the modes skipped, one line per mode that skipped any, from the lists in [`skipped_dir`].
pub(crate) fn skipped_by_mode(modes: &[&str]) -> Vec<(String, Vec<String>)> {
    modes
        .iter()
        .filter_map(|mode| {
            let text = fs::read_to_string(skipped_dir().join(format!("{mode}.txt"))).ok()?;
            let names: Vec<String> = text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
            (!names.is_empty()).then(|| ((*mode).to_string(), names))
        })
        .collect()
}

/// What `--keystroke-ms` allows a keystroke in the window, if anything.
fn keystroke_budget(args: &[String]) -> Result<Option<f64>, String> {
    let Some(at) = args.iter().position(|a| a == "--keystroke-ms") else {
        return Ok(None);
    };
    let value = args.get(at + 1).ok_or(USAGE)?;
    value
        .parse()
        .map(Some)
        .map_err(|_| format!("--keystroke-ms {value}: no number"))
}

/// The fastest keystroke says what the code costs; a slow one may have met a background job at its end.
fn within_budget(report: &Report, budget: f64) -> Result<(), String> {
    let fastest = report.keystrokes.iter().copied().fold(f64::INFINITY, f64::min);
    if fastest.is_infinite() {
        return Err("the benchmark reported no keystroke".into());
    }
    println!("fastest keystroke {fastest:.1} ms of at most {budget:.1} ms");
    if fastest > budget {
        return Err(format!("a keystroke took {fastest:.1} ms, more than {budget:.1} ms"));
    }
    Ok(())
}

/// What a mode needs of the app's environment, and the document it works on.
fn set_up_mode(cmd: &mut Command, mode: &str, args: &[String], work: &std::path::Path) -> Result<(), String> {
    match mode {
        "probe" => {
            cmd.env("SPLAT_RENDER_PROBE", "1");
            if let Some(mount) = full_disk(work) {
                cmd.env("SPLAT_FULL_DISK", mount);
            }
        }
        "gui" => {
            // A copy, since the test edits the document and autosave would write the changes back.
            let file = work.join("QuickReference.md");
            files::copy(root().join("resources/de.lproj/QuickReference.md"), &file)?;
            cmd.env("SPLAT_SELFTEST_FILE", &file).env("SPLAT_SELFTEST_OUT", work);
            if let Some(i) = args.iter().position(|a| a == "--stop-after") {
                cmd.env("SPLAT_SELFTEST_STOP_AFTER", args.get(i + 1).ok_or(USAGE)?);
            }
            if let Some(i) = args.iter().position(|a| a == "--only") {
                cmd.env("SPLAT_SELFTEST_SCENARIOS", "1");
                cmd.env("SPLAT_SELFTEST_ONLY", args.get(i + 1).ok_or(USAGE)?);
            }
            if args.iter().any(|a| a == "--scenarios") {
                cmd.env("SPLAT_SELFTEST_SCENARIOS", "1");
            }
            if args.iter().any(|a| a == "--in-back") {
                cmd.env("SPLAT_SELFTEST_IN_BACK", "1");
            }
            if args.iter().any(|a| a == "--record") {
                cmd.env("SPLAT_SELFTEST_RECORD", "1");
            }
        }
        "resize" => {
            let file = work.join("Breite.md");
            let paragraph =
                "Ein langer Absatz, der in der Spalte umbricht und bei jeder neuen Breite anders. ".repeat(4);
            let text: String = (1..=80)
                .map(|i| format!("## Abschnitt {i}\n\n{paragraph}\n\n"))
                .collect();
            files::write(&file, text)?;
            cmd.env("SPLAT_RESIZE_PROBE", "1").env("SPLAT_SELFTEST_FILE", &file);
        }
        "typing" => {
            let file = work.join("Tippen.md");
            let paragraph = "Ein Absatz, an dessen Ende weitergetippt wird. ".repeat(4);
            files::write(&file, format!("# Tippen\n\n{paragraph}\n"))?;
            cmd.env("SPLAT_TYPING_PROBE", "1").env("SPLAT_SELFTEST_FILE", &file);
            if args.iter().any(|a| a == "--by-hand") {
                cmd.env("SPLAT_TYPING_BY_HAND", "1");
            }
        }
        "bench" => {
            let file = args.get(1).ok_or(USAGE)?;
            cmd.env("SPLAT_SELFTEST_FILE", file).env("SPLAT_BENCH", "1");
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

/// What a selftest said about itself.
#[derive(Default, Debug)]
pub(crate) struct Report {
    /// How often each check passed: a check of one name in two places must not hide that one of them went away.
    passed: std::collections::BTreeMap<String, usize>,
    failed: Vec<String>,
    /// Checks that could not say anything in this run: neither passed nor failed, and listed so nobody takes them for either.
    skipped: Vec<String>,
    /// In milliseconds, as the benchmark reports them.
    keystrokes: Vec<f64>,
    done: bool,
}

impl Report {
    pub(crate) fn note(&mut self, line: &str) {
        if let Some(pid) = line.strip_prefix("SELFTEST END-PROCESS ") {
            end_process(pid);
        } else if let Some(request) = line.strip_prefix("SELFTEST RUN ") {
            run_for_the_app(request);
        } else if let Some(name) = line.strip_prefix("SELFTEST PASS ") {
            *self.passed.entry(name.to_string()).or_default() += 1;
        } else if let Some(failure) = line.strip_prefix("SELFTEST FAIL ") {
            self.failed.push(failure.to_string());
        } else if let Some(skip) = line.strip_prefix("SELFTEST SKIP ") {
            self.skipped.push(skip.to_string());
        } else if line.starts_with("SELFTEST DONE ") {
            self.done = true;
        } else if let Some(ms) = line
            .strip_prefix("BENCH   another one: ")
            .and_then(|rest| rest.strip_suffix(" ms"))
            .and_then(|ms| ms.parse().ok())
        {
            self.keystrokes.push(ms);
        }
    }

    pub(crate) fn summary(&self) -> String {
        let mut summary = format!(
            "{} checks passed, {} failed, {} skipped",
            self.passed.values().sum::<usize>(),
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
    /// `gui --stop-after` or `--only`, which end at a check or run a group of the caller's choice.
    StopsEarly,
}

impl Part {
    fn of(args: &[String]) -> Part {
        if args.iter().any(|a| a == "--stop-after" || a == "--only") {
            Part::StopsEarly
        } else if args.iter().any(|a| a == "--scenarios") {
            Part::Scenarios
        } else {
            Part::Whole
        }
    }
}

/// The checks a run has to make, by name: an exit code alone would also pass a run that a stray environment variable ended
/// before it began, and a count would not notice one check that went away with another that came. `--record` writes the
/// lists anew from a run.
fn listed_checks(mode: &str, part: Part) -> Option<&'static str> {
    match (mode, part) {
        ("gui", Part::Scenarios) => Some(include_str!("../checks/scenarios.txt")),
        ("gui", Part::Whole) => Some(concat!(
            include_str!("../checks/gui.txt"),
            include_str!("../checks/scenarios.txt")
        )),
        ("gui", Part::StopsEarly) => Some(""),
        ("probe", _) => Some(include_str!("../checks/probe.txt")),
        ("resize", _) => Some(include_str!("../checks/resize.txt")),
        ("typing", _) => Some(include_str!("../checks/typing.txt")),
        _ => None,
    }
}

/// The file of `listed_checks`, for writing it anew.
fn list_file(mode: &str, part: Part) -> Option<&'static str> {
    match (mode, part) {
        ("gui", Part::Scenarios) => Some("scenarios.txt"),
        ("gui", Part::Whole) => Some("gui.txt"),
        ("probe", _) => Some("probe.txt"),
        ("resize", _) => Some("resize.txt"),
        ("typing", _) => Some("typing.txt"),
        _ => None,
    }
}

pub(crate) fn verdict(mode: &str, part: Part, exited_well: bool, report: &Report) -> Result<(), String> {
    judged(listed_checks(mode, part), exited_well, report)
}

/// Whether a run went well that had to make the checks `listed`, one name per line. A check that says it could not tell in
/// this run counts as made, and the summary names it.
fn judged(listed: Option<&str>, exited_well: bool, report: &Report) -> Result<(), String> {
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
    let Some(listed) = listed else {
        return Ok(());
    };
    if !report.done {
        return Err("the app ended before reporting that it was done".into());
    }
    let skipped = |name: &str| {
        report
            .skipped
            .iter()
            .any(|s| s.strip_prefix(name).is_some_and(|r| r.starts_with(": ")))
    };
    let mut wanted: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for name in listed.lines() {
        *wanted.entry(name).or_default() += 1;
    }
    let passed = |name: &str| report.passed.get(name).copied().unwrap_or(0);
    let missing: Vec<&str> = wanted
        .iter()
        .filter(|(name, count)| passed(name) < **count && !skipped(name))
        .map(|(name, _)| *name)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "{} checks did not run:\n  {}",
            missing.len(),
            missing.join("\n  ")
        ));
    }
    // What goes by the list must be on it, or a check that goes away later goes unnoticed; a run of the caller's choice has none.
    if !listed.is_empty() {
        let unlisted: Vec<&str> = report
            .passed
            .iter()
            .filter(|(name, count)| **count > wanted.get(name.as_str()).copied().unwrap_or(0))
            .map(|(name, _)| name.as_str())
            .collect();
        if !unlisted.is_empty() {
            return Err(format!(
                "{} checks ran more often than listed, which --record writes:\n  {}",
                unlisted.len(),
                unlisted.join("\n  ")
            ));
        }
    }
    if report.passed.is_empty() {
        return Err("no check passed".into());
    }
    Ok(())
}

/// What `--record` writes: every check the run made as often as it made it, without those that `also_listed` holds.
fn recorded(report: &Report, also_listed: &str) -> String {
    let mut left_out: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for name in also_listed.lines() {
        *left_out.entry(name).or_default() += 1;
    }
    let skipped = report
        .skipped
        .iter()
        .filter_map(|s| s.split(": ").next().map(|name| (name.to_string(), 1)));
    let mut names: Vec<String> = Vec::new();
    for (name, count) in report.passed.iter().map(|(n, c)| (n.clone(), *c)).chain(skipped) {
        let count = count.saturating_sub(left_out.get(name.as_str()).copied().unwrap_or(0));
        names.extend(std::iter::repeat_n(name, count));
    }
    names.sort();
    names.into_iter().map(|name| name + "\n").collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(lines: &str) -> Report {
        let mut r = Report::default();
        lines.lines().for_each(|l| r.note(l));
        r
    }

    const DONE: &str = "SELFTEST DONE failures=0";

    fn passes(names: &[&str]) -> String {
        names.iter().map(|n| format!("SELFTEST PASS {n}\n")).collect::<String>() + DONE
    }

    #[test]
    fn a_run_passes_only_when_it_made_all_its_checks_and_says_it_finished() {
        let list = Some("a\nb\n");
        assert!(judged(list, true, &report(&passes(&["a", "b"]))).is_ok());
        assert!(
            judged(list, true, &report("SELFTEST PASS a\nSELFTEST PASS b"))
                .unwrap_err()
                .contains("before reporting")
        );
        assert!(judged(list, true, &report("MENU a | b")).is_err());
        assert_eq!(
            judged(list, true, &report(&passes(&["a"]))).unwrap_err(),
            "1 checks did not run:\n  b"
        );
        assert_eq!(
            judged(list, false, &report(&passes(&["a", "b"]))).unwrap_err(),
            "the app ended with an error"
        );
        let failed = judged(list, false, &report("SELFTEST FAIL one: why\nSELFTEST FAIL two: how")).unwrap_err();
        assert!(failed.contains("2 checks failed") && failed.contains("two: how"));
    }

    #[test]
    fn a_new_check_asks_to_be_listed_and_one_that_goes_away_shows() {
        let list = Some("a\n");
        assert_eq!(
            judged(list, true, &report(&passes(&["a", "neu"]))).unwrap_err(),
            "1 checks ran more often than listed, which --record writes:\n  neu"
        );
        assert!(judged(list, true, &report(&passes(&["neu"]))).is_err());
        assert!(
            judged(Some(""), true, &report(&passes(&["neu"]))).is_ok(),
            "a run without a list"
        );
    }

    #[test]
    fn a_check_of_one_name_counts_as_often_as_it_is_listed() {
        let list = Some("a\na\nb\n");
        assert!(judged(list, true, &report(&passes(&["a", "a", "b"]))).is_ok());
        assert_eq!(
            judged(list, true, &report(&passes(&["a", "b"]))).unwrap_err(),
            "1 checks did not run:\n  a"
        );
        assert!(judged(list, true, &report(&passes(&["a", "a", "a", "b"]))).is_err());
    }

    #[test]
    fn a_skipped_check_neither_passes_nor_fails() {
        let list = Some("a\nb\n");
        let run = report(&format!("SELFTEST SKIP b: not active\n{}", passes(&["a"])));
        assert!(judged(list, true, &run).is_ok(), "no failure");
        assert_eq!(
            run.summary(),
            "1 checks passed, 0 failed, 1 skipped\n  skipped: b: not active"
        );
        assert_eq!(
            judged(
                Some("a\n"),
                true,
                &report(&format!("SELFTEST SKIP a: not active\n{DONE}"))
            )
            .unwrap_err(),
            "no check passed"
        );
        assert!(
            judged(
                list,
                true,
                &report(&format!("SELFTEST SKIP bb: not active\n{}", passes(&["a"])))
            )
            .is_err(),
            "another check of a similar name"
        );
    }

    #[test]
    fn the_budget_of_a_keystroke_goes_by_the_fastest() {
        let run = report("BENCH   another one: 70.5 ms\nBENCH   another one: 36.0 ms\nBENCH open: 800.0 ms\n");
        assert!(within_budget(&run, 40.0).is_ok());
        assert!(within_budget(&run, 30.0).is_err());
        assert!(within_budget(&report(""), 30.0).is_err(), "no keystroke at all");
    }

    #[test]
    fn a_budget_is_a_number() {
        let args = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(keystroke_budget(&args(&["bench", "f"])), Ok(None));
        assert_eq!(
            keystroke_budget(&args(&["bench", "f", "--keystroke-ms", "50"])),
            Ok(Some(50.0))
        );
        assert!(keystroke_budget(&args(&["bench", "f", "--keystroke-ms", "x"])).is_err());
        assert!(keystroke_budget(&args(&["bench", "f", "--keystroke-ms"])).is_err());
    }

    #[test]
    fn a_recording_lists_every_check_the_run_made() {
        let run = report(&format!("SELFTEST SKIP b: not active\n{}", passes(&["c", "a", "c"])));
        assert_eq!(recorded(&run, ""), "a\nb\nc\nc\n");
        assert_eq!(recorded(&run, "c\nb\n"), "a\nc\n", "what the scenarios list");
    }

    #[test]
    fn every_whole_mode_has_a_list_and_what_stops_early_only_has_to_end_well() {
        for (mode, part) in [
            ("probe", Part::Whole),
            ("resize", Part::Whole),
            ("typing", Part::Whole),
            ("gui", Part::Whole),
            ("gui", Part::Scenarios),
        ] {
            let list = listed_checks(mode, part).unwrap();
            assert!(list.lines().count() > 1, "{mode} {part:?}");
            assert!(list_file(mode, part).is_some(), "{mode} {part:?}");
        }
        assert_eq!(listed_checks("bench", Part::Whole), None);
        assert!(verdict("gui", Part::StopsEarly, true, &report(&passes(&["a"]))).is_ok());
        let scenarios = listed_checks("gui", Part::Scenarios).unwrap();
        let whole = listed_checks("gui", Part::Whole).unwrap();
        assert!(whole.lines().count() > scenarios.lines().count());
    }

    #[test]
    fn the_arguments_say_which_part_runs() {
        let part = |args: &[&str]| Part::of(&args.iter().map(ToString::to_string).collect::<Vec<_>>());
        assert_eq!(part(&["gui"]), Part::Whole);
        assert_eq!(part(&["gui", "--scenarios"]), Part::Scenarios);
        assert_eq!(part(&["gui", "--stop-after", "a check"]), Part::StopsEarly);
        assert_eq!(part(&["gui", "--only", "links"]), Part::StopsEarly);
        assert_eq!(
            part(&["gui", "--scenarios", "--stop-after", "a check"]),
            Part::StopsEarly
        );
    }
}
