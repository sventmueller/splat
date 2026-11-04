//! What a document knows about its file, without a window: how the file encodes the text, which text it holds, what a save
//! under way writes, and what to do about a file that changed.

use std::time::SystemTime;

/// Modification time and size of a file; `None` when it cannot be reached.
pub type Stamp = Option<(SystemTime, u64)>;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf16Le,
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
    pub bom: bool,
    /// Every line ended in CRLF; lines typed since then get CRLF as well.
    pub crlf: bool,
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

    pub fn try_again(self) -> bool {
        self.tries < READ_TRIES
    }
}

/// Tells texts and files apart without keeping a copy of them. Nothing depends on it holding against someone who means
/// to fool it: two files that collide only miss a reload.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Print {
    len: usize,
    hash: u64,
}

impl Print {
    /// A gigabyte takes about a tenth of a second, which is what lets a save take this on the main thread.
    pub fn of(bytes: &[u8]) -> Print {
        const FACTOR: u64 = 0x9E37_79B9_7F4A_7C15;
        let mix = |hash: u64, word: u64| (hash ^ word).wrapping_mul(FACTOR).rotate_left(29);
        let (words, rest) = bytes.as_chunks::<8>();
        let mut hash = words
            .iter()
            .fold(FACTOR, |hash, &word| mix(hash, u64::from_le_bytes(word)));
        let mut last = [0u8; 8];
        last[..rest.len()].copy_from_slice(rest);
        hash = mix(hash, u64::from_le_bytes(last));
        Print { len: bytes.len(), hash }
    }

    /// The hash is looked at only if the length fits, which while typing it rarely does.
    pub fn is_of(self, bytes: &[u8]) -> bool {
        self.len == bytes.len() && self == Print::of(bytes)
    }
}

/// A version of the text as a file holds it.
#[derive(Clone, Copy, Default, Debug)]
struct Version {
    revision: u64,
    text: Print,
    file: Print,
}

/// Which text the file holds, kept apart from AppKit's change count, which follows undo steps: while typing goes on
/// through a save, no new step opens and the count stays at zero with text that no file holds.
#[derive(Default)]
pub struct FileState {
    /// Counts the changes of the text.
    revision: u64,
    on_file: Version,
    /// The text came from elsewhere than the document's file, as a restored draft does, and no file holds it yet.
    from_elsewhere: bool,
    /// What the newest save took. A save that takes its text before an earlier one has started writing writes the newest
    /// text in both, so this and `bytes` go with the last of them.
    taken: Option<Version>,
    bytes: Option<Vec<u8>>,
    /// What the save on its way to the file writes, which the newest may have taken over from it only for later ones.
    writing: Option<Version>,
    under_way: usize,
    saves_begun: u64,
    /// A save ended in a question of AppKit, whose answer "save anyway" writes what that save took.
    asking: bool,
}

impl FileState {
    pub fn text_changed(&mut self) {
        self.revision += 1;
        self.question_is_over();
    }

    fn question_is_over(&mut self) {
        if std::mem::take(&mut self.asking) && self.under_way == 0 {
            self.taken = None;
            self.bytes = None;
        }
    }

