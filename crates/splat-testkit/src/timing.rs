//! Inputs built to hurt, at two sizes whose time and memory must grow as the input does: run on a small stack within a
//! time limit, measured one measurement at a time, and with every suspect run again alone.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant};

use crate::memory;

/// As small as the app's smallest stack.
pub const STACK: usize = 2 << 20;

/// An unoptimized or measured build takes ten times as long for linear work too. On Intel the system allocator can spend
/// a minute on such inputs moving its regions back and forth, so there the limit only catches endless loops, while Apple
/// Silicon holds the same code to time.
pub const LIMIT: Duration = Duration::from_secs(if cfg!(debug_assertions) || cfg!(coverage) {
    100
} else if cfg!(target_arch = "x86_64") {
    300
} else {
    10
});

/// Below this nothing hurts, and short times say little.
pub const NOTICEABLE: Duration = Duration::from_millis(400);

/// Whether this build is held to the time its work takes, as `LIMIT` explains; a measured build counts every line it
/// runs, which says nothing about how the real one grows.
pub fn holds_to_time() -> bool {
    cfg!(target_arch = "aarch64") && !cfg!(debug_assertions) && !cfg!(coverage)
}

/// Whether time that grows from `small` to `large` with an input growing from `small_len` to `large_len` is more than
/// linear work with some noise.
pub fn grows_too_fast(
    noticeable: Duration,
    small: Duration,
    large: Duration,
    small_len: usize,
    large_len: usize,
) -> bool {
    const SHORTEST: Duration = Duration::from_millis(20);
    const NOISE: f64 = 2.25;
    #[expect(clippy::cast_precision_loss, reason = "a ratio of input lengths far below 2^52")]
    let allowed = large_len as f64 / small_len.max(1) as f64 * NOISE;
    large > noticeable && large.as_secs_f64() / small.max(SHORTEST).as_secs_f64() > allowed
}

static MEASURING: Mutex<()> = Mutex::new(());

/// While it lives, no other measurement of this process runs: side by side they would only measure each other.
pub struct Alone(#[expect(dead_code, reason = "dropping it lets the next measurement run")] MutexGuard<'static, ()>);

/// Waits until no other measurement runs.
pub fn alone() -> Alone {
    Alone(MEASURING.lock().unwrap_or_else(PoisonError::into_inner))
}

impl Alone {
    /// The shortest of `runs` runs of `work`: a single run may have met a busy moment of the Mac.
    pub fn fastest(&self, runs: usize, mut work: impl FnMut()) -> Duration {
        (0..runs.max(1))
            .map(|_| {
                let started = Instant::now();
                work();
                started.elapsed()
            })
            .min()
            .unwrap_or_default()
    }
}

/// One part of the way through the code, with the time from which on it hurts.
struct Stage {
    name: &'static str,
    took: Duration,
    noticeable: Duration,
}

/// The time each part of the way through the code takes: linear work elsewhere must not hide a part that is not.
#[derive(Default)]
pub struct Stages(Vec<Stage>);

impl Stages {
    pub fn time<T>(&mut self, stage: &'static str, work: impl FnOnce() -> T) -> T {
        self.time_noticeable_from(stage, NOTICEABLE, work)
    }

    /// Like `time`, for a stage whose time is allowed to reach `noticeable` before it counts.
    pub fn time_noticeable_from<T>(
        &mut self,
        stage: &'static str,
        noticeable: Duration,
        work: impl FnOnce() -> T,
    ) -> T {
        let started = Instant::now();
        let result = work();
        self.0.push(Stage {
            name: stage,
            took: started.elapsed(),
            noticeable,
        });
        result
    }

    /// The shorter of each stage, of two runs of the same input.
    fn shortest(self, other: &Stages) -> Stages {
        let pairs = self.0.into_iter().zip(&other.0);
        Stages(
            pairs
                .map(|(this, that)| Stage {
                    took: this.took.min(that.took),
                    ..this
                })
                .collect(),
        )
    }

    fn noticeable(&self) -> bool {
        self.0.iter().any(|stage| stage.took > stage.noticeable)
    }

    /// The stage that grows too fast, as a message.
    fn too_fast(small: &Stages, large: &Stages, small_len: usize, large_len: usize) -> Option<String> {
        small.0.iter().zip(&large.0).find_map(|(small, large)| {
            grows_too_fast(large.noticeable, small.took, large.took, small_len, large_len).then(|| {
                format!(
                    "{}: {:?} for {small_len} bytes, but {:?} for {large_len} bytes",
                    small.name, small.took, large.took
                )
            })
        })
    }
}

/// What a case's size is counted in.
pub trait Input: Send + Sync + 'static {
    fn size(&self) -> usize;
}

impl Input for str {
    fn size(&self) -> usize {
        self.len()
    }
}

impl Input for [u8] {
    fn size(&self) -> usize {
        self.len()
    }
}

/// An input at a fourth of its size and at full size, and the work that has to take time and memory as the input grows.
pub struct Scaled<I: Input + ?Sized> {
    pub name: &'static str,
    pub small: Arc<I>,
    pub large: Arc<I>,
    pub work: fn(&I) -> Stages,
}

