//! Pattern matching in HIR

use super::binding::Binding;
use crate::value::SymbolId;

/// HIR pattern for match expressions
#[derive(Debug, Clone)]
pub enum HirPattern {
    /// Match anything, don't bind
    Wildcard,

    /// Match nil
    Nil,

    /// Match a literal value
    Literal(PatternLiteral),

    /// Bind to a variable
    Var(Binding),

    /// Match cons cell and destructure
    Cons {
        head: Box<HirPattern>,
        tail: Box<HirPattern>,
    },

    /// Match a list pattern with optional rest
    List {
        elements: Vec<HirPattern>,
        rest: Option<Box<HirPattern>>,
    },

    /// Match an array \[...\] pattern with optional rest (emits IsArray guard)
    Tuple {
        elements: Vec<HirPattern>,
        rest: Option<Box<HirPattern>>,
    },

    /// Match an array @\[...\] pattern with optional rest (emits IsArrayMut guard)
    Array {
        elements: Vec<HirPattern>,
        rest: Option<Box<HirPattern>>,
    },

    /// Match a struct {...} by keyword or symbol keys (emits IsStruct guard).
    /// Used by binding forms (def, var, let, fn params): missing keys signal an error.
    /// When `rest` is Some, collects all keys NOT explicitly named into a new immutable struct.
    Struct {
        entries: Vec<(PatternKey, HirPattern)>,
        rest: Option<Box<HirPattern>>,
    },

    /// Match a mutable @struct @{...} by keyword or symbol keys (emits IsStructMut guard).
    /// Used by binding forms: missing keys signal an error.
    /// When `rest` is Some, collects all keys NOT explicitly named into a new immutable struct.
    Table {
        entries: Vec<(PatternKey, HirPattern)>,
        rest: Option<Box<HirPattern>>,
    },

    /// Match a &named parameter struct: keyword or symbol keys with silent nil on missing.
    /// Used only by &named parameter destructuring, where absent keys are valid (nil).
    NamedStruct {
        entries: Vec<(PatternKey, HirPattern)>,
    },

    /// Match a set |x| pattern (emits IsSet guard, binds whole set)
    Set { binding: Box<HirPattern> },

    /// Match a mutable set @|x| pattern (emits IsSetMut guard, binds whole set)
    SetMut { binding: Box<HirPattern> },

    /// Match any of the alternative patterns.
    /// All alternatives must bind the same set of variable names.
    Or(Vec<HirPattern>),
}

/// Literal values that can appear in patterns
#[derive(Debug, Clone, PartialEq)]
pub enum PatternLiteral {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Keyword(String),
}

impl Eq for PatternLiteral {}

impl std::hash::Hash for PatternLiteral {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Discriminant first so different variants never collide.
        std::mem::discriminant(self).hash(state);
        match self {
            PatternLiteral::Bool(b) => b.hash(state),
            PatternLiteral::Int(n) => n.hash(state),
            PatternLiteral::Float(f) => f.to_bits().hash(state),
            PatternLiteral::String(s) | PatternLiteral::Keyword(s) => s.hash(state),
        }
    }
}

/// Key type in struct/table patterns: keyword (:foo) or symbol ('foo)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PatternKey {
    Keyword(String),
    Symbol(SymbolId),
}

/// Bindings introduced by a pattern
#[derive(Debug, Clone, Default)]
pub struct PatternBindings {
    pub bindings: Vec<Binding>,
}

impl PatternBindings {
    pub fn new() -> Self {
        PatternBindings {
            bindings: Vec::new(),
        }
    }

    pub fn add(&mut self, binding: Binding) {
        self.bindings.push(binding);
    }

    pub fn extend(&mut self, other: &PatternBindings) {
        self.bindings.extend(other.bindings.iter().copied());
    }
}

impl HirPattern {
    /// Collect all bindings introduced by this pattern
    pub fn bindings(&self) -> PatternBindings {
        let mut result = PatternBindings::new();
        self.collect_bindings(&mut result);
        result
    }