    /// The text now is what was read; `file` is what it was read from, if anything.
    pub fn loaded(&mut self, text: Print, file: Option<Print>) {
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

    pub fn begin_save(&mut self) {
        self.under_way += 1;
        self.saves_begun += 1;
    }

    /// A save takes the text, or `None` once there is none it could write.
    pub fn take_text(&mut self, text: Print, bytes: Option<Vec<u8>>) {
        self.taken = bytes.as_deref().map(|written| Version {
            revision: self.revision,
            text,
            file: Print::of(written),
        });
        self.bytes = bytes;
    }

    /// A save asks for what to write on its writing thread: the text it took, or a newer one that a later save took.
    pub fn start_writing(&mut self) -> Option<&[u8]> {
        self.writing = self.taken;
        self.bytes.as_deref()
    }

    /// A save writes what it got on the main thread, right now.
    pub fn writes_at_once(&mut self, text: Print, written: &[u8]) {
        self.writing = Some(Version {
            revision: self.revision,
            text,
            file: Print::of(written),
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
    pub fn end_save(&mut self) {
        self.under_way = self.under_way.saturating_sub(1);
        if self.under_way == 0 {
            self.taken = None;
            self.bytes = None;
            self.writing = None;
        }
        self.asking = false;
    }

    /// A save is over, and AppKit now asks what to do about it, as for a file another program changed. Its answer "save
    /// anyway" writes on another thread with neither a new save nor a new token, so the text stays until the next change.
    pub fn end_save_that_asks(&mut self) {
        self.under_way = self.under_way.saturating_sub(1);
        self.asking = true;
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
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
    pub fn look_ticket(&self) -> Option<u64> {
        (self.under_way == 0).then_some(self.saves_begun)
    }

    /// Whether a look at the file says something about it: bytes read around a save may be from before it.
    pub fn look_counts(&self, ticket: Option<u64>) -> bool {
        self.under_way == 0 && ticket == Some(self.saves_begun)
    }

    /// Whether `file` is what Splat itself last wrote or read.
    pub fn holds(&self, file: Print) -> bool {
        self.on_file.file == file
    }
}

/// See `FileState::on_file`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnFile {
    Yes,
    No,
    IfTextIs(Print),
}

/// What to do about the change mark.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeMark {
    Keep,
    Set,
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
            state.loaded(Print::of(text.as_bytes()), Some(Print::of(text.as_bytes())));
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

        fn take(&mut self) {
            self.state.begin_save();
            self.state
                .take_text(Print::of(self.text.as_bytes()), Some(self.text.clone().into_bytes()));
        }

        fn write(&mut self) {
            self.file = self.state.start_writing().unwrap().to_vec();
        }

        /// AppKit clears the mark after a save, whatever happened to the text during it.
        fn finish(&mut self) {
            self.write();
            self.state.written();
            self.edited = false;
            self.follow(false);
            self.state.end_save();
        }

        fn save(&mut self) {
            self.take();
            self.finish();
        }

        fn reaction_to(&self, file: &str) -> Reaction {
            reaction(self.state.holds(Print::of(file.as_bytes())), || file == self.text)
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
        doc.take();
        doc.change("abc");
        doc.finish();
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
        doc.state.loaded(Print::of(b"Entwurf"), None);
        doc.change("x");
        doc.change("Entwurf");
        assert!(doc.edited);
    }

    #[test]
    fn reading_the_documents_own_file_ends_a_draft() {
        let mut doc = Doc::restored("Entwurf");
        doc.state.loaded(Print::of(b"Datei"), Some(Print::of(b"Datei")));
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
        doc.take();
        doc.write();
        doc.change("abc");
        doc.take();
        doc.state.written();
        doc.state.end_save();
        assert_eq!(doc.file, b"ab");
        assert_eq!(doc.state.on_file(), OnFile::IfTextIs(Print::of(b"ab")));
        doc.follow(false);
        assert!(doc.edited, "the file is behind the text");
        assert!(doc.state.holds(Print::of(b"ab")));
        doc.state.end_save();
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
        doc.take();
        doc.change("abc");
        doc.take();
        doc.write();
        doc.state.written();
        doc.state.end_save();
        assert_eq!(doc.file, b"abc");
        assert!(doc.state.holds(Print::of(b"abc")));
    }

    #[test]
    fn a_save_on_the_main_thread_gets_credit_for_what_it_wrote() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        doc.state.begin_save();
        doc.state.writes_at_once(Print::of(b"ab"), b"ab");
        doc.state.written();
        doc.state.end_save();
        assert!(doc.state.holds(Print::of(b"ab")));
        assert_eq!(doc.state.on_file(), OnFile::Yes);
    }

    #[test]
    fn a_write_that_is_not_to_the_file_gets_no_credit_later() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        doc.take();
        doc.write();
        doc.state.end_save();
        doc.state.written();
        assert!(
            doc.state.holds(Print::of(b"a")),
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
        let print = Print::of(b"Text");
        state.begin_save();
        state.take_text(print, Some(b"Text".to_vec()));
        state.end_save_that_asks();
        assert_eq!(state.bytes(), Some(&b"Text"[..]), "for the answer to save anyway");
        assert_eq!(state.start_writing(), Some(&b"Text"[..]));
        state.written();
        assert_eq!(state.bytes(), None, "written, and no copy stays");
        assert!(state.holds(Print::of(b"Text")));

        state.begin_save();
        state.take_text(print, Some(b"Text".to_vec()));
        state.end_save_that_asks();
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
        state.begin_save();
        state.take_text(Print::of(b"Text"), Some(b"Text".to_vec()));
        state.text_changed();
        assert_eq!(state.start_writing(), Some(&b"Text"[..]));
    }

    #[test]
    fn a_failed_save_leaves_the_file_as_it_was_and_keeps_no_copy() {
        let mut doc = Doc::opened("a");
        doc.change("ab");
        doc.take();
        doc.state.end_save();
        assert!(doc.state.bytes().is_none());
        assert_eq!(doc.state.on_file(), OnFile::IfTextIs(Print::of(b"a")));
        assert!(doc.state.look_counts(doc.state.look_ticket()), "looks go on afterwards");
        assert_eq!(doc.reaction_to("a"), Reaction::TakeDate);
    }

    #[test]
    fn the_text_of_a_save_goes_with_the_last_save_under_way() {
        let mut state = FileState::default();
        state.begin_save();
        state.take_text(Print::default(), Some(vec![1]));
        state.begin_save();
        state.take_text(Print::default(), Some(vec![2]));
        state.end_save();
        assert_eq!(state.bytes(), Some(&[2][..]), "the second save has yet to read it");
        state.end_save();
        assert_eq!(state.bytes(), None);
        state.end_save();
        state.begin_save();
        state.take_text(Print::default(), Some(vec![3]));
        assert_eq!(state.bytes(), Some(&[3][..]), "an end too many counts for nothing");
    }

    #[test]
    fn a_save_without_text_to_write_changes_nothing_when_it_ends() {
        let mut doc = Doc::opened("a");
        doc.state.begin_save();
        doc.state.take_text(Print::of(b"x"), None);
        assert!(doc.state.start_writing().is_none());
        doc.state.written();
        assert!(doc.state.holds(Print::of(b"a")));
    }

    #[test]
    fn a_look_says_nothing_about_a_file_a_save_has_touched_since() {
        let mut doc = Doc::opened("a");
        let asked = doc.state.look_ticket();
        assert!(doc.state.look_counts(asked));
        doc.state.begin_save();
        assert!(!doc.state.look_counts(asked), "under way");
        assert!(
            !doc.state.look_counts(doc.state.look_ticket()),
            "under way, and asked after it began"
        );
        doc.state.end_save();
        assert!(!doc.state.look_counts(asked), "asked before it");
        assert!(doc.state.look_counts(doc.state.look_ticket()));

        doc.state.begin_save();
        let during = doc.state.look_ticket();
        doc.state.end_save();
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
    fn prints_tell_texts_apart_and_look_at_every_byte() {
        let text = "Grüße aus dem Text, länger als ein Wort.".as_bytes();
        assert_eq!(Print::of(text), Print::of(text));
        assert!(Print::of(text).is_of(text));
        assert!(!Print::of(text).is_of(&text[1..]));
        for at in 0..text.len() {
            let mut other = text.to_vec();
            other[at] ^= 1;
            assert_ne!(Print::of(text), Print::of(&other), "byte {at}");
            assert!(!Print::of(text).is_of(&other));
        }
        assert_ne!(Print::of(b""), Print::of(b"\0"), "the length counts");
    }

    #[test]
    fn a_file_that_stays_unreadable_is_not_read_forever() {
        let version = |seconds| Some((std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds), 1));
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
