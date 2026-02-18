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
#[derive(Debug, Clone, PartialEq)]
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
}

impl Closure {
    /// Get the effect of this closure
    pub fn effect(&self) -> Effect {
        self.effect
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
            effect: Effect::Pure,
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
        };
        assert_eq!(closure.effect(), Effect::Pure);
    }
}
