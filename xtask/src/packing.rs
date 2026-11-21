//! The large scripts of the bundle in LZFSE, through Apple's Compression framework, as `splat_render::scheme` unpacks
//! them for the pages: `<name>.lzfse` holds the length of the script as eight bytes little-endian and then its LZFSE
//! stream. The repository keeps the scripts as published, which `vendor check` compares.

use std::ffi::{c_int, c_void};
use std::fs;
use std::path::{Path, PathBuf};

use crate::*;

/// What the name of a packed script adds.
const PACKED: &str = ".lzfse";

/// Smaller files stay as they are: packing them saves next to nothing.
const PACKED_ABOVE: u64 = 64 << 10;

/// `COMPRESSION_LZFSE` of `<compression.h>`.
const LZFSE: c_int = 0x801;

#[link(name = "compression")]
unsafe extern "C" {
    fn compression_encode_buffer(
        dst: *mut u8,
        dst_size: usize,
        src: *const u8,
        src_size: usize,
        scratch: *mut c_void,
        algorithm: c_int,
    ) -> usize;
    fn compression_decode_buffer(
        dst: *mut u8,
        dst_size: usize,
        src: *const u8,
        src_size: usize,
        scratch: *mut c_void,
        algorithm: c_int,
    ) -> usize;
}

/// `script` packed, or `None` where that would not make it smaller.
fn pack(script: &[u8]) -> Option<Vec<u8>> {
    let mut stream = vec![0; script.len()];
    // SAFETY: Both buffers are valid for the lengths given; without a scratch buffer the library allocates its own.
    let written = unsafe {
        compression_encode_buffer(
            stream.as_mut_ptr(),
            stream.len(),
            script.as_ptr(),
            script.len(),
            std::ptr::null_mut(),
            LZFSE,
        )
    };
    // The encoder answers 0 when the stream does not fit, here when it would not be smaller.
    if written == 0 || written + 8 >= script.len() {
        return None;
    }
    stream.truncate(written);
    let mut packed = u64::try_from(script.len()).ok()?.to_le_bytes().to_vec();
    packed.extend(stream);
    Some(packed)
}

/// What `pack` packed, as the app unpacks it.
fn unpack(packed: &[u8]) -> Option<Vec<u8>> {
    let (length, stream) = packed.split_first_chunk::<8>()?;
    let length = usize::try_from(u64::from_le_bytes(*length)).ok()?;
    // The decoder stops at the end of the buffer without saying so; a byte to spare shows a stream that holds more.
    let mut bytes = vec![0; length.checked_add(1)?];
    // SAFETY: Both buffers are valid for the lengths given; without a scratch buffer the library allocates its own.
    let written = unsafe {
        compression_decode_buffer(
            bytes.as_mut_ptr(),
            bytes.len(),
            stream.as_ptr(),
            stream.len(),
            std::ptr::null_mut(),
            LZFSE,
        )
    };
    (written == length).then(|| {
        bytes.truncate(length);
        bytes
    })
}

/// Replaces every file below `dir` larger than `PACKED_ABOVE` with its packed `<name>.lzfse`, once that unpacks to the
/// same bytes; a file packing would not make smaller stays.
pub(crate) fn pack_scripts(dir: &Path) -> Result<(), String> {
    let entries: Vec<PathBuf> = files::read_dir(dir)?
        .map(|entry| entry.map(|e| e.path()).map_err(|e| format!("{}: {e}", dir.display())))
        .collect::<Result<_, _>>()?;
    for path in entries {
        if path.is_dir() {
            pack_scripts(&path)?;
            continue;
        }
        let size = fs::metadata(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .len();
        if size <= PACKED_ABOVE {
            continue;
        }
        let script = files::read(&path)?;
        let Some(packed) = pack(&script) else {
            continue;
        };
        if unpack(&packed).as_deref() != Some(&script[..]) {
            return Err(format!("{}: LZFSE does not give back the same bytes", path.display()));
        }
        let mut name = path.clone().into_os_string();
        name.push(PACKED);
        files::write(PathBuf::from(name), &packed)?;
        files::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(bytes: usize) -> Vec<u8> {
        "mermaid.initialize({ startOnLoad: false });\n"
            .repeat(bytes / 44 + 1)
            .into_bytes()[..bytes]
            .to_vec()
    }

    /// Bytes no compression makes smaller.
    fn noise(bytes: usize) -> Vec<u8> {
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        (0..bytes)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state.to_le_bytes()[0]
            })
            .collect()
    }

    #[test]
    fn a_packed_script_unpacks_to_its_bytes_and_a_broken_one_to_nothing() {
        let script = script(200_000);
        let packed = pack(&script).expect("a script gets smaller");
        assert!(packed.len() < script.len() / 10, "{} bytes", packed.len());
        assert_eq!(packed[..8], (script.len() as u64).to_le_bytes());
        assert_eq!(unpack(&packed).as_deref(), Some(&script[..]));
        let mut longer = packed.clone();
        longer[..8].copy_from_slice(&(script.len() as u64 + 1).to_le_bytes());
        assert_eq!(unpack(&longer), None);
        let mut shorter = packed.clone();
        shorter[..8].copy_from_slice(&(script.len() as u64 - 1).to_le_bytes());
        assert_eq!(unpack(&shorter), None);
        assert_eq!(unpack(&packed[..packed.len() / 2]), None);
        assert_eq!(unpack(&packed[..4]), None);
        assert_eq!(pack(&noise(100_000)), None, "noise stays as it is");
    }

    #[test]
    fn only_large_files_are_packed_and_each_as_it_unpacks() {
        let dir = std::env::temp_dir().join(format!("splat-xtask-packing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mathjax")).unwrap();
        let (large, small, noisy) = (script(300_000), script(1000), noise(100_000));
        fs::write(dir.join("mermaid.min.js"), &large).unwrap();
        fs::write(dir.join("mathjax/tex.js"), &large).unwrap();
        fs::write(dir.join("render.js"), &small).unwrap();
        fs::write(dir.join("font.woff2"), &noisy).unwrap();
        pack_scripts(&dir).unwrap();
        for packed in ["mermaid.min.js", "mathjax/tex.js"] {
            assert!(!dir.join(packed).exists(), "{packed}");
            let unpacked = unpack(&fs::read(dir.join(format!("{packed}.lzfse"))).unwrap());
            assert_eq!(unpacked.as_deref(), Some(&large[..]), "{packed}");
        }
        assert_eq!(fs::read(dir.join("render.js")).unwrap(), small);
        assert_eq!(fs::read(dir.join("font.woff2")).unwrap(), noisy);
        let mut names: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        names.sort();
        assert_eq!(names, ["font.woff2", "mathjax", "mermaid.min.js.lzfse", "render.js"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
