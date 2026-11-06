//! Zlib streams, inflated only as far as what they declare.

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};

use crate::ReadError;
use crate::source::{Source, read_some};

const CHUNK: usize = 64 * 1024;

/// A zlib stream that starts at a position of a source; where it ends shows only by reading it.
pub(crate) struct Inflater<'a> {
    source: &'a dyn Source,
    next: u64,
    state: Box<InflateState>,
    input: Vec<u8>,
    consumed: usize,
    filled: usize,
    finished: bool,
}

impl<'a> Inflater<'a> {
    pub(crate) fn new(source: &'a dyn Source, start: u64) -> Inflater<'a> {
        Inflater {
            source,
            next: start,
            state: InflateState::new_boxed(DataFormat::Zlib),
            input: vec![0; CHUNK],
            consumed: 0,
            filled: 0,
            finished: false,
        }
    }

    /// Fills `out` unless the stream ends first, and says how much it wrote.
    pub(crate) fn read(&mut self, out: &mut [u8]) -> Result<usize, ReadError> {
        let mut written = 0;
        while written < out.len() && !self.finished {
            if self.consumed == self.filled {
                self.filled = read_some(self.source, &mut self.input, self.next)?;
                self.consumed = 0;
                self.next += self.filled as u64;
            }
            let input = &self.input[self.consumed..self.filled];
            let result = inflate(&mut self.state, input, &mut out[written..], MZFlush::None);
            self.consumed += result.bytes_consumed;
            written += result.bytes_written;
            match result.status {
                Ok(MZStatus::StreamEnd) => self.finished = true,
                Ok(_) if result.bytes_consumed + result.bytes_written > 0 => {}
                _ => return Err(ReadError::Damaged),
            }
        }
        Ok(written)
    }

    /// `data`, which came from this stream, with the rest of it; the stream has to end at exactly `size` bytes. Memory grows
    /// with what the stream really yields, so a declared size alone costs none.
    pub(crate) fn finish(mut self, mut data: Vec<u8>, size: usize) -> Result<Vec<u8>, ReadError> {
        while data.len() < size {
            let have = data.len();
            data.resize((have * 2).max(CHUNK).min(size), 0);
            let count = self.read(&mut data[have..])?;
            data.truncate(have + count);
            if count == 0 {
                return Err(ReadError::Damaged);
            }
        }
        if self.read(&mut [0])? != 0 {
            return Err(ReadError::Damaged);
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{Failing, zlib};

    fn inflated(stream: Vec<u8>, size: usize) -> Result<Vec<u8>, ReadError> {
        Inflater::new(&stream, 0).finish(Vec::new(), size)
    }

    #[test]
    fn a_stream_of_the_declared_size_inflates() {
        let text = b"a line of text\n".repeat(20_000);
        assert_eq!(inflated(zlib(&text), text.len()).as_deref(), Ok(&text[..]));
        assert_eq!(inflated(zlib(b""), 0).as_deref(), Ok(&b""[..]));
    }

    #[test]
    fn a_stream_that_yields_more_or_less_than_declared_is_damaged() {
        assert_eq!(inflated(zlib(b"abcdef"), 5), Err(ReadError::Damaged));
        assert_eq!(inflated(zlib(b"abcdef"), 7), Err(ReadError::Damaged));
        assert_eq!(inflated(zlib(b"abcdef"), 0), Err(ReadError::Damaged));
    }

    #[test]
    fn a_stream_cut_short_is_damaged_wherever_it_is_cut() {
        let text = b"some text that compresses somewhat, some text that compresses somewhat";
        let stream = zlib(text);
        for keep in 0..stream.len() {
            let cut = stream[..keep].to_vec();
            assert_eq!(inflated(cut, text.len()), Err(ReadError::Damaged), "cut at {keep}");
        }
    }

    #[test]
    fn a_wrong_checksum_and_noise_are_damaged() {
        let mut stream = zlib(b"abc");
        *stream.last_mut().unwrap() ^= 1;
        assert_eq!(inflated(stream, 3), Err(ReadError::Damaged));
        assert_eq!(inflated(b"noise, not zlib".to_vec(), 3), Err(ReadError::Damaged));
    }

    #[test]
    fn a_failing_volume_is_no_damage() {
        assert_eq!(
            Inflater::new(&Failing::always(), 0).read(&mut [0; 4]),
            Err(ReadError::Io(std::io::ErrorKind::Other))
        );
    }

    #[test]
    fn a_declared_size_alone_takes_no_memory() {
        let stream = zlib(b"tiny");
        let mut inflater = Inflater::new(&stream, 0);
        let mut head = [0; 2];
        assert_eq!(inflater.read(&mut head), Ok(2));
        assert_eq!(inflater.finish(head.to_vec(), 512 << 20), Err(ReadError::Damaged));
    }
}
