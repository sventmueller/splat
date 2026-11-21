//! Which formulas and diagrams are queued, rendering, rendered or failed, and what happens when WebKit goes away. It knows
//! nothing of WebKit: `js` tells it what happened and does what it answers, so the rules can be tested without a window.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

/// Tries to load the page before formulas stay as source text for this run of the app.
const MAX_LOADS: u32 = 3;

/// Who asks for renders: one document, by a number that no other gets while the app runs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Owner(u64);

impl Owner {
    /// An owner that nobody was before.
    pub fn unique() -> Owner {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Owner(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

/// What waits for WebKit, what it renders now and what came of it, by key.
pub struct RenderQueue<K, V> {
    ready: bool,
    /// Attempts to load the page since it last finished loading.
    loads: u32,
    /// The key being rendered.
    current: Option<K>,
    /// A number identifying the current attempt, for its time limit and for answers that come too late.
    job: u64,
    /// What waits, by the owner that asked first.
    waiting: HashMap<Owner, VecDeque<K>>,
    /// Whose key comes next, one of each in turn: a document full of slow diagrams must not hold up the formulas of others.
    turns: VecDeque<Owner>,
    /// What each owner last asked for.
    wanted: HashMap<Owner, HashSet<K>>,
    pending: HashSet<K>,
    failed: HashSet<K>,
    /// Keys that already got their second chance after WebKit went away while rendering them.
    retried: HashSet<K>,
    done: HashMap<K, V>,
    errors: HashMap<K, String>,
    /// Last time each key was requested, so the least recently used ones are dropped first.
    used: HashMap<K, u64>,
    tick: u64,
    /// Keys whose result is being converted on another thread.
    converting: usize,
    /// How many bytes of memory each result holds.
    sizes: HashMap<K, usize>,
    bytes: usize,
}

impl<K: Clone + Eq + Hash, V: Clone> Default for RenderQueue<K, V> {
    fn default() -> Self {
        RenderQueue {
            ready: false,
            loads: 0,
            current: None,
            job: 0,
            waiting: HashMap::new(),
            turns: VecDeque::new(),
            wanted: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            retried: HashSet::new(),
            done: HashMap::new(),
            errors: HashMap::new(),
            used: HashMap::new(),
            tick: 0,
            converting: 0,
            sizes: HashMap::new(),
            bytes: 0,
        }
    }
}

impl<K: Clone + Eq + Hash, V: Clone> RenderQueue<K, V> {
    /// The page finished loading.
    pub fn page_ready(&mut self) {
        self.ready = true;
        self.loads = 0;
    }

    /// A load of the page starts; the number of the attempt that may give up on it.
    pub fn load_started(&mut self) -> u64 {
        self.loads += 1;
        self.job
    }

    /// The time limit of the load in attempt `job` ran out: whether to try once more.
    pub fn load_timed_out(&self, job: u64) -> bool {
        job == self.job && !self.ready && self.loads < MAX_LOADS
    }

    /// All that `owner` needs now: what is neither rendered, failed nor queued gets queued, and what nobody needs any longer
    /// leaves the queue.
    pub fn request(&mut self, owner: Owner, keys: Vec<K>) {
        self.tick += 1;
        let mut wanted = HashSet::with_capacity(keys.len());
        for key in keys {
            self.used.insert(key.clone(), self.tick);
            if !self.done.contains_key(&key) && !self.failed.contains(&key) && self.pending.insert(key.clone()) {
                self.enqueue(owner, key.clone());
            }
            wanted.insert(key);
        }
        self.wanted.insert(owner, wanted);
        self.drop_what_nobody_wants();
    }

    fn enqueue(&mut self, owner: Owner, key: K) {
        if !self.turns.contains(&owner) {
            self.turns.push_back(owner);
        }
        self.waiting.entry(owner).or_default().push_back(key);
    }

    fn drop_what_nobody_wants(&mut self) {
        let Self {
            waiting,
            wanted,
            pending,
            ..
        } = self;
        for queue in waiting.values_mut() {
            queue.retain(|key| {
                let keep = wanted.values().any(|keys| keys.contains(key));
                if !keep {
                    pending.remove(key);
                }
                keep
            });
        }
    }

    /// `owner` is gone: what only it asked for leaves the queue, what another asked for too goes to that one, and all it
    /// holds counts as no longer wanted.
    pub fn forget(&mut self, owner: Owner) {
        self.wanted.remove(&owner);
        self.turns.retain(|o| *o != owner);
        for key in self.waiting.remove(&owner).unwrap_or_default() {
            match self.owner_of(&key) {
                Some(other) => self.enqueue(other, key),
                None => {
                    self.pending.remove(&key);
                }
            }
        }
    }

    fn owner_of(&self, key: &K) -> Option<Owner> {
        self.wanted.iter().find(|(_, keys)| keys.contains(key)).map(|(o, _)| *o)
    }

    /// The next key to render and the number of its attempt, if the page is free.
    #[expect(
        clippy::should_implement_trait,
        reason = "it hands the page its next key, which ends no iteration"
    )]
    pub fn next(&mut self) -> Option<(K, u64)> {
        if !self.ready || self.current.is_some() {
            return None;
        }
        let key = self.next_in_turn()?;
        self.job += 1;
        self.current = Some(key.clone());
        Some((key, self.job))
    }

    fn next_in_turn(&mut self) -> Option<K> {
        for _ in 0..self.turns.len() {
            let owner = self.turns.pop_front()?;
            self.turns.push_back(owner);
            if let Some(key) = self.waiting.get_mut(&owner).and_then(VecDeque::pop_front) {
                return Some(key);
            }
        }
        None
    }

    /// Whether an answer of attempt `job` still counts. One from a page that was given up on or lost may come long after
    /// its key was queued anew.
    pub fn answer_counts(&self, job: u64) -> bool {
        job == self.job && self.current.is_some()
    }

    /// The page is done with `key` and free for the next; its result follows, possibly later.
    pub fn release(&mut self, key: &K) -> bool {
        if self.current.as_ref() != Some(key) {
            return false;
        }
        self.current = None;
        true
    }

    /// The result of a key is being converted on another thread.
    pub fn conversion_started(&mut self) {
        self.converting += 1;
    }

    /// A conversion that started ended.
    pub fn conversion_ended(&mut self) {
        self.converting = self.converting.saturating_sub(1);
    }

    /// The result for `key`; `None` means it failed. Whether it can now be shown.
    pub fn store(&mut self, key: K, result: Option<V>) -> bool {
        self.pending.remove(&key);
        self.retried.remove(&key);
        match result {
            Some(value) => {
                self.done.insert(key, value);
                true
            }
            None => {
                self.failed.insert(key);
                false
            }
        }
    }

    /// How much memory the result for `key` holds; a picture of a large diagram can take as much as a thousand formulas.
    pub fn note_size(&mut self, key: K, bytes: usize) {
        self.bytes += bytes;
        if let Some(before) = self.sizes.insert(key, bytes) {
            self.bytes -= before;
        }
    }

    /// The memory all results hold.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The renderer said what went wrong with `key`.
    pub fn note_error(&mut self, key: K, message: String) {
        self.errors.insert(key, message);
    }

    /// Whether attempt `job` is still running when its time limit ran out.
    pub fn timed_out(&self, job: u64) -> bool {
        job == self.job && self.current.is_some()
    }

    /// WebKit went away or stopped answering: the page has to load again. The running key gets a second chance, since
    /// WebKit can go away for reasons of its own, if anybody still wants it; only a second failure counts.
    pub fn lost_page(&mut self) {
        if let Some(key) = self.current.take() {
            match self.owner_of(&key) {
                Some(owner) if self.retried.insert(key.clone()) => self.enqueue(owner, key),
                Some(_) => {
                    self.store(key, None);
                }
                None => {
                    self.pending.remove(&key);
                    self.retried.remove(&key);
                }
            }
        }
        self.job += 1;
        self.ready = false;
    }

    /// Nothing is queued, rendering or converting.
    pub fn idle(&self) -> bool {
        self.waiting.values().all(VecDeque::is_empty) && self.current.is_none() && self.converting == 0
    }

    /// Something waits for the page, which has to start if it is not there.
    pub fn waits(&self) -> bool {
        self.waiting.values().any(|queue| !queue.is_empty())
    }

    /// The result for `key`, once it is rendered.
    pub fn rendered(&self, key: &K) -> Option<&V> {
        self.done.get(key)
    }

    /// What a finished render reports: `None` while the key is still queued, else whether it worked, with the error of
    /// the renderer if it said one.
    pub fn outcome(&self, key: &K) -> Option<Result<(), Option<String>>> {
        if let Some(message) = self.errors.get(key) {
            return Some(Err(Some(message.clone())));
        }
        if self.done.contains_key(key) {
            return Some(Ok(()));
        }
        self.failed.contains(key).then_some(Err(None))
    }

    /// Drops the results no open document wants, the least recently asked for first, until at most `limit` of them
    /// holding at most `budget` bytes are left, and returns them. What a document wants stays, however much it is:
    /// dropping it would only make the document request it again.
    pub fn evict(&mut self, limit: usize, budget: usize) -> Vec<K> {
        if self.done.len() + self.failed.len() <= limit && self.bytes <= budget {
            return Vec::new();
        }
        let gone: Vec<K> = {
            let wanted: HashSet<&K> = self.wanted.values().flatten().collect();
            let mut unwanted: Vec<(u64, &K)> = self
                .done
                .keys()
                .chain(self.failed.iter())
                .filter(|key| !wanted.contains(key))
                .map(|key| (self.used.get(key).copied().unwrap_or(0), key))
                .collect();
            unwanted.sort_unstable_by_key(|(used, _)| *used);
            let size = |key: &K| self.sizes.get(key).copied().unwrap_or(0);
            let mut count = unwanted.len();
            let mut bytes: usize = unwanted.iter().map(|(_, key)| size(key)).sum();
            let mut gone = Vec::new();
            for (_, key) in unwanted {
                if count <= limit && bytes <= budget {
                    break;
                }
                count -= 1;
                bytes -= size(key);
                gone.push(key.clone());
            }
            gone
        };
        for key in &gone {
            self.done.remove(key);
            self.failed.remove(key);
            self.errors.remove(key);
            self.used.remove(key);
            self.bytes -= self.sizes.remove(key).unwrap_or(0);
        }
        gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: Owner = Owner(1);
    const OTHER: Owner = Owner(2);

    fn ready() -> RenderQueue<&'static str, u32> {
        let mut q = RenderQueue::default();
        q.page_ready();
        q
    }

    #[test]
    fn keys_render_one_at_a_time_in_order_and_once() {
        let mut q = ready();
        q.request(ONE, vec!["a", "b", "a"]);
        let (key, _) = q.next().unwrap();
        assert_eq!(key, "a");
        assert!(q.next().is_none(), "the page renders one at a time");
        assert!(q.release(&"a"));
        q.store("a", Some(1));
        assert_eq!(q.next().unwrap().0, "b");
        q.request(ONE, vec!["a", "b"]);
        assert!(q.release(&"b"));
        q.store("b", Some(2));
        assert!(
            q.next().is_none(),
            "what is rendered or under way is not asked for again"
        );
        assert!(q.idle());
        assert_eq!(q.rendered(&"a"), Some(&1));
    }

    #[test]
    fn nothing_renders_before_the_page_is_ready() {
        let mut q: RenderQueue<&str, u32> = RenderQueue::default();
        q.request(ONE, vec!["a"]);
        assert!(q.next().is_none());
        assert!(!q.idle());
        q.page_ready();
        assert!(q.next().is_some());
    }

    #[test]
    fn a_lost_page_gives_the_running_key_one_more_chance() {
        let mut q = ready();
        q.request(ONE, vec!["a"]);
        let (_, first) = q.next().unwrap();
        assert!(q.timed_out(first));
        q.lost_page();
        assert!(q.next().is_none(), "the page has to load first");
        q.page_ready();
        let (key, second) = q.next().unwrap();
        assert_eq!(key, "a");
        assert_ne!(first, second);
        q.lost_page();
        assert_eq!(q.outcome(&"a"), Some(Err(None)), "the second failure counts");
        q.page_ready();
        assert!(q.next().is_none() && q.idle());
    }

    /// Without this, a diagram that hangs WebKit would render again and again: the killed page's answer used to end its
    /// second chance while it was queued anew.
    #[test]
    fn an_answer_from_a_lost_page_does_not_count() {
        let mut q = ready();
        q.request(ONE, vec!["a"]);
        let (_, job) = q.next().unwrap();
        q.lost_page();
        assert!(!q.answer_counts(job));
        q.page_ready();
        let (_, again) = q.next().unwrap();
        assert!(!q.answer_counts(job), "not even while the same key renders again");
        assert!(q.answer_counts(again));
    }

    #[test]
    fn a_time_limit_only_ends_its_own_attempt() {
        let mut q = ready();
        q.request(ONE, vec!["a", "b"]);
        let (_, first) = q.next().unwrap();
        q.release(&"a");
        q.store("a", Some(1));
        let _second = q.next().unwrap();
        assert!(!q.timed_out(first));
    }

    #[test]
    fn a_page_that_never_loads_is_given_up_on() {
        let mut q: RenderQueue<&str, u32> = RenderQueue::default();
        let job = q.load_started();
        assert!(q.load_timed_out(job), "second attempt");
        q.load_started();
        assert!(q.load_timed_out(job), "third attempt");
        q.load_started();
        assert!(!q.load_timed_out(job));
        q.page_ready();
        assert!(!q.load_timed_out(job));
    }

    #[test]
    fn a_conversion_keeps_the_queue_busy_but_frees_the_page() {
        let mut q = ready();
        q.request(ONE, vec!["a", "b"]);
        q.next().unwrap();
        q.conversion_started();
        q.release(&"a");
        assert_eq!(q.next().unwrap().0, "b");
        q.release(&"b");
        q.store("b", Some(2));
        assert!(!q.idle());
        q.conversion_ended();
        q.store("a", Some(1));
        assert!(q.idle());
    }

    #[test]
    fn errors_are_reported_and_failures_not_asked_for_again() {
        let mut q = ready();
        q.request(ONE, vec!["a"]);
        q.next().unwrap();
        q.note_error("a", "Syntaxfehler".into());
        q.release(&"a");
        q.store("a", None);
        assert_eq!(q.outcome(&"a"), Some(Err(Some("Syntaxfehler".into()))));
        q.request(ONE, vec!["a"]);
        assert!(q.next().is_none());
        assert_eq!(q.outcome(&"b"), None);
    }

    /// Renders `keys` one after the other for `ONE`, each asked for alone as a formula being typed is, with its size.
    fn typed(q: &mut RenderQueue<&'static str, u32>, keys: &[(&'static str, usize)]) {
        for &(key, size) in keys {
            q.request(ONE, vec![key]);
            let (key, _) = q.next().unwrap();
            q.release(&key);
            q.store(key, Some(0));
            q.note_size(key, size);
        }
    }

    #[test]
    fn what_no_document_wants_goes_least_recently_asked_for_first() {
        let mut q = ready();
        typed(&mut q, &[("a", 0), ("b", 0), ("c", 0)]);
        assert!(q.evict(2, 100).is_empty(), "below the limit nothing goes");
        assert_eq!(q.evict(1, 100), ["a"]);
        assert_eq!(q.outcome(&"a"), None);
        assert_eq!(q.rendered(&"b"), Some(&0));
        assert_eq!(q.evict(0, 100), ["b"]);
        assert_eq!(q.rendered(&"c"), Some(&0), "what a document wants stays");
    }

    #[test]
    fn few_large_results_are_dropped_like_many_small_ones() {
        let mut q = ready();
        typed(&mut q, &[("alt", 90), ("mittel", 20), ("neu", 30)]);
        assert!(
            q.evict(10, 110).is_empty(),
            "only what no document wants counts against the budget"
        );
        assert_eq!(
            q.evict(10, 100),
            ["alt"],
            "the oldest goes first, and only until the rest fits"
        );
        assert_eq!(q.evict(10, 0), ["mittel"]);
        assert!(q.evict(10, 0).is_empty(), "what is wanted stays, however large");
        assert_eq!(q.bytes(), 30, "the bytes went with the others");
    }

    #[test]
    fn a_failure_counts_against_the_limit_and_goes_with_its_error() {
        let mut q = ready();
        q.request(ONE, vec!["kaputt"]);
        q.next().unwrap();
        q.note_error("kaputt", "Syntaxfehler".into());
        q.release(&"kaputt");
        q.store("kaputt", None);
        typed(&mut q, &[("neu", 0)]);
        assert_eq!(q.evict(0, 100), ["kaputt"]);
        assert_eq!(q.outcome(&"kaputt"), None);
    }

    #[test]
    fn only_what_waits_asks_for_the_page() {
        let mut q: RenderQueue<&str, u32> = RenderQueue::default();
        q.request(ONE, Vec::new());
        assert!(!q.waits());
        q.request(ONE, vec!["a"]);
        assert!(q.waits(), "also before the page is there");
        q.page_ready();
        let (key, _) = q.next().unwrap();
        assert!(!q.waits(), "what renders waits no longer");
        q.release(&key);
        q.store(key, Some(1));
        q.request(ONE, vec!["a"]);
        assert!(!q.waits(), "what is rendered needs no page");
    }

    #[test]
    fn what_was_rendered_stays_when_the_page_ends() {
        let mut q = ready();
        q.request(ONE, vec!["a"]);
        let (key, job) = q.next().unwrap();
        q.release(&key);
        q.store(key, Some(1));
        q.lost_page();
        assert!(!q.answer_counts(job) && q.idle());
        assert_eq!(q.rendered(&"a"), Some(&1));
        q.request(ONE, vec!["a", "b"]);
        assert!(q.waits() && q.next().is_none(), "a new page has to load first");
        q.page_ready();
        assert_eq!(q.next().unwrap().0, "b");
    }

    #[test]
    fn owners_take_turns() {
        let mut q = ready();
        q.request(ONE, vec!["a", "b", "c"]);
        q.request(OTHER, vec!["x", "y"]);
        let mut order = Vec::new();
        while let Some((key, _)) = q.next() {
            q.release(&key);
            q.store(key, Some(0));
            order.push(key);
        }
        assert_eq!(order, ["a", "x", "b", "y", "c"]);
        assert!(q.idle());
    }

    #[test]
    fn what_nobody_needs_any_longer_leaves_the_queue() {
        let mut q = ready();
        q.request(ONE, vec!["a", "b", "c"]);
        q.request(ONE, vec!["c", "d"]);
        let (first, _) = q.next().unwrap();
        assert_eq!(first, "c");
        q.release(&first);
        q.store(first, Some(0));
        assert_eq!(q.next().unwrap().0, "d");
        q.request(ONE, vec!["a"]);
        assert!(q.pending.contains("a"), "asked for again, it queues again");
    }

    #[test]
    fn a_key_two_owners_want_stays_until_both_are_gone() {
        let mut q = ready();
        q.request(ONE, vec!["a", "b"]);
        q.request(OTHER, vec!["b"]);
        q.forget(ONE);
        assert_eq!(q.next().unwrap().0, "b", "the other still wants it");
        q.release(&"b");
        q.store("b", Some(0));
        assert!(q.idle(), "and what only the one wanted has gone");
        assert!(q.next().is_none());
    }

    #[test]
    fn an_owner_that_is_gone_leaves_nothing_waiting() {
        let mut q = ready();
        q.request(ONE, vec!["a", "b", "c"]);
        let (running, _) = q.next().unwrap();
        q.forget(ONE);
        assert!(!q.idle(), "what runs cannot be taken back");
        q.release(&running);
        q.store(running, Some(0));
        assert!(q.idle());
        assert!(q.next().is_none());
        assert!(q.pending.is_empty());
    }

    #[test]
    fn a_lost_page_does_not_try_again_for_nobody() {
        let mut q = ready();
        q.request(ONE, vec!["a"]);
        q.next().unwrap();
        q.forget(ONE);
        q.lost_page();
        q.page_ready();
        assert!(q.next().is_none());
        assert!(q.pending.is_empty() && q.outcome(&"a").is_none());
    }

    #[test]
    fn what_a_gone_owner_held_goes_at_the_next_eviction_and_what_others_need_stays() {
        let mut q = ready();
        for (owner, key) in [(ONE, "a"), (OTHER, "b")] {
            q.request(owner, vec![key]);
            let (key, _) = q.next().unwrap();
            q.release(&key);
            q.store(key, Some(0));
        }
        q.forget(ONE);
        assert_eq!(q.evict(0, 0), ["a"]);
        assert_eq!(q.rendered(&"b"), Some(&0));
    }

    #[test]
    fn the_bytes_of_the_results_are_known() {
        let mut q = ready();
        q.note_size("a", 10);
        q.note_size("b", 5);
        q.note_size("a", 7);
        assert_eq!(q.bytes(), 12);
    }
}
