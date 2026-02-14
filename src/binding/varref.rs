//! Variable reference types for lexical scope resolution
//!
//! VarRef represents a fully-resolved variable reference where all
//! scope resolution has been done at compile time.

use crate::value::SymbolId;

/// A resolved variable reference - all resolution happens at compile time
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarRef {
    /// Local variable in current activation frame (inside a lambda)
    /// index is offset in frame's locals array: [params..., locals...]
    Local { index: usize },

    /// Let-bound variable (outside a lambda, in a let/block scope)
    /// sym is used for runtime lookup in the scope stack
    LetBound { sym: SymbolId },

    /// Captured variable from enclosing closure
    /// sym is the original symbol (used during index adjustment)
    /// index is offset in closure's captures array (set during adjust_var_indices)
    Upvalue {
        sym: SymbolId,
        index: usize,
        is_param: bool,
    },

    /// Global/top-level binding
    /// sym is used for runtime lookup in globals HashMap
    Global { sym: SymbolId },
}

/// Extended VarRef that includes cell-boxing information
/// Used during compilation to determine which instructions to emit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedVar {
    /// The base variable reference
    pub var: VarRef,
    /// Whether this variable is boxed in a cell (for mutable captures)
    pub boxed: bool,
}

impl VarRef {
    /// Create a local variable reference (inside lambda)
    pub fn local(index: usize) -> Self {
        VarRef::Local { index }
    }

    /// Create a let-bound variable reference (outside lambda)
    pub fn let_bound(sym: SymbolId) -> Self {
        VarRef::LetBound { sym }
    }

    /// Create an upvalue (captured) variable reference
    /// The index is a placeholder that will be adjusted later during capture resolution
    pub fn upvalue(sym: SymbolId, index: usize, is_param: bool) -> Self {
        VarRef::Upvalue {
            sym,
            index,
            is_param,
        }
    }

    /// Create a global variable reference
    pub fn global(sym: SymbolId) -> Self {
        VarRef::Global { sym }
    }

    /// Check if this is a local variable
    pub fn is_local(&self) -> bool {
        matches!(self, VarRef::Local { .. })
    }

    /// Check if this is a let-bound variable
    pub fn is_let_bound(&self) -> bool {
        matches!(self, VarRef::LetBound { .. })
    }

    /// Check if this is an upvalue (captured variable)
    pub fn is_upvalue(&self) -> bool {
        matches!(self, VarRef::Upvalue { .. })
    }

    /// Check if this is a global variable
    pub fn is_global(&self) -> bool {
        matches!(self, VarRef::Global { .. })
    }
}

impl ResolvedVar {
    /// Create a resolved variable (unboxed)
    pub fn new(var: VarRef) -> Self {
        ResolvedVar { var, boxed: false }
    }

    /// Create a resolved variable with boxing
    pub fn boxed(var: VarRef) -> Self {
        ResolvedVar { var, boxed: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varref_local() {
        let v = VarRef::local(5);
        assert!(v.is_local());
        assert!(!v.is_upvalue());
        assert!(!v.is_global());
        assert_eq!(v, VarRef::Local { index: 5 });
    }

    #[test]
    fn test_varref_upvalue() {
        let sym = SymbolId(5);
        let v = VarRef::upvalue(sym, 3, false);
        assert!(!v.is_local());
        assert!(v.is_upvalue());
        assert!(!v.is_global());
        assert_eq!(
            v,
            VarRef::Upvalue {
                sym,
                index: 3,
                is_param: false
            }
        );
    }

    #[test]
    fn test_varref_global() {
        let sym = SymbolId(42);
        let v = VarRef::global(sym);
        assert!(!v.is_local());
        assert!(!v.is_upvalue());
        assert!(v.is_global());
        assert_eq!(v, VarRef::Global { sym });
    }

    #[test]
    fn test_resolved_var() {
        let local = VarRef::local(0);
        let resolved = ResolvedVar::new(local);
        assert!(!resolved.boxed);

        let boxed_resolved = ResolvedVar::boxed(local);
        assert!(boxed_resolved.boxed);
    }
}
