//! Pattern matching in HIR

use super::binding::Binding;

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

    /// Match an array pattern with optional rest
    Array {
        elements: Vec<HirPattern>,
        rest: Option<Box<HirPattern>>,
    },

    /// Match a table/struct by keyword keys
    /// Each entry is (keyword_name, pattern_for_value)
    Table { entries: Vec<(String, HirPattern)> },
}

/// Literal values that can appear in patterns
#[derive(Debug, Clone)]
pub enum PatternLiteral {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Keyword(String),
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
            HirPattern::List { elements, rest } | HirPattern::Array { elements, rest } => {
                for p in elements {
                    p.collect_bindings(out);
                }
                if let Some(r) = rest {
                    r.collect_bindings(out);
                }
            }
            HirPattern::Table { entries } => {
                for (_, pattern) in entries {
                    pattern.collect_bindings(out);
                }
            }
            HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        }
    }
}
