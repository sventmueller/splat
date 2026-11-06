//! Table commands and moving between cells.

use objc2::sel;
use objc2_foundation::NSRange;

use super::super::{Step, applied, check, select_in_source, source};
use super::{TABLE_COMMANDS, before, command, remember, undo};

pub(super) fn table_steps() -> Vec<Step> {
    let mut steps = Vec::new();
    for case in &TABLE_COMMANDS {
        steps.extend(command(case));
    }
    for (name, backwards) in [("tab", false), ("backtab", true)] {
        steps.push(applied(move |c| {
            select_in_source(&c.e, "1 | 2");
            c.e.source()
                .setSelectedRange(NSRange::new(c.e.source().selectedRange().location, 0));
            remember(c);
            let command = if backwards {
                sel!(insertBacktab:)
            } else {
                sel!(insertTab:)
            };
            let from = c.e.source().selectedRange();
            let handled = c.e.editing().source_command(command);
            let to = c.e.source().selectedRange();
            check(
                &format!("{name} in a table moves between its cells"),
                handled && to.location != from.location,
                &format!("{from:?} -> {to:?}"),
            );
        }));
        steps.push(applied(|c| {
            if source(&c.e) != before() {
                undo(c);
            }
        }));
    }
    steps
}
