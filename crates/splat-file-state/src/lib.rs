//! What a document knows about its file, without a window: how the file encodes the text, which text it holds, what a save
//! under way writes, and what to do about a file that changed.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

use std::time::SystemTime;

/// Modification time and size of a file, or the mark of one that cannot be reached.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Stamp(Option<(SystemTime, u64)>);

impl Stamp {
    /// The stamp of a file that cannot be reached.
    pub const UNREACHABLE: Stamp = Stamp(None);

    /// The stamp of a file that was reached.
    pub fn new(modified: SystemTime, size: u64) -> Stamp {
        Stamp(Some((modified, size)))
    }

    /// Whether the file was reached.
    pub fn is_reached(self) -> bool {
        self.0.is_some()
    }
}

/// How the bytes of a file encode its text.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Encoding {
    /// UTF-8, with or without a byte order mark.
    #[default]
    Utf8,
    /// UTF-16, little-endian, behind its byte order mark.
    Utf16Le,
    /// UTF-16, big-endian, behind its byte order mark.
    Utf16Be,
}

impl Encoding {
    /// The file's text and how it was encoded. UTF-16 announces itself with a byte order mark, as Windows editors write
    /// it; anything else that is not UTF-8 gets `None`, since guessing would corrupt the text on the first save.
    pub fn read(bytes: Vec<u8>) -> Option<(Encoding, String)> {
        let wide = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&c| unit(c)).collect();
            String::from_utf16(&units).ok()
        };
        // UTF-32 starts like UTF-16, and half a code unit at the end cannot be written back.
        let whole = |rest: &[u8]| rest.len().is_multiple_of(2) && !rest.starts_with(&[0, 0]);
        if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE").filter(|r| whole(r)) {
            return Some((Encoding::Utf16Le, wide(rest, u16::from_le_bytes)?));
        }
        if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF").filter(|r| whole(r)) {
            return Some((Encoding::Utf16Be, wide(rest, u16::from_be_bytes)?));
        }
        Some((Encoding::Utf8, String::from_utf8(bytes).ok()?))
    }

    fn write(self, text: &str) -> Vec<u8> {
        let wide = |mark: [u8; 2], unit: fn(u16) -> [u8; 2]| {
            let mut out = Vec::with_capacity(text.len() * 2 + 2);
            out.extend_from_slice(&mark);
            out.extend(text.encode_utf16().flat_map(unit));
            out
        };
        match self {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf16Le => wide([0xFF, 0xFE], u16::to_le_bytes),
            Encoding::Utf16Be => wide([0xFE, 0xFF], u16::to_be_bytes),
        }
    }
}

/// What the file had beyond its text, written back on save so that a save changes only what was edited.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct FileFormat {
    /// The text started with a byte order mark.
    pub bom: bool,
    /// Every line ended in CRLF; lines typed since then get CRLF as well.
    pub crlf: bool,
    /// How the file encoded the text.
    pub encoding: Encoding,
}

impl FileFormat {
    /// The format of `text` as read, and the text without its byte order mark.
    pub fn of(text: &str) -> (FileFormat, &str) {
        let (bom, text) = match text.strip_prefix('\u{FEFF}') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let lines = text.matches('\n').count();
        let crlf = lines > 0 && text.matches("\r\n").count() == lines;
        (
            FileFormat {
                bom,
                crlf,
                encoding: Encoding::Utf8,
            },
            text,
        )
    }

    /// The bytes of a file in this format that holds `text`.
    pub fn encode(self, text: &str) -> Vec<u8> {
        if !self.bom && !self.crlf {
            return self.encoding.write(text);
        }
        let mut out = String::with_capacity(text.len() + 3);
        if self.bom {
            out.push('\u{FEFF}');
        }
        if !self.crlf {
            out.push_str(text);
            return self.encoding.write(&out);
        }
        for line in text.split_inclusive('\n') {
            match line.strip_suffix('\n') {
                Some(content) if !content.ends_with('\r') => {
                    out.push_str(content);
                    out.push_str("\r\n");
                }
                _ => out.push_str(line),
            }
        }
        self.encoding.write(&out)
    }
}

