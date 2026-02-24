//! Pattern matching in HIR

use super::binding::BindingId;

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
    Var(BindingId),

    /// Match cons cell and destructure
    Cons {
        head: Box<HirPattern>,
        tail: Box<HirPattern>,
    },

    /// Match a list of specific length
    List(Vec<HirPattern>),

    /// Match an array of specific length
    Array(Vec<HirPattern>),
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
    pub bindings: Vec<BindingId>,
}

impl PatternBindings {
    pub fn new() -> Self {
        PatternBindings {
            bindings: Vec::new(),
        }
    }

    pub fn add(&mut self, id: BindingId) {
        self.bindings.push(id);
    }

    pub fn extend(&mut self, other: &PatternBindings) {
        self.bindings.extend(other.bindings.iter().cloned());
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
            HirPattern::Var(id) => out.add(*id),
            HirPattern::Cons { head, tail } => {
                head.collect_bindings(out);
                tail.collect_bindings(out);
            }
            HirPattern::List(patterns) | HirPattern::Array(patterns) => {
                for p in patterns {
                    p.collect_bindings(out);
                }
            }
            HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        }
    }
}
