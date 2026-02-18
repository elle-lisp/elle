//! Unified closure representation
//!
//! This module provides a single Closure type that handles both interpreted
//! (bytecode) and JIT-compiled closures. The design eliminates the need for
//! separate Closure/JitClosure variants in Value.

use std::rc::Rc;

use crate::compiler::ast::Expr;
use crate::effects::Effect;
use crate::value::Value;

// Re-export SymbolId for use in closure definitions
pub use crate::value_old::SymbolId;

/// Function arity specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Exact number of arguments required
    Exact(usize),
    /// At least this many arguments (rest parameter)
    AtLeast(usize),
    /// Between min and max arguments (inclusive)
    Range(usize, usize),
}

impl Arity {
    /// Check if a given argument count matches this arity.
    #[inline]
    pub fn matches(&self, n: usize) -> bool {
        match self {
            Arity::Exact(expected) => n == *expected,
            Arity::AtLeast(min) => n >= *min,
            Arity::Range(min, max) => n >= *min && n <= *max,
        }
    }

    /// Get the minimum number of arguments.
    #[inline]
    pub fn min_args(&self) -> usize {
        match self {
            Arity::Exact(n) | Arity::AtLeast(n) | Arity::Range(n, _) => *n,
        }
    }

    /// Get the maximum number of arguments, if bounded.
    #[inline]
    pub fn max_args(&self) -> Option<usize> {
        match self {
            Arity::Exact(n) | Arity::Range(_, n) => Some(*n),
            Arity::AtLeast(_) => None,
        }
    }
}

/// Source AST for deferred JIT compilation.
///
/// When a closure is created, we optionally store its source AST so that
/// the `jit-compile` primitive can compile it to native code later.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosureSource {
    /// Parameter symbols
    pub params: Vec<SymbolId>,
    /// Body expression (the AST)
    pub body: Box<Expr>,
    /// Captured variable symbols
    pub captures: Vec<SymbolId>,
    /// Effect annotation of the body
    pub effect: Effect,
}

/// Unified closure type for both interpreted and JIT-compiled functions.
///
/// A closure captures its lexical environment and can be executed either
/// via the bytecode interpreter or native code (if JIT-compiled).
#[derive(Clone)]
pub struct Closure {
    /// Bytecode for interpretation.
    /// None only if this is a JIT-only closure (rare in practice).
    pub bytecode: Option<Rc<Vec<u8>>>,

    /// Native code pointer for JIT-compiled execution.
    /// None if not yet JIT-compiled.
    /// Points to: fn(args: &[Value], env: &[Value]) -> Result<Value, String>
    pub jit_code: Option<*const u8>,

    /// Function arity specification.
    pub arity: Arity,

    /// Captured environment values.
    /// These are the values captured from the enclosing scope.
    pub env: Rc<Vec<Value>>,

    /// Number of local variable slots needed in the call frame.
    pub num_locals: usize,

    /// Bitmask indicating which env slots are Cells that auto-deref.
    /// Bit N = 1 means env\[N\] is a Cell created by the compiler for
    /// a mutable captured variable. The VM should auto-deref these.
    /// Bit N = 0 means env\[N\] is a direct value or a user-created Cell
    /// (via `box`) that should NOT be auto-derefed.
    pub cell_mask: u64,

    /// Constants pool for this closure's bytecode.
    pub constants: Rc<Vec<Value>>,

    /// Original source AST for deferred JIT compilation.
    pub source_ast: Option<Rc<ClosureSource>>,

    /// Effect annotation for this closure.
    pub effect: Effect,

    /// Unique ID for JIT cache management.
    /// Zero if not JIT-relevant.
    pub func_id: u64,
}

impl Closure {
    /// Create a new bytecode closure.
    pub fn new_bytecode(
        bytecode: Vec<u8>,
        arity: Arity,
        env: Vec<Value>,
        num_locals: usize,
        constants: Vec<Value>,
        effect: Effect,
    ) -> Self {
        Closure {
            bytecode: Some(Rc::new(bytecode)),
            jit_code: None,
            arity,
            env: Rc::new(env),
            num_locals,
            cell_mask: 0,
            constants: Rc::new(constants),
            source_ast: None,
            effect,
            func_id: 0,
        }
    }