/// How often a file that cannot be read as text is read again, as one being written may fail for a moment.
pub const READ_TRIES: u8 = 5;

/// Failed reads of one version of a file.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct FailedReads {
    version: Stamp,
    tries: u8,
}

impl FailedReads {
    /// Counts a failed read of the version `now`.
    #[must_use]
    pub fn after_another(self, now: Stamp) -> FailedReads {
        let tries = if self.version == now {
            self.tries.saturating_add(1)
        } else {
            1
        };
        FailedReads { version: now, tries }
    }

    /// Whether another read of the same version is worth it.
    pub fn try_again(self) -> bool {
        self.tries < READ_TRIES
    }
}

/// The length and a hash of a text or a file, which tell texts and files apart without keeping a copy of them. Nothing
/// depends on it holding against someone who means to fool it: two files that collide only miss a reload.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Fingerprint {
    len: usize,
    hash: u64,
}

impl Fingerprint {
    /// The fingerprint of `bytes`. A gigabyte takes about a tenth of a second, which is what lets a save take this on
    /// the main thread.
    pub fn of(bytes: &[u8]) -> Fingerprint {
        const FACTOR: u64 = 0x9E37_79B9_7F4A_7C15;
        let mix = |hash: u64, word: u64| (hash ^ word).wrapping_mul(FACTOR).rotate_left(29);
        let (words, rest) = bytes.as_chunks::<8>();
        let mut hash = words
            .iter()
            .fold(FACTOR, |hash, &word| mix(hash, u64::from_le_bytes(word)));
        let mut last = [0u8; 8];
        last[..rest.len()].copy_from_slice(rest);
        hash = mix(hash, u64::from_le_bytes(last));
        Fingerprint { len: bytes.len(), hash }
    }

    /// Whether this is the fingerprint of `bytes`. The hash is looked at only if the length fits, which while typing it
    /// rarely does.
    pub fn is_of(self, bytes: &[u8]) -> bool {
        self.len == bytes.len() && self == Fingerprint::of(bytes)
    }
}

/// A version of the text as a file holds it.
#[derive(Clone, Copy, Default, Debug)]
struct Version {
    revision: u64,
    text: Fingerprint,
    file: Fingerprint,
}

/// What a save took: the version of the text and the bytes that hold it.
struct Taken {
    version: Version,
    bytes: Vec<u8>,
}

/// A save that has begun and not yet ended. `FileState::begin_save` hands it out, and `end_save` or `end_save_that_asks`
/// takes it back by value, so that a save ends once:
///
/// ```compile_fail,E0382
/// let mut state = splat_file_state::FileState::default();
/// let save = state.begin_save();
/// state.end_save(save);
/// state.end_save(save);
/// ```
///
/// ```compile_fail,E0599
/// let mut state = splat_file_state::FileState::default();
/// let save = state.begin_save();
/// state.end_save(save.clone());
/// state.end_save_that_asks(save);
/// ```
#[must_use = "a save that never ends keeps every later look at the file from counting"]
#[derive(Debug)]
pub struct SaveTicket(());

/// What a look at the file asked for hands in when it comes back, to tell whether a save has touched the file since.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LookTicket(Option<u64>);

/// Which text the file holds, kept apart from AppKit's change count, which follows undo steps: while typing goes on
/// through a save, no new step opens and the count stays at zero with text that no file holds.
///
/// A save goes through `begin_save`, `take_text`, then `start_writing` on its writing thread or `writes_at_once` on the
/// main thread, `written` if it wrote the document's file, and `end_save` or `end_save_that_asks`. Saves may overlap.
///
/// ```
/// use splat_file_state::{FileState, Fingerprint, OnFile};
/// let mut state = FileState::default();
/// state.text_changed();
/// let save = state.begin_save();
/// state.take_text(Fingerprint::of(b"Text"), Some(b"Text".to_vec()));
/// assert_eq!(state.start_writing(), Some(&b"Text"[..]));
/// state.written();
/// state.end_save(save);
/// assert_eq!(state.on_file(), OnFile::Yes);
/// ```
#[derive(Default)]
pub struct FileState {
    /// Counts the changes of the text.
    revision: u64,
    on_file: Version,
    /// The text came from elsewhere than the document's file, as a restored draft does, and no file holds it yet.
    from_elsewhere: bool,
    /// What the newest save took. A save that takes its text before an earlier one has started writing writes the newest
    /// text in both, so this goes with the last of them.
    taken: Option<Taken>,
    /// What the save on its way to the file writes, which the newest may have taken over from it only for later ones.
    writing: Option<Version>,
    under_way: usize,
    saves_begun: u64,
    /// A save ended in a question of AppKit, whose answer "save anyway" writes what that save took.
    asking: bool,
}

