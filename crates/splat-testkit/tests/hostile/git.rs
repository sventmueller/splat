//! Files of a repository built to hurt what reads them.

use splat_git::raw as git;

use super::common::{NOTICEABLE, grows_too_fast, shortest_of_three};

type Reading = Box<dyn Fn()>;

/// A file of a repository made of `count` pieces, its size, and what reads it.
fn git_files_that_hurt(count: usize) -> Vec<(&'static str, usize, Reading)> {
    let entry = |name: &str, at: usize| format!("100644 {name} {at}\0aaaaaaaaaaaaaaaaaaaa");
    let tree = (0..count).map(|at| entry("name", at)).collect::<String>().into_bytes();
    let umlauts = (0..count)
        .map(|at| entry("Übersicht", at))
        .collect::<String>()
        .into_bytes();
    let mut delta = vec![1];
    let mut left = u64::try_from(count).unwrap();
    while left >= 0x80 {
        delta.push(u8::try_from(left & 0x7f).unwrap() | 0x80);
        left >>= 7;
    }
    delta.push(u8::try_from(left).unwrap());
    delta.extend(std::iter::repeat_n([0x90, 1], count).flatten());
    let refs: String = (0..count)
        .map(|at| format!("{:040x} refs/heads/branch{at}\n", at + 1))
        .collect();
    let config: String = (0..count)
        .map(|at| format!("[branch \"b{at}\"]\n\tremote = \"origin\" # why\n"))
        .collect();
    vec![
        (
            "a tree without the name",
            tree.len(),
            Box::new(move || assert_eq!(git::lookup(&tree, b"absent"), Ok(None))),
        ),
        (
            "a tree of umlauts",
            umlauts.len(),
            Box::new(move || assert_eq!(git::lookup(&umlauts, "U\u{308}".as_bytes()), Ok(None))),
        ),
        (
            "a delta of tiny copies",
            delta.len(),
            Box::new(move || assert!(git::apply_delta(b"x", &delta, 1 << 30).is_ok())),
        ),
        (
            "packed refs without the ref",
            refs.len(),
            Box::new(move || assert_eq!(git::packed_ref(refs.as_bytes(), b"refs/x"), Ok(None))),
        ),
        (
            "a config of many sections",
            config.len(),
            Box::new(move || assert_eq!(git::check_format(config.as_bytes()), Ok(()))),
        ),
    ]
}

#[test]
fn reading_the_files_of_a_repository_takes_linear_time() {
    let cases = git_files_that_hurt(100_000)
        .into_iter()
        .zip(git_files_that_hurt(400_000));
    for ((name, small_len, small), (_, large_len, large)) in cases {
        let small = shortest_of_three(&small);
        let large = shortest_of_three(&large);
        if cfg!(target_arch = "aarch64") && !cfg!(debug_assertions) && !cfg!(coverage) {
            let too_fast = grows_too_fast(NOTICEABLE, small, large, small_len, large_len);
            assert!(!too_fast, "{name}: {small:?}, then {large:?}");
        }
    }
}
