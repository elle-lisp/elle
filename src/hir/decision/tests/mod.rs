//! Tests for decision tree compilation.

use super::*;
use crate::hir::{HirPattern, PatternLiteral};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

// Helper: create a literal int pattern.
fn lit_int(n: i64) -> HirPattern {
    HirPattern::Literal(PatternLiteral::Int(n))
}

// Helper: create a keyword pattern.
fn lit_kw(s: &str) -> HirPattern {
    HirPattern::Literal(PatternLiteral::Keyword(s.to_string()))
}

// Helper: create a variable binding pattern.
fn var(b: u32) -> HirPattern {
    HirPattern::Var(Binding(b))
}

// Bring the private `redundancy` module into `tests` scope so that
// child test modules can reach its `pub(super)` test hooks via
// `super::redundancy::…` (their `super` is this module).
use crate::hir::decision::redundancy;

mod compile;
mod hashing;
mod reachability;