/// What a run of `work` took: its stages and the most memory its thread held at once.
struct Run {
    stages: Stages,
    peak: usize,
}

/// Runs `work` on a thread of its own and waits at most `LIMIT`, so an endless loop fails as well. The thread is named
/// after the case, so a stack overflow, which ends the test run, names it.
fn survives<I: Input + ?Sized>(name: &'static str, input: &Arc<I>, work: fn(&I) -> Stages) -> Result<Run, String> {
    let (done, finished) = mpsc::channel();
    let input = input.clone();
    std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(STACK)
        .spawn(move || {
            let (stages, peak) = memory::peak_of(|| work(&input));
            let _ = done.send(Run { stages, peak });
        })
        .map_err(|e| format!("{name}: {e}"))?;
    match finished.recv_timeout(LIMIT) {
        Ok(run) => Ok(run),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{name}: over {LIMIT:?}")),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!("{name}: panicked")),
    }
}

/// What the first runs of a case say: that it is fine, that its time is suspect, or what is wrong with it.
enum Verdict {
    Fine,
    Suspect(Stages, Stages),
    Wrong(String),
}

fn first_look<I: Input + ?Sized>(case: &Scaled<I>) -> Result<Verdict, String> {
    let (small_len, large_len) = (case.small.size(), case.large.size());
    let small = survives(case.name, &case.small, case.work)?;
    let large = survives(case.name, &case.large, case.work)?;
    if memory::grows_too_much(small.peak, large.peak, small_len, large_len) {
        return Ok(Verdict::Wrong(format!(
            "{}: memory grows faster than the input: {} bytes at most for {small_len} bytes, but {} bytes for {large_len} \
             bytes",
            case.name, small.peak, large.peak
        )));
    }
    if !holds_to_time() {
        return Ok(Verdict::Fine);
    }
    // A small run that got held up would make any growth look fine, so wherever it matters it is run again.
    let small = if large.stages.noticeable() {
        small
            .stages
            .shortest(&survives(case.name, &case.small, case.work)?.stages)
    } else {
        small.stages
    };
    Ok(match Stages::too_fast(&small, &large.stages, small_len, large_len) {
        Some(_) => Verdict::Suspect(small, large.stages),
        None => Verdict::Fine,
    })
}

/// A suspect, run again without anything else running in the process.
fn again_alone<I: Input + ?Sized>(case: &Scaled<I>, mut small: Stages, mut large: Stages) -> Result<(), String> {
    let (small_len, large_len) = (case.small.size(), case.large.size());
    for _ in 0..3 {
        small = small.shortest(&survives(case.name, &case.small, case.work)?.stages);
        large = large.shortest(&survives(case.name, &case.large, case.work)?.stages);
        if Stages::too_fast(&small, &large, small_len, large_len).is_none() {
            return Ok(());
        }
    }
    match Stages::too_fast(&small, &large, small_len, large_len) {
        Some(stage) => Err(format!("{}: {stage}", case.name)),
        None => Ok(()),
    }
}

/// What goes wrong with `cases`: each within `LIMIT` on a small stack, with memory that grows as the input does, and on
/// Apple Silicon with time that does so as well. They run four at a time, sharing the caches and the memory bus, so a large
/// input that meets a busy moment looks like more than linear work; a suspect is run again alone, where nothing else is to
/// blame.
pub fn scales<I: Input + ?Sized>(cases: Vec<Scaled<I>>) -> Vec<String> {
    let _alone = alone();
    let queue = Mutex::new(cases);
    let problems = Mutex::new(Vec::new());
    let suspects = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let Some(case) = queue.lock().unwrap_or_else(PoisonError::into_inner).pop() else {
                        return;
                    };
                    match first_look(&case) {
                        Ok(Verdict::Fine) => {}
                        Ok(Verdict::Suspect(small, large)) => {
                            suspects
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push((case, small, large));
                        }
                        Ok(Verdict::Wrong(problem)) | Err(problem) => {
                            problems.lock().unwrap_or_else(PoisonError::into_inner).push(problem);
                        }
                    }
                }
            });
        }
    });
    let mut problems = problems.into_inner().unwrap_or_else(PoisonError::into_inner);
    for (case, small, large) in suspects.into_inner().unwrap_or_else(PoisonError::into_inner) {
        if let Err(problem) = again_alone(&case, small, large) {
            problems.push(problem);
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_that_grows_faster_than_the_input_is_found() {
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

    #[test]
    fn the_stage_that_grows_too_fast_is_named() {
        let stages = |took: &[u64]| {
            Stages(
                took.iter()
                    .zip(["parse", "layout"])
                    .map(|(ms, name)| Stage {
                        name,
                        took: Duration::from_millis(*ms),
                        noticeable: NOTICEABLE,
                    })
                    .collect(),
            )
        };
        let found = Stages::too_fast(&stages(&[100, 100]), &stages(&[400, 1600]), 1000, 4000);
        assert!(found.is_some_and(|stage| stage.starts_with("layout: ")));
        assert!(Stages::too_fast(&stages(&[100, 100]), &stages(&[400, 400]), 1000, 4000).is_none());
    }
}