impl FileState {
    /// The text changed, by typing, undoing or any other way.
    pub fn text_changed(&mut self) {
        self.revision += 1;
        self.question_is_over();
    }

    fn question_is_over(&mut self) {
        if std::mem::take(&mut self.asking) && self.under_way == 0 {
            self.taken = None;
        }
    }

    /// The text now is what was read; `file` is what it was read from, if anything.
    pub fn loaded(&mut self, text: Fingerprint, file: Option<Fingerprint>) {
        self.from_elsewhere &= file.is_none();
        self.on_file = Version {
            revision: self.revision,
            text,
            file: file.unwrap_or(self.on_file.file),
        };
    }

    /// AppKit says that what was read is not what the document's file holds.
    pub fn read_from_elsewhere(&mut self) {
        self.from_elsewhere = true;
    }

    /// A save starts, before it takes the text.
    pub fn begin_save(&mut self) -> SaveTicket {
        self.under_way += 1;
        self.saves_begun += 1;
        SaveTicket(())
    }

    /// A save takes the text, or `None` once there is none it could write.
    pub fn take_text(&mut self, text: Fingerprint, bytes: Option<Vec<u8>>) {
        self.taken = bytes.map(|bytes| Taken {
            version: Version {
                revision: self.revision,
                text,
                file: Fingerprint::of(&bytes),
            },
            bytes,
        });
    }

    /// A save asks for what to write on its writing thread: the text it took, or a newer one that a later save took.
    pub fn start_writing(&mut self) -> Option<&[u8]> {
        self.writing = self.taken.as_ref().map(|taken| taken.version);
        self.bytes()
    }

    /// A save writes what it got on the main thread, right now.
    pub fn writes_at_once(&mut self, text: Fingerprint, written: &[u8]) {
        self.writing = Some(Version {
            revision: self.revision,
            text,
            file: Fingerprint::of(written),
        });
    }

    /// The file now holds what the save that just ended wrote.
    pub fn written(&mut self) {
        if let Some(version) = self.writing.take() {
            self.on_file = version;
            self.from_elsewhere = false;
        }
        self.question_is_over();
    }

    /// A save is over, however it went: one that failed reports only this.
    pub fn end_save(&mut self, save: SaveTicket) {
        let SaveTicket(()) = save;
        self.under_way = self.under_way.saturating_sub(1);
        if self.under_way == 0 {
            self.taken = None;
            self.writing = None;
        }
        self.asking = false;
    }

    /// A save is over, and AppKit now asks what to do about it, as for a file another program changed. Its answer "save
    /// anyway" writes on another thread with neither a new save nor a new token, so the text stays until the next change.
    pub fn end_save_that_asks(&mut self, save: SaveTicket) {
        let SaveTicket(()) = save;
        self.under_way = self.under_way.saturating_sub(1);
        self.asking = true;
    }

    /// The bytes the newest save took, as long as a save may still write them.
    pub fn bytes(&self) -> Option<&[u8]> {
        self.taken.as_ref().map(|taken| taken.bytes.as_slice())
    }

    /// Whether the file holds the text, as far as that can be told without looking at the text.
    pub fn on_file(&self) -> OnFile {
        if self.from_elsewhere {
            OnFile::No
        } else if self.revision == self.on_file.revision {
            OnFile::Yes
        } else {
            OnFile::IfTextIs(self.on_file.text)
        }
    }

    /// The text came back to what the file holds, by undoing or by typing.
    pub fn text_back_on_file(&mut self) {
        self.on_file.revision = self.revision;
    }

