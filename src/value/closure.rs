//! Closure type for the Elle runtime
//!
//! A closure captures its environment and bytecode for later execution.
//! `ClosureTemplate` holds per-definition data (shared across all instances
//! of the same lambda). `Closure` pairs a template with a captured environment.

use crate::error::LocationMap;
use crate::signals::Signal;
use crate::value::types::Arity;
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// Per-definition closure data shared across all instances of the same lambda.
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
    /// Signal of the closure body
    pub signal: Signal,
    /// Bitmask indicating which parameters need box wrapping.
    /// Bit i set means parameter i is mutated and needs a LocalLBox.
    pub lbox_params_mask: u64,
    /// Bitmask indicating which locally-defined variables need box wrapping.
    /// Bit i set means locally-defined variable i needs a LocalLBox.
    pub lbox_locals_mask: u64,
    /// Symbol ID → name mapping for cross-thread portability.
    pub symbol_names: Rc<HashMap<u32, String>>,
    /// Bytecode offset → source location mapping for error reporting.
    pub location_map: Rc<LocationMap>,
    /// JIT-compiled native code for this closure (if available).
    pub jit_code: Option<Rc<crate::jit::JitCode>>,
    /// LIR function for deferred JIT compilation.
    pub lir_function: Option<Rc<crate::lir::LirFunction>>,
    /// Optional docstring from the source lambda
    pub doc: Option<Value>,
    /// Original syntax node for eval environment reconstruction
    pub syntax: Option<Rc<crate::syntax::Syntax>>,
    /// How varargs are collected (List or Struct).
    /// Only meaningful when arity is AtLeast.
    pub vararg_kind: crate::hir::VarargKind,
    /// Optional name of this closure (for debugging/stack traces).
    pub name: Option<Rc<str>>,
}

/// Closure with captured environment
#[derive(Debug, Clone)]
pub struct Closure {
    /// Shared per-definition data
    pub template: Rc<ClosureTemplate>,
    /// Captured environment (upvalues)
    pub env: Rc<Vec<Value>>,
}

impl Closure {
    /// Get the signal of this closure
    pub fn signal(&self) -> Signal {
        self.template.signal
    }

    /// Calculate the total environment capacity needed for a call.
    /// This is: existing captures + parameters + locally-defined variables.
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
            && self.template.signal == other.template.signal
            && self.template.lbox_params_mask == other.template.lbox_params_mask
            && self.template.lbox_locals_mask == other.template.lbox_locals_mask
            && self.template.symbol_names == other.template.symbol_names
            && self.template.location_map == other.template.location_map
            && self.template.doc == other.template.doc
            && self.template.vararg_kind == other.template.vararg_kind
            && self.template.num_params == other.template.num_params
            && self.template.name == other.template.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_template() -> Rc<ClosureTemplate> {
        Rc::new(ClosureTemplate {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants: Rc::new(vec![]),
            signal: Signal::silent(),
            lbox_params_mask: 0,
            lbox_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        })
    }

    #[test]
    fn test_closure_signal() {
        let closure = Closure {
            template: make_template(),
            env: Rc::new(vec![]),
        };
        assert_eq!(closure.signal(), Signal::silent());
    }

    #[test]
    fn test_closure_env_capacity() {
        let template = Rc::new(ClosureTemplate {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(3),
            num_locals: 5,
            num_captures: 2,
            num_params: 3,
            constants: Rc::new(vec![]),
            signal: Signal::silent(),
            lbox_params_mask: 0,
            lbox_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        });
        let closure = Closure {
            template,
            env: Rc::new(vec![Value::NIL, Value::NIL]),
        };
        assert_eq!(closure.env_capacity(), 7);

        let template2 = Rc::new(ClosureTemplate {
            bytecode: Rc::new(vec![]),
            arity: Arity::AtLeast(2),
            num_locals: 4,
            num_captures: 1,
            num_params: 3,
            constants: Rc::new(vec![]),
            signal: Signal::silent(),
            lbox_params_mask: 0,
            lbox_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        });
        let closure2 = Closure {
            template: template2,
            env: Rc::new(vec![Value::NIL]),
        };
        assert_eq!(closure2.env_capacity(), 5);

        let template3 = Rc::new(ClosureTemplate {
            bytecode: Rc::new(vec![]),
            arity: Arity::Range(1, 3),
            num_locals: 3,
            num_captures: 0,
            num_params: 3,
            constants: Rc::new(vec![]),
            signal: Signal::silent(),
            lbox_params_mask: 0,
            lbox_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        });
        let closure3 = Closure {
            template: template3,
            env: Rc::new(vec![]),
        };
        assert_eq!(closure3.env_capacity(), 3);
    }
}
