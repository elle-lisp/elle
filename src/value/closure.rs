//! Closure type for the Elle runtime
//!
//! A closure captures its environment and bytecode for later execution.

use crate::effects::Effect;
use crate::error::LocationMap;
use crate::value::types::Arity;
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// Closure with captured environment
#[derive(Debug, Clone)]
pub struct Closure {
    /// Compiled bytecode for this closure
    pub bytecode: Rc<Vec<u8>>,
    /// Function arity specification
    pub arity: Arity,
    /// Captured environment (upvalues)
    pub env: Rc<Vec<Value>>,
    /// Total number of local slots needed
    pub num_locals: usize,
    /// Number of captured variables (for env layout)
    pub num_captures: usize,
    /// Constant pool for this closure
    pub constants: Rc<Vec<Value>>,
    /// Effect of the closure body
    pub effect: Effect,
    /// Bitmask indicating which parameters need cell wrapping.
    /// Bit i set means parameter i is mutated and needs a LocalCell.
    pub cell_params_mask: u64,
    /// Symbol ID → name mapping for cross-thread portability.
    /// When bytecode is sent to a new thread, symbol IDs may differ.
    /// This map allows remapping globals to the correct IDs.
    pub symbol_names: Rc<HashMap<u32, String>>,
    /// Bytecode offset → source location mapping for error reporting.
    pub location_map: Rc<LocationMap>,
    /// JIT-compiled native code for this closure (if available).
    /// Stored separately from bytecode to allow lazy JIT compilation.
    pub jit_code: Option<Rc<crate::jit::JitCode>>,
    /// LIR function for deferred JIT compilation.
    /// Preserved from emission so the JIT can compile hot functions.
    pub lir_function: Option<Rc<crate::lir::LirFunction>>,
    /// Optional docstring from the source lambda
    pub doc: Option<Value>,
    /// How varargs are collected (List or Struct).
    /// Only meaningful when arity is AtLeast.
    pub vararg_kind: crate::hir::VarargKind,
    /// Total number of parameter slots (required + optional + rest if present).
    pub num_params: usize,
}

impl Closure {
    /// Get the effect of this closure
    pub fn effect(&self) -> Effect {
        self.effect
    }

    /// Calculate the total environment capacity needed for a call.
    /// This is: existing captures + parameters + locally-defined variables.
    /// For variadic functions (AtLeast), the rest slot is an extra parameter.
    pub fn env_capacity(&self) -> usize {
        let num_locally_defined = self.num_locals.saturating_sub(self.num_params);
        self.env.len() + self.num_params + num_locally_defined
    }
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        self.bytecode == other.bytecode
            && self.arity == other.arity
            && self.env == other.env
            && self.num_locals == other.num_locals
            && self.num_captures == other.num_captures
            && self.constants == other.constants
            && self.effect == other.effect
            && self.cell_params_mask == other.cell_params_mask
            && self.symbol_names == other.symbol_names
            && self.location_map == other.location_map
            && self.doc == other.doc
            && self.vararg_kind == other.vararg_kind
            && self.num_params == other.num_params
        // Note: jit_code and lir_function are not compared
        // as they are derived/cached data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closure_effect() {
        let closure = Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            vararg_kind: crate::hir::VarargKind::List,
            num_params: 0,
        };
        assert_eq!(closure.effect(), Effect::none());
    }

    #[test]
    fn test_closure_env_capacity() {
        // Closure with 2 captures, 3 params, 5 total locals (so 2 locally-defined)
        let closure = Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(3),
            env: Rc::new(vec![Value::NIL, Value::NIL]), // 2 captures
            num_locals: 5,                              // 3 params + 2 locally-defined
            num_captures: 2,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            vararg_kind: crate::hir::VarargKind::List,
            num_params: 3,
        };
        // env_capacity = 2 (captures) + 3 (params) + 2 (locally-defined) = 7
        assert_eq!(closure.env_capacity(), 7);

        // Closure with AtLeast arity
        let closure_variadic = Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::AtLeast(2),
            env: Rc::new(vec![Value::NIL]), // 1 capture
            num_locals: 4,                  // 3 param slots (2 fixed + 1 rest) + 1 locally-defined
            num_captures: 1,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            vararg_kind: crate::hir::VarargKind::List,
            num_params: 3,
        };
        // env_capacity = 1 (captures) + 3 (param slots) + 1 (locally-defined) = 5
        assert_eq!(closure_variadic.env_capacity(), 5);

        // Closure with Range arity
        let closure_range = Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Range(1, 3),
            env: Rc::new(vec![]), // 0 captures
            num_locals: 3,        // 3 params (1 required + 2 optional)
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            vararg_kind: crate::hir::VarargKind::List,
            num_params: 3,
        };
        // env_capacity = 0 (captures) + 3 (params) + 0 (locally-defined) = 3
        assert_eq!(closure_range.env_capacity(), 3);
    }
}