    /// What a look at the file asked for now hands in to count; asked for during a save, it never does.
    pub fn look_ticket(&self) -> LookTicket {
        LookTicket((self.under_way == 0).then_some(self.saves_begun))
    }

    /// Whether a look at the file says something about it: bytes read around a save may be from before it.
    pub fn look_counts(&self, ticket: LookTicket) -> bool {
        self.under_way == 0 && ticket.0 == Some(self.saves_begun)
    }

    /// Whether a file with this fingerprint is what Splat itself last wrote or read.
    pub fn is_own_file(&self, file: Fingerprint) -> bool {
        self.on_file.file == file
    }
}

/// Whether the file holds the text, as `FileState::on_file` tells it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnFile {
    /// No change of the text since the file held it.
    Yes,
    /// No file holds the text, whatever it is.
    No,
    /// Only if the text has this fingerprint.
    IfTextIs(Fingerprint),
}

/// What to do about the change mark.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeMark {
    /// Leave the mark as it is.
    Keep,
    /// Mark the document as edited.
    Set,
    /// Clear the mark.
    Clear,
}

/// Unsaved text must always show as such, or autosave, the question on closing and the copies against a crash pass it by.
pub fn mark_for(edited: bool, text_on_file: bool, may_clear: bool) -> ChangeMark {
    match (edited, text_on_file) {
        (false, false) => ChangeMark::Set,
        (true, true) if may_clear => ChangeMark::Clear,
        _ => ChangeMark::Keep,
    }
}

/// What a document without unsaved text does about a file that holds something else than last time it looked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reaction {
    /// The file holds the text, so only AppKit's idea of its date is out of date.
    TakeDate,
    /// The file holds another text, which replaces the one shown.
    Reread,
}

