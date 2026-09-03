//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn interning_the_same_name_twice_yields_one_id() {
    let a = intern("files-tests-one.lisp");
    let b = intern("files-tests-one.lisp");
    assert_eq!(a, b);
    assert_eq!(name(a), Some("files-tests-one.lisp"));
}

#[test]
fn distinct_names_get_distinct_ids() {
    let a = intern("files-tests-two.lisp");
    let b = intern("files-tests-three.lisp");
    assert_ne!(a, b);
    assert_eq!(name(a), Some("files-tests-two.lisp"));
    assert_eq!(name(b), Some("files-tests-three.lisp"));
}

#[test]
fn the_absent_name_is_the_empty_name() {
    // A caller with an empty string where a file name belongs must land on
    // the same id as a caller with no name at all. Without this, a synthetic
    // span and a span built from `with_file("")` would print differently and
    // compare unequal.
    assert_eq!(intern(""), FileId::NONE);
    assert_eq!(name(FileId::NONE), None);
    assert!(!FileId::NONE.is_some());
    assert_eq!(FileId::default(), FileId::NONE);
}

#[test]
fn an_id_this_process_never_minted_has_no_name() {
    assert_eq!(name(FileId(u32::MAX)), None);
}
