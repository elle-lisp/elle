//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn level_widens_by_multiplication() {
    // Spec: n steps of `width` spaces each is exactly n * width spaces.
    assert_eq!(
        IndentLevel::new(3).widen(IndentWidth::new(2)),
        Indent::new(6)
    );
    assert_eq!(
        IndentLevel::new(1).widen(IndentWidth::new(4)),
        Indent::new(4)
    );
}

#[test]
fn zero_level_widens_to_no_indent() {
    assert_eq!(IndentLevel::new(0).widen(IndentWidth::new(8)), Indent::ZERO);
}

#[test]
fn column_advance_and_plus() {
    assert_eq!(Column::new(5).advance(3), Column::new(8));
    // position + flat width = end column
    assert_eq!(Column::new(5).plus(Column::new(3)), Column::new(8));
}

#[test]
fn checked_plus_detects_overflow() {
    assert_eq!(
        Column::new(1).checked_plus(Column::new(2)),
        Some(Column::new(3))
    );
    assert_eq!(Column::new(usize::MAX).checked_plus(Column::new(1)), None);
}

#[test]
fn fits_is_inclusive_of_the_budget() {
    let budget = LineWidth::new(80);
    assert!(Column::new(80).fits(budget));
    assert!(!Column::new(81).fits(budget));
}

#[test]
fn half_is_the_midpoint_column() {
    assert_eq!(LineWidth::new(80).half(), Column::new(40));
    // Odd widths round down, matching integer division.
    assert_eq!(LineWidth::new(15).half(), Column::new(7));
}

#[test]
fn indent_widens_then_stacks() {
    let base = IndentLevel::new(1).widen(IndentWidth::new(2)); // 2
    let nested = base.plus(IndentLevel::new(1).widen(IndentWidth::new(2))); // +2
    assert_eq!(nested, Indent::new(4));
    assert_eq!(nested.as_column(), Column::new(4));
    assert_eq!(nested.spaces(), "    ");
}