    /// Create a closure with JIT compilation support.
    pub fn with_source(mut self, source: ClosureSource) -> Self {
        self.source_ast = Some(Rc::new(source));
        self
    }

    /// Set the cell mask for auto-deref behavior.
    pub fn with_cell_mask(mut self, mask: u64) -> Self {
        self.cell_mask = mask;
        self
    }

    /// Check if this closure has been JIT-compiled.
    #[inline]
    pub fn is_jit_compiled(&self) -> bool {
        self.jit_code.is_some()
    }

    /// Check if this closure can be JIT-compiled.
    #[inline]
    pub fn is_jit_compilable(&self) -> bool {
        self.source_ast.is_some()
    }

    /// Set the JIT code pointer after compilation.
    ///
    /// # Safety
    /// The code pointer must point to valid, properly-aligned native code
    /// with the expected signature.
    pub unsafe fn set_jit_code(&mut self, code: *const u8, func_id: u64) {
        self.jit_code = Some(code);
        self.func_id = func_id;
    }

    /// Get the effect annotation.
    #[inline]
    pub fn effect(&self) -> Effect {
        self.effect
    }

    /// Check if env\[index\] should be auto-derefed as a Cell.
    #[inline]
    pub fn is_cell_capture(&self, index: usize) -> bool {
        if index >= 64 {
            // cell_mask only tracks first 64 captures
            // Larger closures fall back to no auto-deref
            false
        } else {
            (self.cell_mask & (1u64 << index)) != 0
        }
    }
}

impl std::fmt::Debug for Closure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<closure")?;
        if self.is_jit_compiled() {
            write!(f, " jit={}", self.func_id)?;
        }
        write!(f, " arity={:?}", self.arity)?;
        write!(f, " env={}", self.env.len())?;
        write!(f, ">")
    }
}

impl PartialEq for Closure {
    fn eq(&self, _other: &Self) -> bool {
        // Closures are never equal (identity semantics)
        false
    }
}

// Note: Closure is not Send due to raw pointer.
// This is intentional - closures should not cross thread boundaries
// without proper marshaling.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arity_exact() {
        let a = Arity::Exact(2);
        assert!(a.matches(2));
        assert!(!a.matches(1));
        assert!(!a.matches(3));
        assert_eq!(a.min_args(), 2);
        assert_eq!(a.max_args(), Some(2));
    }

    #[test]
    fn test_arity_at_least() {
        let a = Arity::AtLeast(1);
        assert!(!a.matches(0));
        assert!(a.matches(1));
        assert!(a.matches(100));
        assert_eq!(a.min_args(), 1);
        assert_eq!(a.max_args(), None);
    }

    #[test]
    fn test_arity_range() {
        let a = Arity::Range(1, 3);
        assert!(!a.matches(0));
        assert!(a.matches(1));
        assert!(a.matches(2));
        assert!(a.matches(3));
        assert!(!a.matches(4));
    }

    #[test]
    fn test_cell_mask() {
        let c = Closure::new_bytecode(vec![], Arity::Exact(0), vec![], 0, vec![], Effect::Pure)
            .with_cell_mask(0b101); // indices 0 and 2 are cells

        assert!(c.is_cell_capture(0));
        assert!(!c.is_cell_capture(1));
        assert!(c.is_cell_capture(2));
        assert!(!c.is_cell_capture(63));
        assert!(!c.is_cell_capture(64)); // Out of range
    }

    #[test]
    fn test_closure_not_equal() {
        let c1 = Closure::new_bytecode(vec![], Arity::Exact(0), vec![], 0, vec![], Effect::Pure);
        let c2 = Closure::new_bytecode(vec![], Arity::Exact(0), vec![], 0, vec![], Effect::Pure);
        assert_ne!(c1, c2); // Closures are never equal
    }
}
