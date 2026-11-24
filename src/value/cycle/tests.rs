// audited: 2026-11-18
//! The cycle guards: a second entry is refused while the first is held, and a pair is one entry either way round.

use super::*;

#[test]
fn fmt_enter_detects_reentry() {
    let ptr = 0xDEAD_BEEF_usize;
    let guard = fmt_enter(ptr);
    assert!(guard.is_some(), "first entry should succeed");
    assert!(fmt_enter(ptr).is_none(), "reentry should fail");
    drop(guard);
    let guard2 = fmt_enter(ptr);
    assert!(guard2.is_some(), "entry after drop should succeed");
}

#[test]
fn cmp_enter_normalizes_pair_order() {
    let a = 100_usize;
    let b = 200_usize;
    let guard = cmp_enter(a, b);
    assert!(guard.is_some());
    // (b, a) should hit the same entry
    assert!(cmp_enter(b, a).is_none());
    // (a, b) should also hit
    assert!(cmp_enter(a, b).is_none());
    drop(guard);
    assert!(cmp_enter(a, b).is_some());
}

#[test]
fn cmp_enter_same_pointer_twice() {
    let a = 42_usize;
    let guard = cmp_enter(a, a);
    assert!(guard.is_some());
    assert!(cmp_enter(a, a).is_none());
    drop(guard);
}