    fn collect_bindings(&self, out: &mut PatternBindings) {
        match self {
            HirPattern::Var(binding) => out.add(*binding),
            HirPattern::Cons { head, tail } => {
                head.collect_bindings(out);
                tail.collect_bindings(out);
            }
            HirPattern::List { elements, rest }
            | HirPattern::Tuple { elements, rest }
            | HirPattern::Array { elements, rest } => {
                for p in elements {
                    p.collect_bindings(out);
                }
                if let Some(r) = rest {
                    r.collect_bindings(out);
                }
            }
            HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
                for (_, pattern) in entries {
                    pattern.collect_bindings(out);
                }
                if let Some(r) = rest {
                    r.collect_bindings(out);
                }
            }
            HirPattern::NamedStruct { entries } => {
                for (_, pattern) in entries {
                    pattern.collect_bindings(out);
                }
            }
            HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
                binding.collect_bindings(out);
            }
            HirPattern::Or(alternatives) => {
                // All alternatives bind the same variables; collect from the first
                if let Some(first) = alternatives.first() {
                    first.collect_bindings(out);
                }
            }
            HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        }
    }

    /// Return the set of SymbolIds bound by this pattern.
    pub fn binding_names(&self) -> std::collections::BTreeSet<SymbolId> {
        let mut names = std::collections::BTreeSet::new();
        self.collect_binding_names(&mut names);
        names
    }

    fn collect_binding_names(&self, out: &mut std::collections::BTreeSet<SymbolId>) {
        match self {
            HirPattern::Var(binding) => {
                out.insert(binding.name());
            }
            HirPattern::Cons { head, tail } => {
                head.collect_binding_names(out);
                tail.collect_binding_names(out);
            }
            HirPattern::List { elements, rest }
            | HirPattern::Tuple { elements, rest }
            | HirPattern::Array { elements, rest } => {
                for p in elements {
                    p.collect_binding_names(out);
                }
                if let Some(r) = rest {
                    r.collect_binding_names(out);
                }
            }
            HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
                for (_, pattern) in entries {
                    pattern.collect_binding_names(out);
                }
                if let Some(r) = rest {
                    r.collect_binding_names(out);
                }
            }
            HirPattern::NamedStruct { entries } => {
                for (_, pattern) in entries {
                    pattern.collect_binding_names(out);
                }
            }
            HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
                binding.collect_binding_names(out);
            }
            HirPattern::Or(alts) => {
                if let Some(first) = alts.first() {
                    first.collect_binding_names(out);
                }
            }
            HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        }
    }
}

/// Check if a match expression's arms are exhaustive.
///
/// A match is considered exhaustive if:
/// - Any arm's pattern is a wildcard or variable (always matches) without a guard, OR
/// - The arms cover both `true` and `false` boolean literals (without guards),
///   including via or-patterns
pub fn is_exhaustive_match(
    arms: &[(HirPattern, Option<super::expr::Hir>, super::expr::Hir)],
) -> bool {
    // Check if any arm is a catch-all (wildcard or variable without guard)
    // Typically the last arm, but check all for robustness
    for (pat, guard, _) in arms {
        if guard.is_none() && is_catch_all(pat) {
            return true;
        }
    }

    // Check if all boolean values are covered (without guards)
    let mut has_true = false;
    let mut has_false = false;
    for (pat, guard, _) in arms {
        if guard.is_none() {
            collect_bool_coverage(pat, &mut has_true, &mut has_false);
        }
    }
    if has_true && has_false {
        return true;
    }

    false
}

/// Check if a pattern is a catch-all (always matches).
fn is_catch_all(pat: &HirPattern) -> bool {
    match pat {
        HirPattern::Wildcard | HirPattern::Var(_) => true,
        HirPattern::Or(alts) => alts.iter().any(is_catch_all),
        _ => false,
    }
}

/// Collect boolean literal coverage from a pattern, including or-pattern alternatives.
fn collect_bool_coverage(pat: &HirPattern, has_true: &mut bool, has_false: &mut bool) {
    match pat {
        HirPattern::Literal(PatternLiteral::Bool(b)) => {
            if *b {
                *has_true = true;
            } else {
                *has_false = true;
            }
        }
        HirPattern::Or(alts) => {
            for alt in alts {
                collect_bool_coverage(alt, has_true, has_false);
            }
        }
        _ => {}
    }
}

/// Validate that all alternatives in an or-pattern bind the same set of variables.
pub(crate) fn validate_or_pattern_bindings(
    alternatives: &[HirPattern],
    span: &crate::syntax::Span,
) -> Result<(), String> {
    if alternatives.len() < 2 {
        return Ok(());
    }
    let reference_names = alternatives[0].binding_names();
    for (i, alt) in alternatives.iter().enumerate().skip(1) {
        let alt_names = alt.binding_names();
        if alt_names != reference_names {
            return Err(format!(
                "{}: or-pattern alternative {} binds different variables than alternative 1",
                span,
                i + 1
            ));
        }
    }
    Ok(())
}
