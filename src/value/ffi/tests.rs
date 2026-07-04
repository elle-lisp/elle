//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_lib_handle() {
    let h1 = LibHandle(1);
    let h2 = LibHandle(1);
    let h3 = LibHandle(2);

    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
}
