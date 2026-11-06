# The partial parse

After an edit, `crates/splat-markdown/src/incremental.rs` parses only the top-level blocks around the change and keeps the
others. A keystroke in a large document would otherwise take a full parse. It must give the tree, the colours and the facts
a full parse gives; the keystroke chains in `crates/splat-testkit/tests/robustness/` and `fuzz/fuzz_targets/reparse.rs`
compare exactly that, and `cargo test --profile checked` also looks after every partial parse in a debug build.

## Why that can hold

Markdown is not local: a fence or an HTML comment runs across blank lines, and some things reach through the whole text.
So nothing here relies on blank lines ending anything. It relies on two arguments:

1. **Resynchronising.** A top-level block is what comrak makes of its lines given how the block before it ended. The part
   parsed again includes one unchanged block on each side (`Change::neighbours_hold`), with the lines of link definitions
   right above the first of them, which comrak reads out of the same paragraph (`block_start_with_definitions`). If
   those come out exactly as they were, moved by the length of the change, the parser was in the same state at both ends,
   and what lies outside cannot have changed. If they do not, the part grows on both sides and is tried again.
2. **Global effects are enumerated.** Everything that the argument above does not cover is a rule below. If a rule cannot be
   shown to hold, the whole text is parsed, and `WholeParse` names why.

## The rules

| # | What reaches beyond a block | Checked in | `WholeParse` |
|---|---|---|---|
| 1 | The state of the parser at the ends of the part: open fences, comments, lists, HTML blocks | `neighbours_hold`, `parse_window` for the end of the text | `OpenBlock`, `TooMuch` |
| 2 | Footnotes that cannot be told from the tree: inline notes, a definition nobody refers to, a reference hidden in text or in the title of a callout | `footnotes_can_be_told` | `Footnotes` |
| 3 | Numbers of footnotes and the order of their definitions: comrak numbers them in the whole text and moves the definitions behind it | `references_keep_their_numbers`, `footnotes_stay`, `source_order` | `Footnotes` |
| 4 | Link reference definitions: one anywhere turns brackets anywhere into links, and the first of a label counts, compared by Unicode case folding (`label_key`) | `link_definitions_stay`, `link_definitions_of` in `doc/facts.rs` | `LinkDefinitions` |
| 5 | `%%` comments: an unclosed one runs on to the next one anywhere | `link_definitions_stay`, `read_as_a_whole` | `LooseComment` |
| 6 | The budget comrak gives reference links in a text | `expansions_after`, `expansions_fit` in `doc/facts.rs` | `LinkDefinitions` |
| 7 | Front matter: a `---` at the very start pairs with the next `---` line anywhere | `reparse_or_why_not` | `FrontMatter` |
| 8 | Formulas and inline notes so many that the reader switches them off in the whole text | `read_as_a_whole` | `SwitchedOff` |
| 9 | A part so large that a full parse costs no more, or a text too large to parse | `part_between`, `reparse_or_why_not` | `TooMuch` |

Positions: blocks of a part count from its own start until `PartBlocks::placed_at`, and `TextPart` pairs a part with where it
starts, so that neither kind can be mistaken for positions in the whole text.

## When comrak changes

1. Run `cargo test -p splat-markdown assumptions`: `reader/assumptions.rs` pins what the rules take for granted, and names what
   changed.
2. Read its release notes for anything in the table: a new extension that reads across blocks needs a rule, and an entry here.
3. Run `make test-long` and `make fuzz`. A difference from a full parse fails there with the smallest text that shows it.
