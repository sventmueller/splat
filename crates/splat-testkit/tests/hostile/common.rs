//! Running a document on a small stack within a time limit, and telling time that grows faster than the text.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use splat_markdown::commands::Edit;
use splat_markdown::text::Byte;

pub(super) const STACK: usize = 2 << 20;

/// An unoptimized build takes ten times as long for linear work too. On Intel the system allocator can spend a minute on
/// such documents moving its regions back and forth, so there the limit only catches endless loops, while Apple Silicon
/// holds the same code to time.
pub(super) const LIMIT: Duration = Duration::from_secs(if cfg!(debug_assertions) {
    100
} else if cfg!(target_arch = "x86_64") {
    300
} else {
    10
});

/// An edit a command returned has to apply to the text it was made for.
pub(super) fn applies(text: &str, edit: Edit) {
    let mut out = text.to_string();
    assert!(
        edit.range.end <= Byte::new(text.len())
            && text.is_char_boundary(edit.range.start.get())
            && text.is_char_boundary(edit.range.end.get()),
        "an edit outside the text: {:?} of {} bytes",
        edit.range,
        text.len()
    );
    edit.apply_to(&mut out);
    assert!(
        edit.selection.end <= Byte::new(out.len())
            && out.is_char_boundary(edit.selection.start.get())
            && out.is_char_boundary(edit.selection.end.get()),
        "a selection outside the result: {:?} of {} bytes",
        edit.selection,
        out.len()
    );
}

/// The time each part of the way through the app takes: linear work elsewhere must not hide a part that is not.
#[derive(Default)]
pub(super) struct Stages(Vec<(&'static str, Duration)>);

impl Stages {
    pub(super) fn time<T>(&mut self, stage: &'static str, work: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = work();
        self.0.push((stage, started.elapsed()));
        result
    }

    /// The shorter of each stage, of two runs of the same document.
    fn shortest(self, other: &Stages) -> Stages {
        let pairs = self.0.into_iter().zip(&other.0);
        Stages(pairs.map(|((stage, a), (_, b))| (stage, a.min(*b))).collect())
    }
}

pub(super) fn numbered(count: usize, line: impl Fn(usize) -> String) -> String {
    (0..count).map(line).collect()
}

/// Runs a document on a thread of its own and waits at most `LIMIT`, so an endless loop fails as well.
fn survives(name: &'static str, text: &std::sync::Arc<str>, work: fn(&str) -> Stages) -> Result<Stages, String> {
    let (done, finished) = mpsc::channel();
    let text = text.clone();
    std::thread::Builder::new()
        .name(format!("hostile document: {name}"))
        .stack_size(STACK)
        .spawn(move || {
            let _ = done.send(work(&text));
        })
        .map_err(|e| format!("{name}: {e}"))?;
    match finished.recv_timeout(LIMIT) {
        Ok(took) => Ok(took),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{name}: over {LIMIT:?}")),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!("{name}: panicked")),
    }
}

/// Below this no document hurts, and short times say little.
pub(super) const NOTICEABLE: Duration = Duration::from_millis(400);

/// An edit in the preview reads the text it would make for as far as `MAX_CHECKED` goes, and from there on not at all, so
/// its time grows faster up to that size and then stops; only other programs on the Mac can take it above this.
const NOTICEABLE_IN_THE_PREVIEW: Duration = Duration::from_millis(2000);

/// Whether time that grows from `small` to `large` with a text growing from `small_len` to `large_len` is more than linear
/// work with some noise.
pub(super) fn grows_too_fast(
    noticeable: Duration,
    small: Duration,
    large: Duration,
    small_len: usize,
    large_len: usize,
) -> bool {
    const SHORTEST: Duration = Duration::from_millis(20);
    const NOISE: f64 = 2.25;
    #[expect(clippy::cast_precision_loss, reason = "a ratio of text lengths far below 2^52")]
    let allowed = large_len as f64 / small_len.max(1) as f64 * NOISE;
    large > noticeable && large.as_secs_f64() / small.max(SHORTEST).as_secs_f64() > allowed
}

#[test]
fn time_that_grows_faster_than_the_text_is_found() {
    let ms = Duration::from_millis;
    assert!(!grows_too_fast(ms(400), ms(100), ms(400), 1000, 4000), "linear");
    assert!(
        !grows_too_fast(ms(400), ms(100), ms(800), 1000, 4000),
        "linear, with a slow run"
    );
    assert!(grows_too_fast(ms(400), ms(100), ms(1600), 1000, 4000), "quadratic");
    assert!(
        !grows_too_fast(ms(2000), ms(100), ms(1600), 1000, 4000),
        "not noticeable"
    );
    assert!(
        grows_too_fast(ms(400), ms(1), ms(500), 1000, 4000),
        "a small time counts for at least a little"
    );
}

/// The stage that grows too fast, as a message.
fn stage_too_fast(small: &Stages, large: &Stages, small_len: usize, large_len: usize) -> Option<String> {
    small.0.iter().zip(&large.0).find_map(|((stage, small), (_, large))| {
        let noticeable = if *stage == "editing in the preview" {
            NOTICEABLE_IN_THE_PREVIEW
        } else {
            NOTICEABLE
        };
        grows_too_fast(noticeable, *small, *large, small_len, large_len)
            .then(|| format!("{stage}: {small:?} for {small_len} bytes, but {large:?} for {large_len} bytes"))
    })
}

/// A document at a fourth of its size and at full size, within the limit and with times that grow as the text does, in
/// every stage. Only Apple Silicon is held to that, as `LIMIT` says, and not a measured build, which counts every line.
pub(super) fn scales(name: &'static str, small: String, large: String, work: fn(&str) -> Stages) -> Result<(), String> {
    let (small_len, large_len) = (small.len(), large.len());
    let (small, large): (std::sync::Arc<str>, std::sync::Arc<str>) = (small.into(), large.into());
    let mut small_took = survives(name, &small, work)?;
    let mut large_took = survives(name, &large, work)?;
    if !cfg!(target_arch = "aarch64") || cfg!(debug_assertions) || cfg!(coverage) {
        return Ok(());
    }
    // A small run that got held up would make any growth look fine, so wherever it matters it is run again.
    if large_took.0.iter().any(|(_, took)| *took > NOTICEABLE) {
        small_took = small_took.shortest(&survives(name, &small, work)?);
    }
    // Other documents run alongside, so a single slow run proves nothing.
    for _ in 0..2 {
        if stage_too_fast(&small_took, &large_took, small_len, large_len).is_none() {
            return Ok(());
        }
        small_took = small_took.shortest(&survives(name, &small, work)?);
        large_took = large_took.shortest(&survives(name, &large, work)?);
    }
    match stage_too_fast(&small_took, &large_took, small_len, large_len) {
        Some(stage) => Err(format!("{name}: {stage}")),
        None => Ok(()),
    }
}

/// Taken by what measures many documents: side by side they would only measure each other.
pub(super) static MEASURING: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn shortest_of_three(work: impl Fn()) -> Duration {
    let run = || {
        let started = Instant::now();
        work();
        started.elapsed()
    };
    (0..3).map(|_| run()).min().unwrap()
}
