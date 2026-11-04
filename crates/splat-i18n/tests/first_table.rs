//! The table of interface texts stays once installed, so this runs in a process of its own.

use splat_i18n::{install, t, tf};

#[test]
fn texts_show_their_key_until_the_first_table_is_installed() {
    assert_eq!(t("Fett"), "Fett");
    assert_eq!(tf("Spalte {}", &["1"]), "Spalte 1");
    install("\"Fett\" = \"Bold\";\n\"Spalte {}\" = \"Column {}\";\n");
    assert_eq!(t("Fett"), "Bold");
    assert_eq!(tf("Spalte {}", &["1"]), "Column 1");
    assert_eq!(t("Kursiv"), "Kursiv", "a missing entry");
    install("\"Fett\" = \"Gras\";\n");
    assert_eq!(t("Fett"), "Bold", "the first table stays");
}
