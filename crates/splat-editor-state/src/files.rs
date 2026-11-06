//! New files beside a document, such as a pasted picture, under a name no file has.

use std::fs::OpenOptions;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

/// More files of one name than anyone pastes; past this the folder behaves oddly, and the write gives up.
const MOST_TRIES: usize = 10_000;

/// The name of try `n` of a new file, counting from 1: `stem.extension`, then `stem 2.extension` and on.
pub fn numbered_name(stem: &str, extension: &str, n: usize) -> String {
    if n <= 1 {
        format!("{stem}.{extension}")
    } else {
        format!("{stem} {n}.{extension}")
    }
}

/// Writes `bytes` to a new file in `folder` under the first name of `numbered_name` that no file has, and returns its
/// path. The file is created only if none of that name exists, so a file that appears meanwhile keeps its content; a
/// write that fails leaves no file behind.
pub fn write_new_file(folder: &Path, stem: &str, extension: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    write_trying(folder, stem, extension, bytes, MOST_TRIES)
}

/// `write_new_file` with no more than `tries` names.
fn write_trying(folder: &Path, stem: &str, extension: &str, bytes: &[u8], tries: usize) -> io::Result<PathBuf> {
    for n in 1..=tries {
        let path = folder.join(numbered_name(stem, extension, n));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let written = file.write_all(bytes);
        if written.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        return written.map(|()| path);
    }
    Err(ErrorKind::AlreadyExists.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_name_has_no_number_and_the_next_count_from_two() {
        assert_eq!(numbered_name("Bild", "png", 1), "Bild.png");
        assert_eq!(numbered_name("Bild", "png", 2), "Bild 2.png");
        assert_eq!(numbered_name("Bild 1", "jpeg", 10), "Bild 1 10.jpeg");
        assert_eq!(numbered_name("Bild", "png", 0), "Bild.png");
    }

    #[test]
    fn a_new_file_never_replaces_one_that_is_there() {
        let dir = std::env::temp_dir().join(format!("splat-new-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = write_new_file(&dir, "Bild", "png", b"eins").unwrap();
        assert_eq!(first, dir.join("Bild.png"));
        std::fs::write(dir.join("Bild 2.png"), b"anderes").unwrap();
        let second = write_new_file(&dir, "Bild", "png", b"zwei").unwrap();
        assert_eq!(second, dir.join("Bild 3.png"), "the names in between are taken");
        assert_eq!(std::fs::read(&first).unwrap(), b"eins");
        assert_eq!(std::fs::read(dir.join("Bild 2.png")).unwrap(), b"anderes");
        assert_eq!(std::fs::read(&second).unwrap(), b"zwei");
        let missing = dir.join("fehlt");
        assert_eq!(
            write_new_file(&missing, "Bild", "png", b"x").unwrap_err().kind(),
            ErrorKind::NotFound
        );
        assert_eq!(
            write_trying(&dir, "Bild", "png", b"x", 3).unwrap_err().kind(),
            ErrorKind::AlreadyExists,
            "every name it may take is taken"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3, "and it wrote nothing");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writers_at_the_same_time_each_get_a_name_of_their_own() {
        let dir = std::env::temp_dir().join(format!("splat-new-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let writers: Vec<_> = (0..8u8)
            .map(|n| {
                let dir = dir.clone();
                std::thread::spawn(move || (n, write_new_file(&dir, "Bild", "png", &[n]).unwrap()))
            })
            .collect();
        let mut paths: Vec<PathBuf> = Vec::new();
        for writer in writers {
            let (n, path) = writer.join().unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), [n], "no writer took another's file");
            paths.push(path);
        }
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 8);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