/// The content decides, not the date: another program may give a file any date, and a write right after Splat's own save
/// leaves AppKit believing it knows the new date.
pub fn reaction(own_file: bool, shows_it: impl FnOnce() -> bool) -> Reaction {
    if own_file || shows_it() {
        Reaction::TakeDate
    } else {
        Reaction::Reread
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document as AppKit runs it: a change mark that follows undo steps, and a text that changes without one.
    struct Doc {
        state: FileState,
        text: String,
        file: Vec<u8>,
        edited: bool,
    }

    impl Doc {
        fn opened(text: &str) -> Doc {
            let mut state = FileState::default();
            state.loaded(Fingerprint::of(text.as_bytes()), Some(Fingerprint::of(text.as_bytes())));
            Doc {
                state,
                text: text.to_string(),
                file: text.as_bytes().to_vec(),
                edited: false,
            }
        }

        /// A draft as AppKit brings it back: read from elsewhere, and marked from the start.
        fn restored(text: &str) -> Doc {
            let mut doc = Doc::opened(text);
            doc.state.read_from_elsewhere();
            doc.edited = true;
            doc
        }

        fn on_file(&mut self) -> bool {
            match self.state.on_file() {
                OnFile::Yes => true,
                OnFile::No => false,
                OnFile::IfTextIs(print) => {
                    let same = print.is_of(self.text.as_bytes());
                    if same {
                        self.state.text_back_on_file();
                    }
                    same
                }
            }
        }

        fn follow(&mut self, may_clear: bool) {
            match mark_for(self.edited, self.on_file(), may_clear) {
                ChangeMark::Keep => {}
                ChangeMark::Set => self.edited = true,
                ChangeMark::Clear => self.edited = false,
            }
        }

        /// What every change of the text does: no undo step needs to open.
        fn change(&mut self, text: &str) {
            self.text = text.to_string();
            self.state.text_changed();
            self.follow(true);
        }

        fn take(&mut self) -> SaveTicket {
            let save = self.state.begin_save();
            self.state.take_text(
                Fingerprint::of(self.text.as_bytes()),
                Some(self.text.clone().into_bytes()),
            );
            save
        }

        fn write(&mut self) {
            self.file = self.state.start_writing().unwrap().to_vec();
        }

        /// AppKit clears the mark after a save, whatever happened to the text during it.
        fn finish(&mut self, save: SaveTicket) {
            self.write();
            self.state.written();
            self.edited = false;
            self.follow(false);
            self.state.end_save(save);
        }

        fn save(&mut self) {
            let save = self.take();
            self.finish(save);
        }

        fn reaction_to(&self, file: &str) -> Reaction {
            reaction(self.state.is_own_file(Fingerprint::of(file.as_bytes())), || {
                file == self.text
            })
        }
    }

    #[test]
    fn text_typed_after_a_save_counts_as_unsaved_without_a_new_undo_step() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        doc.save();
        assert!(!doc.edited);
        doc.change("abc");
        assert!(doc.edited, "the text is ahead of the file");
    }

    #[test]
    fn text_typed_while_a_save_writes_stays_unsaved_when_it_ends() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        let save = doc.take();
        doc.change("abc");
        doc.finish(save);
        assert_eq!(doc.file, b"ab");
        assert!(doc.edited, "AppKit cleared the mark, the text says otherwise");
        assert_eq!(
            doc.reaction_to("ab"),
            Reaction::TakeDate,
            "its own file rereads nothing"
        );
    }

    #[test]
    fn a_restored_draft_stays_marked_until_a_file_holds_it() {
        let mut doc = Doc::restored("Entwurf");
        doc.change("Entwurfx");
        doc.change("Entwurf");
        assert!(doc.edited, "back to what was read, which no file holds");
        doc.follow(true);
        assert!(doc.edited);
        doc.save();
        assert!(!doc.edited);
        doc.change("Entwurfy");
        doc.change("Entwurf");
        assert!(!doc.edited, "a file holds it now");
    }

    #[test]
    fn a_draft_stays_marked_when_the_window_controllers_come_after_it() {
        let mut doc = Doc::restored("Entwurf");
        doc.state.loaded(Fingerprint::of(b"Entwurf"), None);
        doc.change("x");
        doc.change("Entwurf");
        assert!(doc.edited);
    }

    #[test]
    fn reading_the_documents_own_file_ends_a_draft() {
        let mut doc = Doc::restored("Entwurf");
        doc.state
            .loaded(Fingerprint::of(b"Datei"), Some(Fingerprint::of(b"Datei")));
        doc.text = "Datei".into();
        doc.edited = false;
        doc.change("x");
        doc.change("Datei");
        assert!(!doc.edited);
    }

    #[test]
    fn an_older_save_gets_no_credit_for_a_newer_text() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        let older = doc.take();
        doc.write();
        doc.change("abc");
        let newer = doc.take();
        doc.state.written();
        doc.state.end_save(older);
        assert_eq!(doc.file, b"ab");
        assert_eq!(doc.state.on_file(), OnFile::IfTextIs(Fingerprint::of(b"ab")));
        doc.follow(false);
        assert!(doc.edited, "the file is behind the text");
        assert!(doc.state.is_own_file(Fingerprint::of(b"ab")));
        doc.state.end_save(newer);
        assert_eq!(doc.state.bytes(), None);
        doc.change("ab");
        assert!(
            !doc.edited,
            "the newer save never wrote, and the text came back to what the file holds"
        );
    }

    #[test]
    fn a_save_that_waited_writes_the_newest_text_and_gets_credit_for_it() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        let older = doc.take();
        doc.change("abc");
        let newer = doc.take();
        doc.write();
        doc.state.written();
        doc.state.end_save(older);
        assert_eq!(doc.file, b"abc");
        assert!(doc.state.is_own_file(Fingerprint::of(b"abc")));
        doc.state.end_save(newer);
        assert_eq!(doc.state.on_file(), OnFile::Yes);
    }

    #[test]
    fn a_save_on_the_main_thread_gets_credit_for_what_it_wrote() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        let save = doc.state.begin_save();
        doc.state.writes_at_once(Fingerprint::of(b"ab"), b"ab");
        doc.state.written();
        doc.state.end_save(save);
        assert!(doc.state.is_own_file(Fingerprint::of(b"ab")));
        assert_eq!(doc.state.on_file(), OnFile::Yes);
    }

    #[test]
    fn a_write_that_is_not_to_the_file_gets_no_credit_later() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        let save = doc.take();
        doc.write();
        doc.state.end_save(save);
        doc.state.written();
        assert!(
            doc.state.is_own_file(Fingerprint::of(b"a")),
            "a copy elsewhere, which ended without touching the file"
        );
    }

    #[test]
    fn coming_back_to_the_text_on_file_clears_the_mark() {
        let mut doc = Doc::opened("ab");
        doc.change("abc");
        doc.change("abd");
        assert!(doc.edited, "same length, other text");
        doc.change("ab");
        assert!(!doc.edited);
        doc.change("abc");
        doc.follow(false);
        assert!(doc.edited);
    }

    #[test]
    fn a_step_undone_past_the_save_leaves_the_text_marked() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        doc.save();
        doc.change("abc");
        // AppKit's count reaches zero by undoing the one step it knows, with a text that differs from the file.
        doc.text = "a".into();
        doc.state.text_changed();
        doc.edited = false;
        doc.follow(true);
        assert!(doc.edited);
    }

    #[test]
    fn only_undoing_and_changing_the_text_may_clear_the_mark() {
        assert_eq!(mark_for(true, true, false), ChangeMark::Keep);
        assert_eq!(mark_for(true, true, true), ChangeMark::Clear);
        assert_eq!(mark_for(false, false, false), ChangeMark::Set);
        assert_eq!(mark_for(false, true, true), ChangeMark::Keep);
        assert_eq!(mark_for(true, false, true), ChangeMark::Keep);
    }

    #[test]
    fn a_save_that_asks_can_still_write_what_it_took_and_nothing_else() {
        let mut state = FileState::default();
        let print = Fingerprint::of(b"Text");
        let save = state.begin_save();
        state.take_text(print, Some(b"Text".to_vec()));
        state.end_save_that_asks(save);
        assert_eq!(state.bytes(), Some(&b"Text"[..]), "for the answer to save anyway");
        assert!(state.look_counts(state.look_ticket()), "no save is under way");
        assert_eq!(state.start_writing(), Some(&b"Text"[..]));
        state.written();
        assert_eq!(state.bytes(), None, "written, and no copy stays");
        assert!(state.is_own_file(Fingerprint::of(b"Text")));

        let save = state.begin_save();
        state.take_text(print, Some(b"Text".to_vec()));
        state.end_save_that_asks(save);
        state.text_changed();
        assert_eq!(
            state.bytes(),
            None,
            "a changed text must not be written over by the old one"
        );
        assert_eq!(state.start_writing(), None);
    }

    #[test]
    fn a_change_of_the_text_leaves_what_a_save_under_way_took() {
        let mut state = FileState::default();
        let save = state.begin_save();
        state.take_text(Fingerprint::of(b"Text"), Some(b"Text".to_vec()));
        state.text_changed();
        assert_eq!(state.start_writing(), Some(&b"Text"[..]));
        state.end_save(save);
    }

    #[test]
    fn a_failed_save_leaves_the_file_as_it_was_and_keeps_no_copy() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        let save = doc.take();
        doc.state.end_save(save);
        assert!(doc.state.bytes().is_none());
        assert_eq!(doc.state.on_file(), OnFile::IfTextIs(Fingerprint::of(b"a")));
        assert!(doc.state.look_counts(doc.state.look_ticket()), "looks go on afterwards");
        assert_eq!(doc.reaction_to("a"), Reaction::TakeDate);
    }

    #[test]
    fn the_text_of_a_save_goes_with_the_last_save_under_way() {
        let mut state = FileState::default();
        let first = state.begin_save();
        state.take_text(Fingerprint::default(), Some(vec![1]));
        let second = state.begin_save();
        state.take_text(Fingerprint::default(), Some(vec![2]));
        state.end_save(second);
        assert_eq!(state.bytes(), Some(&[2][..]), "the first save has yet to read it");
        assert_eq!(state.start_writing(), Some(&[2][..]));
        state.end_save(first);
        assert_eq!(state.bytes(), None);
        let third = state.begin_save();
        state.take_text(Fingerprint::default(), Some(vec![3]));
        assert_eq!(state.bytes(), Some(&[3][..]));
        state.end_save(third);
    }

    #[test]
    fn overlapping_saves_hold_back_looks_until_the_last_ends_in_whatever_order() {
        for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [1, 2, 0]] {
            let mut doc = Doc::opened("a");
            let mut saves: Vec<Option<SaveTicket>> = Vec::new();
            for text in ["ab", "abc", "abcd"] {
                doc.change(text);
                saves.push(Some(doc.take()));
            }
            for (ended, at) in order.into_iter().enumerate() {
                assert!(
                    !doc.state.look_counts(doc.state.look_ticket()),
                    "{order:?}: {ended} ended"
                );
                assert_eq!(doc.state.bytes(), Some(&b"abcd"[..]), "{order:?}: {ended} ended");
                doc.state.end_save(saves[at].take().unwrap());
            }
            assert!(doc.state.look_counts(doc.state.look_ticket()), "{order:?}");
            assert_eq!(doc.state.bytes(), None, "{order:?}");
        }
    }

    #[test]
    fn a_save_that_asks_while_another_is_under_way_keeps_the_text_for_both() {
        let mut state = FileState::default();
        let asking = state.begin_save();
        state.take_text(Fingerprint::of(b"Text"), Some(b"Text".to_vec()));
        let other = state.begin_save();
        state.end_save_that_asks(asking);
        state.text_changed();
        assert_eq!(
            state.bytes(),
            Some(&b"Text"[..]),
            "the save still under way has yet to write it"
        );
        state.end_save(other);
        assert_eq!(state.bytes(), None);
        assert!(state.look_counts(state.look_ticket()));
    }

    #[test]
    fn a_save_without_text_to_write_changes_nothing_when_it_ends() {
        let mut doc = Doc::opened("a");
        let save = doc.state.begin_save();
        doc.state.take_text(Fingerprint::of(b"x"), None);
        assert!(doc.state.start_writing().is_none());
        doc.state.written();
        doc.state.end_save(save);
        assert!(doc.state.is_own_file(Fingerprint::of(b"a")));
    }

    #[test]
    fn a_look_says_nothing_about_a_file_a_save_has_touched_since() {
        let mut doc = Doc::opened("a");
        let asked = doc.state.look_ticket();
        assert!(doc.state.look_counts(asked));
        let save = doc.state.begin_save();
        assert!(!doc.state.look_counts(asked), "under way");
        assert!(
            !doc.state.look_counts(doc.state.look_ticket()),
            "under way, and asked after it began"
        );
        doc.state.end_save(save);
        assert!(!doc.state.look_counts(asked), "asked before it");
        assert!(doc.state.look_counts(doc.state.look_ticket()));

        let save = doc.state.begin_save();
        let during = doc.state.look_ticket();
        doc.state.end_save(save);
        assert!(
            !doc.state.look_counts(during),
            "asked during it, so the bytes may be from before it"
        );
    }

    #[test]
    fn only_a_file_splat_did_not_write_rereads_the_document() {
        let mut doc = Doc::opened("eins");
        assert_eq!(doc.reaction_to("eins"), Reaction::TakeDate);
        assert_eq!(doc.reaction_to("anders"), Reaction::Reread);
        doc.change("zwei");
        doc.save();
        assert_eq!(doc.reaction_to("zwei"), Reaction::TakeDate);
        assert_eq!(
            doc.reaction_to("eins"),
            Reaction::Reread,
            "an earlier version brought back by another program, say by git, whatever its date"
        );
        doc.change("drei");
        assert_eq!(
            doc.reaction_to("drei"),
            Reaction::TakeDate,
            "the file holds what it shows"
        );
    }

    #[test]
    fn fingerprints_tell_texts_apart_and_look_at_every_byte() {
        let text = "Grüße aus dem Text, länger als ein Wort.".as_bytes();
        assert_eq!(Fingerprint::of(text), Fingerprint::of(text));
        assert!(Fingerprint::of(text).is_of(text));
        assert!(!Fingerprint::of(text).is_of(&text[1..]));
        for at in 0..text.len() {
            let mut other = text.to_vec();
            other[at] ^= 1;
            assert_ne!(Fingerprint::of(text), Fingerprint::of(&other), "byte {at}");
            assert!(!Fingerprint::of(text).is_of(&other));
        }
        assert_ne!(Fingerprint::of(b""), Fingerprint::of(b"\0"), "the length counts");
    }

    #[test]
    fn a_file_that_stays_unreadable_is_not_read_forever() {
        let version = |seconds| Stamp::new(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds), 1);
        let mut failed = FailedReads::default();
        for _ in 1..READ_TRIES {
            failed = failed.after_another(version(1));
            assert!(failed.try_again());
        }
        failed = failed.after_another(version(1));
        assert!(!failed.try_again());
        assert!(
            failed.after_another(version(2)).try_again(),
            "a new version gets new tries"
        );
    }

    #[test]
    fn byte_order_marks_that_do_not_announce_utf_16_are_refused() {
        assert!(Encoding::read(b"\xFF\xFE\0\0a\0\0\0".to_vec()).is_none(), "UTF-32");
        assert!(Encoding::read(b"\xFF\xFEa\0b".to_vec()).is_none(), "half a code unit");
        assert!(Encoding::read(b"\xFE\xFF\0a\0".to_vec()).is_none(), "half a code unit");
    }

    #[test]
    fn a_file_keeps_every_mix_of_mark_and_line_ends() {
        for (name, file) in [
            ("plain", "a\nb\n"),
            ("mark, line feeds", "\u{FEFF}a\nb\n"),
            ("no mark, Windows", "a\r\nb\r\n"),
            ("mark, Windows", "\u{FEFF}a\r\nb\r\n"),
            ("old Mac", "a\rb\r"),
        ] {
            let (format, text) = FileFormat::of(file);
            assert_eq!(format.encode(text), file.as_bytes(), "{name}");
        }
    }

    #[test]
    fn a_save_keeps_the_byte_order_mark_and_crlf_line_ends() {
        let (format, text) = FileFormat::of("\u{FEFF}# A\r\n\r\nB\r\n");
        assert_eq!(
            format,
            FileFormat {
                bom: true,
                crlf: true,
                encoding: Encoding::Utf8
            }
        );
        assert_eq!(text, "# A\r\n\r\nB\r\n");
        let typed = format!("{text}C\nD");
        assert_eq!(format.encode(&typed), "\u{FEFF}# A\r\n\r\nB\r\nC\r\nD".as_bytes());
    }

    #[test]
    fn mixed_line_ends_stay_as_they_are() {
        let (format, text) = FileFormat::of("a\r\nb\nc\n");
        assert_eq!(format, FileFormat::default());
        assert_eq!(format.encode(&format!("{text}d\n")), b"a\r\nb\nc\nd\n");
    }

    fn format_of(bytes: &[u8]) -> (FileFormat, String) {
        let (encoding, text) = Encoding::read(bytes.to_vec()).expect("text");
        let (format, text) = FileFormat::of(&text);
        (FileFormat { encoding, ..format }, text.to_string())
    }

    #[test]
    fn utf16_with_a_byte_order_mark_opens_and_saves_as_it_came() {
        let cases: [(&[u8], &str, Encoding); 3] = [
            (b"\xFF\xFEa\0\xE4\0\r\0\n\0", "aä\r\n", Encoding::Utf16Le),
            (b"\xFE\xFF\0a\0\xE4\0\r\0\n", "aä\r\n", Encoding::Utf16Be),
            ("Grüße\n".as_bytes(), "Grüße\n", Encoding::Utf8),
        ];
        for (bytes, expected, encoding) in cases {
            let (format, text) = format_of(bytes);
            assert_eq!((text.as_str(), format.encoding), (expected, encoding));
            assert_eq!(format.encode(&text), bytes, "{encoding:?}");
        }
    }

    #[test]
    fn other_encodings_and_broken_wide_text_are_refused() {
        assert!(Encoding::read(b"Gr\xFC\xDFe".to_vec()).is_none(), "no guessing");
        assert!(Encoding::read(b"\x89PNG\r\n\x1a\n".to_vec()).is_none());
        assert!(Encoding::read(b"\xFF\xFE\x00\xD8".to_vec()).is_none());
    }
}
