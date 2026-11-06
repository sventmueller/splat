//! What crosses the modules: repositories made by the real `git` against what `git` itself says they hold, repositories built by
//! hand to hurt, and `diff` against `git diff`.

mod against_git;
mod diff_against_git;
mod hostile;
mod promises;
pub(crate) mod support;
