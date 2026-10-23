// audited: 2026-10-22
//! Each heap tag's answer to whether its free runs a destructor or a cascade scan.
//!
//! docs/impl/mutable.md
//! docs/impl/region/model.md

use super::*;

// A store in region pages needs no drop. The free scans its shell, found on
// the reference list.
#[test]
fn each_tag_decides_drop_and_cascade() {
    assert!(!needs_drop(HeapTag::Pair));
    assert!(!needs_drop(HeapTag::Float));
    assert!(!needs_drop(HeapTag::LibHandle));
    assert!(!needs_drop(HeapTag::ManagedPointer));
    assert!(!needs_drop(HeapTag::Parameter));
    assert!(!needs_drop(HeapTag::LStringMut));
    assert!(!needs_drop(HeapTag::LBytesMut));
    assert!(!needs_drop(HeapTag::LArrayMut));
    assert!(!needs_drop(HeapTag::LBox));
    assert!(!needs_drop(HeapTag::CaptureCell));
    assert!(!needs_drop(HeapTag::LSetMut));
    assert!(holds_value_refs(HeapTag::LStringMut));
    assert!(holds_value_refs(HeapTag::LBytesMut));
    assert!(holds_value_refs(HeapTag::LArrayMut));
    assert!(holds_value_refs(HeapTag::LBox));
    assert!(holds_value_refs(HeapTag::CaptureCell));
    assert!(holds_value_refs(HeapTag::LSetMut));

    assert!(needs_drop(HeapTag::LString));
    assert!(needs_drop(HeapTag::LStructMut));
    assert!(needs_drop(HeapTag::LStruct));
    assert!(needs_drop(HeapTag::Closure));
    assert!(needs_drop(HeapTag::LArray));
    assert!(needs_drop(HeapTag::LBytes));
    assert!(needs_drop(HeapTag::Syntax));
    assert!(needs_drop(HeapTag::Fiber));
    assert!(needs_drop(HeapTag::ThreadHandle));
    assert!(needs_drop(HeapTag::FFISignature));
    assert!(needs_drop(HeapTag::FFIType));
    assert!(needs_drop(HeapTag::External));
    assert!(needs_drop(HeapTag::LSet));
}
