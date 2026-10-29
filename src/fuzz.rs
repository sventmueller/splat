//! Random inputs for the randomised tests. `SPLAT_FUZZ_ROUNDS` raises the rounds for a longer search,
//! `SPLAT_FUZZ_SEED` varies the inputs.

use std::ops::Range;

/// Rounds per test.
pub fn rounds(default: usize) -> usize {
    std::env::var("SPLAT_FUZZ_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Xorshift: the same seed gives the same inputs, so a failure found once can be run again.
pub struct Rng(u64);

impl Rng {
    /// Each test passes a seed of its own; `SPLAT_FUZZ_SEED` is mixed into it.
    pub fn new(seed: u64) -> Self {
        let varied = std::env::var("SPLAT_FUZZ_SEED")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(seed, |s| seed ^ s);
        // Xorshift never leaves zero.
        Rng(if varied == 0 { seed | 1 } else { varied })
    }

    fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..n`, or 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.step() % n as u64) as usize }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// A byte offset of `text` on a char boundary.
    pub fn boundary(&mut self, text: &str) -> usize {
        let mut at = self.below(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    pub fn range(&mut self, text: &str) -> Range<usize> {
        let (a, b) = (self.boundary(text), self.boundary(text));
        a.min(b)..a.max(b)
    }
}
