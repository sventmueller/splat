//! Delta data of a pack: the sizes of base and result, then instructions that copy from the base or insert new bytes.

use std::ops::Range;

use crate::ReadError;

/// A size in the little-endian form of seven bits per byte, of which no real delta needs more than 63 bits.
fn size_at(delta: &[u8], at: &mut usize) -> Result<u64, ReadError> {
    let mut value = 0;
    let mut shift = 0;
    loop {
        let byte = *delta.get(*at).ok_or(ReadError::Damaged)?;
        *at += 1;
        if shift >= 63 {
            return Err(ReadError::Damaged);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

/// The bytes a copy instruction names: each bit of the instruction says whether the next byte of the delta holds that byte of
/// the value, and a size of zero stands for 64 KiB.
fn copied_range(command: u8, delta: &[u8], at: &mut usize) -> Result<Range<usize>, ReadError> {
    let mut read = |bits: Range<u8>| {
        let mut value = 0;
        for (index, bit) in bits.enumerate() {
            if command & (1 << bit) != 0 {
                value |= usize::from(*delta.get(*at).ok_or(ReadError::Damaged)?) << (8 * index);
                *at += 1;
            }
        }
        Ok(value)
    };
    let offset = read(0..4)?;
    let size = read(4..7)?;
    let size = if size == 0 { 0x10000 } else { size };
    Ok(offset..offset.checked_add(size).ok_or(ReadError::Damaged)?)
}

/// The object `delta` makes of `base`; memory grows with what the instructions really produce.
pub fn apply_delta(base: &[u8], delta: &[u8], limit: u64) -> Result<Vec<u8>, ReadError> {
    let mut at = 0;
    let base_size = size_at(delta, &mut at)?;
    let result_size = size_at(delta, &mut at)?;
    if base_size != base.len() as u64 {
        return Err(ReadError::Damaged);
    }
    if result_size > limit {
        return Err(ReadError::TooLarge);
    }
    let result_size = usize::try_from(result_size).map_err(|_| ReadError::TooLarge)?;
    let mut result = Vec::with_capacity(result_size.min(base.len().saturating_add(delta.len())));
    while let Some(&command) = delta.get(at) {
        at += 1;
        let piece = if command & 0x80 != 0 {
            base.get(copied_range(command, delta, &mut at)?)
                .ok_or(ReadError::Damaged)?
        } else if command != 0 {
            let inserted = delta.get(at..at + usize::from(command)).ok_or(ReadError::Damaged)?;
            at += inserted.len();
            inserted
        } else {
            return Err(ReadError::Damaged);
        };
        if piece.len() > result_size - result.len() {
            return Err(ReadError::Damaged);
        }
        result.extend_from_slice(piece);
    }
    if result.len() != result_size {
        return Err(ReadError::Damaged);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{copy, delta, insert};

    const LIMIT: u64 = 1 << 20;

    #[test]
    fn copies_and_inserts_make_the_result() {
        let made = delta(11, 9, &[copy(6, 5), insert(b"-"), copy(0, 3)]);
        assert_eq!(
            apply_delta(b"hello world", &made, LIMIT).as_deref(),
            Ok(&b"world-hel"[..])
        );
    }

    #[test]
    fn an_empty_result_needs_no_instruction() {
        assert_eq!(apply_delta(b"abc", &delta(3, 0, &[]), LIMIT).as_deref(), Ok(&b""[..]));
    }

    #[test]
    fn a_copy_of_size_zero_takes_64_kib() {
        let base = vec![7; 0x10000];
        let made = delta(0x10000, 0x10000, &[vec![0x80]]);
        assert_eq!(apply_delta(&base, &made, LIMIT), Ok(base));
    }

    #[test]
    fn a_copy_may_name_all_bytes_of_offset_and_size() {
        let base = vec![1; 0x0102_0304 + 0x0001_0203];
        let wide = vec![0xff, 0x04, 0x03, 0x02, 0x01, 0x03, 0x02, 0x01];
        let made = delta(base.len() as u64, 0x0001_0203, &[wide]);
        assert_eq!(
            apply_delta(&base, &made, 1 << 24).map(|result| result.len()),
            Ok(0x0001_0203)
        );
    }

    #[test]
    fn a_copy_past_the_end_of_the_base_is_damaged() {
        assert_eq!(
            apply_delta(b"abc", &delta(3, 2, &[copy(2, 2)]), LIMIT),
            Err(ReadError::Damaged)
        );
        assert_eq!(
            apply_delta(b"abc", &delta(3, 1, &[copy(9, 1)]), LIMIT),
            Err(ReadError::Damaged)
        );
    }

    #[test]
    fn a_base_of_another_size_than_declared_is_damaged() {
        assert_eq!(
            apply_delta(b"abc", &delta(4, 1, &[copy(0, 1)]), LIMIT),
            Err(ReadError::Damaged)
        );
    }

    #[test]
    fn a_result_of_another_size_than_declared_is_damaged() {
        assert_eq!(
            apply_delta(b"abc", &delta(3, 2, &[copy(0, 3)]), LIMIT),
            Err(ReadError::Damaged)
        );
        assert_eq!(
            apply_delta(b"abc", &delta(3, 4, &[copy(0, 3)]), LIMIT),
            Err(ReadError::Damaged)
        );
        assert_eq!(
            apply_delta(b"abc", &delta(3, 1, &[insert(b"xy")]), LIMIT),
            Err(ReadError::Damaged)
        );
    }

    #[test]
    fn a_declared_result_above_the_limit_is_too_large_before_anything_is_copied() {
        assert_eq!(
            apply_delta(b"abc", &delta(3, LIMIT + 1, &[]), LIMIT),
            Err(ReadError::TooLarge)
        );
    }

    #[test]
    fn the_reserved_instruction_is_damaged() {
        assert_eq!(
            apply_delta(b"abc", &delta(3, 0, &[vec![0]]), LIMIT),
            Err(ReadError::Damaged)
        );
    }

    #[test]
    fn an_instruction_cut_short_is_damaged() {
        assert_eq!(
            apply_delta(b"abc", &delta(3, 1, &[vec![0x91]]), LIMIT),
            Err(ReadError::Damaged)
        );
        assert_eq!(
            apply_delta(b"abc", &delta(3, 3, &[vec![3, b'x']]), LIMIT),
            Err(ReadError::Damaged)
        );
    }

    #[test]
    fn sizes_cut_short_or_without_end_are_damaged() {
        assert_eq!(apply_delta(b"", b"", LIMIT), Err(ReadError::Damaged));
        assert_eq!(apply_delta(b"", &[0], LIMIT), Err(ReadError::Damaged));
        assert_eq!(apply_delta(b"", &[0x80], LIMIT), Err(ReadError::Damaged));
        assert_eq!(apply_delta(b"", &[0xff; 12], LIMIT), Err(ReadError::Damaged));
    }

    #[test]
    fn amplification_stops_at_the_declared_result() {
        let base = vec![0; 0xff_ffff];
        let huge_copy = vec![0xf0, 0xff, 0xff, 0xff];
        let many = vec![huge_copy; 64];
        let made = delta(base.len() as u64, 0xff_ffff, &many);
        assert_eq!(apply_delta(&base, &made, 1 << 30), Err(ReadError::Damaged));
    }
}
