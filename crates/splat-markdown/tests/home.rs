//! A process of its own, since the home folder can be given only once.

use std::path::PathBuf;

use splat_markdown::url::{home, set_home};

#[test]
fn the_home_folder_the_app_gives_comes_before_home() {
    assert_eq!(home(), std::env::var_os("HOME").map(PathBuf::from));
    let given = PathBuf::from("/Users/elsewhere");
    assert_ne!(home().as_ref(), Some(&given));
    set_home(given.clone());
    assert_eq!(home(), Some(given));
}
