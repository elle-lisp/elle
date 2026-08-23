//! Pattern matching in HIR

use super::binding::Binding;
use crate::hir::arena::BindingArena;
use crate::value::SymbolId;

/// HIR pattern for match expressions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HirPattern {
    /// Match anything, don't bind
    Wildcard,

    /// Match nil
    Nil,

    /// Match a literal value
    Literal(PatternLiteral),

    /// Bind to a variable
    Var(Binding),

    /// Match a pair (head . tail) and destructure
    Pair {
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
            HirPattern::Pair { head, tail } => {
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
    pub fn binding_names(&self, arena: &BindingArena) -> std::collections::BTreeSet<SymbolId> {
        let mut names = std::collections::BTreeSet::new();
        self.collect_binding_names(&mut names, arena);
        names
    }

    fn collect_binding_names(
        &self,
        out: &mut std::collections::BTreeSet<SymbolId>,
        arena: &BindingArena,
    ) {
        match self {
            HirPattern::Var(binding) => {
                out.insert(arena.get(*binding).name);
            }
            HirPattern::Pair { head, tail } => {
                head.collect_binding_names(out, arena);
                tail.collect_binding_names(out, arena);
            }
            HirPattern::List { elements, rest }
            | HirPattern::Tuple { elements, rest }
            | HirPattern::Array { elements, rest } => {
                for p in elements {
                    p.collect_binding_names(out, arena);
                }
                if let Some(r) = rest {
                    r.collect_binding_names(out, arena);
                }
            }
            HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
                for (_, pattern) in entries {
                    pattern.collect_binding_names(out, arena);
                }
                if let Some(r) = rest {
                    r.collect_binding_names(out, arena);
                }
            }
            HirPattern::NamedStruct { entries } => {
                for (_, pattern) in entries {
                    pattern.collect_binding_names(out, arena);
                }
            }
            HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
                binding.collect_binding_names(out, arena);
            }
            HirPattern::Or(alts) => {
                if let Some(first) = alts.first() {
                    first.collect_binding_names(out, arena);
                }
            }
            HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        }
    }

    /// Does matching this pattern allocate at the destructure site?
    ///
    /// Used by ANF (`Hir::allocates` via `Match`) to decide whether
    /// a `Match` expression's value position must be named. The
    /// criterion is operational: which rest-binding lowerings emit
    /// a fresh heap object?
    ///
    /// - `Array { rest: Some(_) }`, `Tuple { rest: Some(_) }`: lower
    ///   to `ArrayMutSliceFrom`, which allocates a fresh array for
    ///   the slice.
    /// - `Struct { rest: Some(_) }`, `Table { rest: Some(_) }`: lower
    ///   to `StructRest`, which allocates a fresh struct from the
    ///   non-excluded keys.
    /// - `List { rest: Some(_) }`: the rest binding is just the
    ///   remaining cons tail pointer — no allocation.
    /// - All others recurse into sub-patterns.
    pub fn allocates(&self) -> bool {
        match self {
            HirPattern::Array { rest: Some(_), .. }
            | HirPattern::Tuple { rest: Some(_), .. }
            | HirPattern::Struct { rest: Some(_), .. }
            | HirPattern::Table { rest: Some(_), .. } => true,
            HirPattern::Pair { head, tail } => head.allocates() || tail.allocates(),
            HirPattern::List { elements, rest }
            | HirPattern::Tuple { elements, rest }
            | HirPattern::Array { elements, rest } => {
                elements.iter().any(HirPattern::allocates)
                    || rest.as_deref().is_some_and(HirPattern::allocates)
            }
            HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
                entries.iter().any(|(_, p)| p.allocates())
                    || rest.as_deref().is_some_and(HirPattern::allocates)
            }
            HirPattern::NamedStruct { entries } => entries.iter().any(|(_, p)| p.allocates()),
            HirPattern::Set { binding } | HirPattern::SetMut { binding } => binding.allocates(),
            HirPattern::Or(alts) => alts.iter().any(HirPattern::allocates),
            HirPattern::Wildcard
            | HirPattern::Nil
            | HirPattern::Literal(_)
            | HirPattern::Var(_) => false,
        }
    }

    /// True when this pattern matches every value: a wildcard, a bare
    /// variable, or an or-pattern with an irrefutable alternative.
    /// A match with a guardless irrefutable arm cannot raise
    /// `:match-error`; signal inference keys on this.
    pub fn is_irrefutable(&self) -> bool {
        match self {
            HirPattern::Wildcard | HirPattern::Var(_) => true,
            HirPattern::Or(alts) => alts.iter().any(HirPattern::is_irrefutable),
            _ => false,
        }
    }
}

/// Validate that all alternatives in an or-pattern bind the same set of variables.
pub(crate) fn validate_or_pattern_bindings(
    alternatives: &[HirPattern],
    span: &crate::syntax::Span,
    arena: &BindingArena,
) -> Result<(), String> {
    if alternatives.len() < 2 {
        return Ok(());
    }
    let reference_names = alternatives[0].binding_names(arena);
    for (i, alt) in alternatives.iter().enumerate().skip(1) {
        let alt_names = alt.binding_names(arena);
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

#[cfg(test)]
mod allocates_tests;
