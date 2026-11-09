// audited: 2026-11-09
//! Tests for the epoch 16 migration: a call of `import-file` becomes a call of `import`.
//!
//! docs/epochs.md

use super::migrated_text;

/// `source` migrated from epoch 15, the last epoch with `import-file`.
fn from_15(source: &str) -> String {
    migrated_text(source, 15, 16).unwrap()
}

#[test]
fn a_call_of_import_file_becomes_a_call_of_import() {
    assert_eq!(
        from_15("(def b64 ((import-file \"std/base64\")))"),
        "(def b64 ((import \"std/base64\")))"
    );
}

#[test]
fn a_computed_path_moves_with_the_call() {
    assert_eq!(
        from_15("(map (fn [p] (import-file (path/join dir p))) ps)"),
        "(map (fn [p] (import (path/join dir p))) ps)"
    );
}

#[test]
fn import_file_passed_as_a_value_stays() {
    // `import` is a macro and has no value. A rename would write
    // `(map import ps)`, which names a macro where a function belongs.
    assert_eq!(from_15("(map import-file ps)"), "(map import-file ps)");
}

#[test]
fn a_rename_from_an_older_epoch_reaches_inside_the_path() {
    assert_eq!(
        migrated_text("(import-file (car ps))", 9, 16).unwrap(),
        "(import (first ps))"
    );
}
