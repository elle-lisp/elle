//! Closure type for the Elle runtime
//!
//! A closure is split into a shared `ClosureTemplate` (compile-time data)
//! and a per-instance `Closure` (template + captured environment).

use crate::effects::Effect;
use crate::error::LocationMap;
use crate::value::types::Arity;
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// Compile-time closure data shared across all instances of the same lambda.
///
/// Contains everything except the captured environment: bytecode, constants,
/// arity, effect, metadata, and compilation artifacts. Shared via `Rc` —
/// `MakeClosure` at runtime clones one `Rc<ClosureTemplate>` instead of
/// copying 15 fields.
#[derive(Debug, Clone)]
pub struct ClosureTemplate {
    /// Compiled bytecode for this closure
    pub bytecode: Rc<Vec<u8>>,
    /// Function arity specification
    pub arity: Arity,
    /// Total number of local slots needed
    pub num_locals: usize,
    /// Number of captured variables (for env layout)
    pub num_captures: usize,
    /// Total number of parameter slots (required + optional + rest if present).
    pub num_params: usize,
    /// Constant pool for this closure
    pub constants: Rc<Vec<Value>>,
    /// Effect of the closure body
    pub effect: Effect,
    /// Bitmask indicating which parameters need cell wrapping.
    /// Bit i set means parameter i is mutated and needs a LocalCell.
    pub cell_params_mask: u64,
    /// Bitmask indicating which locally-defined variables need cell wrapping.
    /// Bit i set means locally-defined variable i needs a LocalCell.
    /// Used by the emitter to decide StoreLocal vs StoreUpvalue.
    pub cell_locals_mask: u64,
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
    /// Optional name of this closure (for debugging/stack traces).
    /// Set from LirFunction.name during emission.
    pub name: Option<Rc<str>>,
}

/// Closure with captured environment.
///
/// Two fields: a shared template (compile-time data) and a per-instance
/// environment (captured upvalues).
#[derive(Debug, Clone)]
pub struct Closure {
    /// Shared compile-time data
    pub template: Rc<ClosureTemplate>,
    /// Captured environment (upvalues)
    pub env: Rc<Vec<Value>>,
}

impl Closure {
    /// Get the effect of this closure
    pub fn effect(&self) -> Effect {
        self.template.effect
    }

    /// Calculate the total environment capacity needed for a call.
    /// This is: existing captures + parameters + locally-defined variables.
    /// For variadic functions (AtLeast), the rest slot is an extra parameter.
    pub fn env_capacity(&self) -> usize {
        let num_locally_defined = self
            .template
            .num_locals
            .saturating_sub(self.template.num_params);
        self.env.len() + self.template.num_params + num_locally_defined
    }
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        self.template.bytecode == other.template.bytecode
            && self.template.arity == other.template.arity
            && self.env == other.env
            && self.template.num_locals == other.template.num_locals
            && self.template.num_captures == other.template.num_captures
            && self.template.constants == other.template.constants
            && self.template.effect == other.template.effect
            && self.template.cell_params_mask == other.template.cell_params_mask
            && self.template.cell_locals_mask == other.template.cell_locals_mask
            && self.template.symbol_names == other.template.symbol_names
            && self.template.location_map == other.template.location_map
            && self.template.doc == other.template.doc
            && self.template.vararg_kind == other.template.vararg_kind
            && self.template.num_params == other.template.num_params
            && self.template.name == other.template.name
        // Note: jit_code and lir_function are not compared
        // as they are derived/cached data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_template() -> Rc<ClosureTemplate> {
        Rc::new(ClosureTemplate {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            cell_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        })
    }

    #[test]
    fn test_closure_effect() {
        let closure = Closure {
            template: test_template(),
            env: Rc::new(vec![]),
        };
        assert_eq!(closure.effect(), Effect::none());
    }

    #[test]
    fn test_closure_env_capacity() {
        // Closure with 2 captures, 3 params, 5 total locals (so 2 locally-defined)
        let closure = Closure {
            template: Rc::new(ClosureTemplate {
                bytecode: Rc::new(vec![]),
                arity: Arity::Exact(3),
                num_locals: 5, // 3 params + 2 locally-defined
                num_captures: 2,
                num_params: 3,
                constants: Rc::new(vec![]),
                effect: Effect::none(),
                cell_params_mask: 0,
                cell_locals_mask: 0,
                symbol_names: Rc::new(HashMap::new()),
                location_map: Rc::new(LocationMap::new()),
                jit_code: None,
                lir_function: None,
                doc: None,
                vararg_kind: crate::hir::VarargKind::List,
                name: None,
            }),
            env: Rc::new(vec![Value::NIL, Value::NIL]), // 2 captures
        };
        // env_capacity = 2 (captures) + 3 (params) + 2 (locally-defined) = 7
        assert_eq!(closure.env_capacity(), 7);

        // Closure with AtLeast arity
        let closure_variadic = Closure {
            template: Rc::new(ClosureTemplate {
                bytecode: Rc::new(vec![]),
                arity: Arity::AtLeast(2),
                num_locals: 4, // 3 param slots (2 fixed + 1 rest) + 1 locally-defined
                num_captures: 1,
                num_params: 3,
                constants: Rc::new(vec![]),
                effect: Effect::none(),
                cell_params_mask: 0,
                cell_locals_mask: 0,
                symbol_names: Rc::new(HashMap::new()),
                location_map: Rc::new(LocationMap::new()),
                jit_code: None,
                lir_function: None,
                doc: None,
                vararg_kind: crate::hir::VarargKind::List,
                name: None,
            }),
            env: Rc::new(vec![Value::NIL]), // 1 capture
        };
        // env_capacity = 1 (captures) + 3 (param slots) + 1 (locally-defined) = 5
        assert_eq!(closure_variadic.env_capacity(), 5);

        // Closure with Range arity
        let closure_range = Closure {
            template: Rc::new(ClosureTemplate {
                bytecode: Rc::new(vec![]),
                arity: Arity::Range(1, 3),
                num_locals: 3, // 3 params (1 required + 2 optional)
                num_captures: 0,
                num_params: 3,
                constants: Rc::new(vec![]),
                effect: Effect::none(),
                cell_params_mask: 0,
                cell_locals_mask: 0,
                symbol_names: Rc::new(HashMap::new()),
                location_map: Rc::new(LocationMap::new()),
                jit_code: None,
                lir_function: None,
                doc: None,
                vararg_kind: crate::hir::VarargKind::List,
                name: None,
            }),
            env: Rc::new(vec![]), // 0 captures
        };
        // env_capacity = 0 (captures) + 3 (params) + 0 (locally-defined) = 3
        assert_eq!(closure_range.env_capacity(), 3);
    }
}
