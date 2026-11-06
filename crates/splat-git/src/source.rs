//! Where the bytes of a repository come from.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use crate::ReadError;

/// Bytes read by position, so that a pack never has to fit into memory.
pub trait Source {
    /// Reads into `buffer` from `offset` on and says how many bytes it read; none means the end.
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize>;
    /// How many bytes there are.
    fn size(&self) -> u64;
}

impl Source for Vec<u8> {
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(self.len());
        let count = buffer.len().min(self.len() - start);
        buffer[..count].copy_from_slice(&self[start..start + count]);
        Ok(count)
    }

    fn size(&self) -> u64 {
        self.len() as u64
    }
}

/// A regular file and its size.
pub(crate) struct OnDisk {
    file: File,
    size: u64,
}

impl OnDisk {
    /// The regular file at `path`, or `None` if nothing or a folder lies there; a named pipe is refused without opening it,
    /// since opening one waits for a writer that never comes.
    pub(crate) fn open(path: &Path) -> Result<Option<OnDisk>, ReadError> {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if is_missing(&error) => return Ok(None),
            Err(error) => return Err(volume_error(error)),
        };
        if metadata.is_dir() {
            return Ok(None);
        }
        if !metadata.is_file() {
            return Err(ReadError::Damaged);
        }
        let file = File::open(path).map_err(volume_error)?;
        let size = metadata.len();
        Ok(Some(OnDisk { file, size }))
    }
}

impl Source for OnDisk {
    #[cfg(unix)]
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        std::os::unix::fs::FileExt::read_at(&self.file, buffer, offset)
    }

    #[cfg(not(unix))]
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset))?;
        file.read(buffer)
    }

    fn size(&self) -> u64 {
        self.size
    }
}

/// The error of a volume that failed, by its kind.
pub(crate) fn volume_error(error: io::Error) -> ReadError {
    ReadError::Io(error.kind())
}

/// Whether an error says that a path leads to nothing, rather than that the volume failed.
pub(crate) fn is_missing(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory)
}

/// One read that a signal may interrupt.
pub(crate) fn read_some(source: &dyn Source, buffer: &mut [u8], offset: u64) -> Result<usize, ReadError> {
    loop {
        match source.read_at(buffer, offset) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(volume_error(error)),
        }
    }
}

/// Fills `buffer` from `offset` on; a source that ends before that is damaged.
pub(crate) fn read_exact_at(source: &dyn Source, buffer: &mut [u8], offset: u64) -> Result<(), ReadError> {
    let mut done = 0;
    while done < buffer.len() {
        match read_some(source, &mut buffer[done..], offset + done as u64)? {
            0 => return Err(ReadError::Damaged),
            count => done += count,
        }
    }
    Ok(())
}

/// The whole of a file that is small by nature, or `None` if there is none.
pub(crate) fn read_small(path: &Path, most: u64) -> Result<Option<Vec<u8>>, ReadError> {
    let Some(file) = OnDisk::open(path)? else {
        return Ok(None);
    };
    if file.size > most {
        return Err(ReadError::TooLarge);
    }
    let mut data = vec![0; usize::try_from(file.size).map_err(|_| ReadError::TooLarge)?];
    read_exact_at(&file, &mut data, 0)?;
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{Failing, Temp};

    #[test]
    fn a_read_beyond_the_end_finds_nothing() {
        let bytes = vec![1u8, 2, 3];
        let mut buffer = [0; 2];
        assert_eq!(bytes.read_at(&mut buffer, 2).unwrap(), 1);
        assert_eq!(buffer[0], 3);
        assert_eq!(bytes.read_at(&mut buffer, 9).unwrap(), 0);
        assert_eq!(bytes.read_at(&mut buffer, u64::MAX).unwrap(), 0);
        assert_eq!(bytes.size(), 3);
    }

    #[test]
    fn a_source_that_ends_early_is_damaged() {
        let mut buffer = [0; 4];
        assert_eq!(read_exact_at(&vec![1u8, 2], &mut buffer, 0), Err(ReadError::Damaged));
    }

    #[test]
    fn an_interrupted_read_goes_on_and_a_failed_one_is_a_failed_volume() {
        let mut buffer = [0; 4];
        let interrupted_once = Failing::interrupted_once(vec![1, 2, 3, 4]);
        assert_eq!(read_exact_at(&interrupted_once, &mut buffer, 0), Ok(()));
        assert_eq!(buffer, [1, 2, 3, 4]);
        assert_eq!(
            read_exact_at(&Failing::always(), &mut buffer, 0),
            Err(ReadError::Io(std::io::ErrorKind::Other))
        );
    }

    #[test]
    fn a_small_file_is_read_whole_and_a_large_one_refused() {
        let temp = Temp::new();
        let path = temp.write("note", b"12345");
        assert_eq!(read_small(&path, 5), Ok(Some(b"12345".to_vec())));
        assert_eq!(read_small(&path, 4), Err(ReadError::TooLarge));
    }

    #[test]
    fn nothing_and_a_folder_are_no_file() {
        let temp = Temp::new();
        assert_eq!(read_small(&temp.path().join("gone"), 9), Ok(None));
        assert_eq!(read_small(temp.path(), 9), Ok(None));
        let file = temp.write("file", b"x");
        assert_eq!(read_small(&file.join("below"), 9), Ok(None));
    }

    #[test]
    fn a_position_in_a_file_is_read_without_the_rest() {
        let temp = Temp::new();
        let path = temp.write("file", b"abcdef");
        let file = OnDisk::open(&path).unwrap().unwrap();
        let mut buffer = [0; 2];
        assert_eq!(read_exact_at(&file, &mut buffer, 3), Ok(()));
        assert_eq!(&buffer, b"de");
        assert_eq!(file.size(), 6);
    }
}
