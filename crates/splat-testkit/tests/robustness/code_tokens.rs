//! Tokens of code in every language the highlighter knows.

use splat_testkit::{Rng, rounds};

#[test]
fn tokens_of_any_text_are_in_order_apart_and_on_character_boundaries() {
    const PIECES: [&str; 18] = [
        "def ", "fn ", "'", "\"", "\"\"\"", "#", "//", "/*", "*/", "<", ">", "--", "%%", "1", "ä", "😀", "\n", "\\",
    ];
    let mut rng = Rng::new(0x5eed_c0de_0001);
    for _ in 0..rounds(300) {
        let code: String = (0..rng.below(30)).map(|_| *rng.pick(&PIECES)).collect();
        for lang in splat_markdown::code::language_names() {
            let found = splat_markdown::code::tokens(lang, &code);
            let mut at = 0;
            for (range, _) in found {
                assert!(
                    range.start >= at && range.start < range.end,
                    "{}: {range:?} of {code:?}",
                    lang
                );
                assert!(
                    code.is_char_boundary(range.start) && code.is_char_boundary(range.end),
                    "{code:?}"
                );
                at = range.end;
            }
            assert!(at <= code.len());
        }
    }
}
