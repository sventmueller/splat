//! What the controller knows about the document and its copy of the text, lent out only for the length of a closure.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use crate::markdown::text::U16;
use crate::state::State;
use crate::sync::{Snapshot, SourceCopy};

/// The state and the copy of the source text, lent to closures only. They must be `Send` because the controller and its
/// views are not, so none of them can call a view while the state is borrowed or keep the text across a change of the
/// source, which would copy all of it.
pub struct Model {
    state: RefCell<State>,
    copy: SourceCopy,
}

impl Default for Model {
    fn default() -> Model {
        Model::new()
    }
}

impl Model {
    /// The model of an empty document.
    pub fn new() -> Model {
        Model {
            state: RefCell::new(State::new()),
            copy: SourceCopy::default(),
        }
    }

    /// What `f` makes of the state.
    pub fn read<R>(&self, f: impl for<'a> FnOnce(&'a State) -> R + Send) -> R {
        f(&self.state.borrow())
    }

    /// Lets `f` change the state.
    pub fn change<R>(&self, f: impl for<'a> FnOnce(&'a mut State) -> R + Send) -> R {
        f(&mut self.state.borrow_mut())
    }

    /// Lends the current text, which `read_view` gets from the view when edits could not patch the copy, and the state.
    pub fn with_text<R>(
        &self,
        read_view: impl FnOnce() -> (String, usize),
        f: impl for<'a> FnOnce(&'a Snapshot, &'a State) -> R + Send,
    ) -> R {
        let snap = self.copy.current(read_view);
        f(&snap, &self.state.borrow())
    }

    /// Like `with_text`, for a change of the state that depends on the text.
    pub fn change_with_text<R>(
        &self,
        read_view: impl FnOnce() -> (String, usize),
        f: impl for<'a> FnOnce(&'a Snapshot, &'a mut State) -> R + Send,
    ) -> R {
        let snap = self.copy.current(read_view);
        f(&snap, &mut self.state.borrow_mut())
    }

    /// An edit of the source, as `SourceCopy::edited` takes it.
    pub fn source_edited(&self, range: Range<U16>, delta: isize, inserted: &str, length: usize) {
        self.copy.edited(range, delta, inserted, length);
    }

    /// How many edits the source has had.
    pub fn text_version(&self) -> u64 {
        self.copy.version()
    }

    /// Starts an update, and outdates every older one.
    pub fn next_generation(&self) -> u64 {
        self.copy.next_generation()
    }

    /// The newest update as a background job can read it.
    pub fn latest_generation(&self) -> Arc<AtomicU64> {
        self.copy.latest_generation()
    }

    /// Whether `generation` is the newest update.
    pub fn is_latest(&self, generation: u64) -> bool {
        self.copy.is_latest(generation)
    }

    /// Whether this call is the one that has to schedule the update.
    pub fn schedule_update(&self) -> bool {
        self.copy.schedule_update()
    }

    /// The scheduled update ran.
    pub fn update_ran(&self) {
        self.copy.update_ran();
    }

    /// Whether an update is scheduled and has not run yet.
    #[cfg(feature = "selftest")]
    pub fn update_is_scheduled(&self) -> bool {
        self.copy.update_is_scheduled()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn view(text: &str) -> (String, usize) {
        (text.to_string(), text.encode_utf16().count())
    }

    #[test]
    fn a_change_of_the_state_is_what_the_next_read_sees() {
        let model = Model::default();
        assert_eq!(model.read(|st| st.zoom()), 1.0);
        let answer = model.change(|st| {
            st.set_zoom(1.5);
            "done"
        });
        assert_eq!(answer, "done");
        assert_eq!(model.read(|st| st.zoom()), 1.5);
    }

    #[test]
    fn the_view_is_read_only_when_edits_could_not_keep_the_copy() {
        let model = Model::new();
        let reads = Cell::new(0);
        let read_view = |text: &'static str| {
            let reads = &reads;
            move || {
                reads.set(reads.get() + 1);
                view(text)
            }
        };
        let text = model.with_text(read_view("Hallo"), |snap, _| snap.text.clone());
        assert_eq!((text.as_str(), reads.get()), ("Hallo", 1));
        let again = model.with_text(read_view("nie gelesen"), |snap, _| snap.text.clone());
        assert_eq!((again.as_str(), reads.get()), ("Hallo", 1), "nothing changed");
        model.source_edited(U16::new(5)..U16::new(6), 1, "!", 6);
        let patched = model.with_text(read_view("nie gelesen"), |snap, _| (snap.text.clone(), snap.version));
        assert_eq!((patched.0.as_str(), patched.1, reads.get()), ("Hallo!", 1, 1));
        assert_eq!(model.text_version(), 1);
        model.source_edited(U16::new(0)..U16::new(1), 0, "J", 99);
        let reread = model.with_text(read_view("Jallo!"), |snap, _| snap.text.clone());
        assert_eq!(
            (reread.as_str(), reads.get()),
            ("Jallo!", 2),
            "an edit that does not fit the reported length drops the copy"
        );
    }

    #[test]
    fn a_change_with_the_text_sees_the_same_copy_and_changes_the_state() {
        let model = Model::new();
        model.with_text(|| view("Text"), |_, _| ());
        let zoom = model.change_with_text(
            || panic!("the copy is current"),
            |snap, st| {
                st.set_zoom(snap.text.len() as f64);
                st.zoom()
            },
        );
        assert_eq!(zoom, 4.0);
        assert_eq!(model.read(|st| st.zoom()), 4.0);
    }

    #[test]
    fn only_the_newest_update_counts_and_background_jobs_can_see_which_it_is() {
        let model = Model::new();
        let latest = model.latest_generation();
        let first = model.next_generation();
        assert!(model.is_latest(first));
        assert_eq!(latest.load(std::sync::atomic::Ordering::SeqCst), first);
        let second = model.next_generation();
        assert!(!model.is_latest(first));
        assert!(model.is_latest(second));
        assert_eq!(latest.load(std::sync::atomic::Ordering::SeqCst), second);
    }

    #[test]
    fn an_update_is_scheduled_once_until_it_ran() {
        let model = Model::new();
        assert!(model.schedule_update());
        assert!(!model.schedule_update());
        model.update_ran();
        assert!(model.schedule_update());
    }
}
