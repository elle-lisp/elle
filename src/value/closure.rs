//! Closure type for the Elle runtime
//!
//! `Closure` pairs a template with a captured environment and an optional
//! per-instance squelch mask. When non-zero, `squelch_mask` modifies the
//! effective signal: squelched bits are cleared and `SIG_ERROR` is added
//! (only when the closure could actually emit them). Use `effective_signal()`
//! externally; `template.signal` is the underlying code's signal.

use crate::error::LocationMap;
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::inline_slice::InlineSlice;
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
    pub capture_params_mask: u64,
    /// Bitmask indicating which locally-defined variables need box wrapping.
    /// Bit i set means locally-defined variable i needs a LocalLBox.
    pub capture_locals_mask: u64,
    /// Symbol ID → name mapping for cross-thread portability.
    pub symbol_names: Rc<HashMap<u32, String>>,
    /// Bytecode offset → source location mapping for error reporting.
    pub location_map: Rc<LocationMap>,
    /// True when pool rotation is safe during tail-call iteration.
    /// A rotation-safe function doesn't escape heap values to external
    /// mutable structures, so temporaries can be freed between iterations.
    pub rotation_safe: bool,
    /// LIR function for deferred JIT compilation.
    pub lir_function: Option<Rc<crate::lir::LirFunction>>,
    /// Module's closure list for JIT MakeClosure resolution.
    /// Optional docstring from the source lambda
    pub doc: Option<Value>,
    /// Original syntax node for eval environment reconstruction
    pub syntax: Option<Rc<crate::syntax::Syntax>>,
    /// How varargs are collected (List or Struct).
    /// Only meaningful when arity is AtLeast.
    pub vararg_kind: crate::hir::VarargKind,
    /// Optional name of this closure (for debugging/stack traces).
    pub name: Option<Rc<str>>,
    /// True when the body's final expression is provably not a heap pointer.
    /// Used by fiber resume to decide whether shared allocation is needed.
    pub result_is_immediate: bool,
    /// True when the body contains `set!` to a captured binding with a
    /// potentially heap-allocated value. Used by fiber resume.
    pub has_outward_heap_set: bool,
    /// WASM function table index (if compiled to WASM backend).
    /// When set, rt_call dispatches to this WASM function instead of bytecode.
    pub wasm_func_idx: Option<u32>,
    /// Cached SPIR-V bytes (write-once). Populated by `(git f)`.
    /// SPIR-V is a property of the code, not the instance — all closures
    /// from the same lambda share the template, so the cache is shared.
    pub spirv: std::cell::OnceCell<Vec<u8>>,
}

impl ClosureTemplate {
    /// True if signal and structural checks pass for GPU eligibility.
    ///
    /// This is a necessary but not sufficient condition — the full
    /// `LirFunction::is_gpu_eligible()` also walks instructions.
    /// Use this for cheap runtime queries on compiled closures.
    /// True if signal and structural checks pass for GPU eligibility.
    ///
    /// Allows SIG_ERROR (arithmetic type errors can't happen on unboxed GPU
    /// scalars) but rejects yield, I/O, FFI, and polymorphism.
    pub fn is_gpu_candidate(&self) -> bool {
        // Allow error-only signals (arithmetic ops on unboxed types can't type-error)
        let non_error_bits = self.signal.bits.subtract(crate::signals::SIG_ERROR);
        non_error_bits.is_empty()
            && self.signal.propagates == 0
            && self.num_captures == 0
            && matches!(self.arity, Arity::Exact(_))
            && self.capture_params_mask == 0
            && self.capture_locals_mask == 0
    }
}

/// Closure with captured environment
#[derive(Debug, Clone)]
pub struct Closure {
    /// Shared per-definition data
    pub template: Rc<ClosureTemplate>,
    /// Captured environment (upvalues). Arena-allocated inline slice; its
    /// lifetime matches the arena that allocated the enclosing Closure.
    pub env: InlineSlice<Value>,
    /// Per-instance squelch mask. Empty = no squelch; non-empty bits identify
    /// signals that are suppressed at the call boundary and converted to errors.
    pub squelch_mask: SignalBits,
}

impl Closure {
    /// Returns the effective signal of this closure, accounting for any squelch mask.
    /// When the squelch mask suppresses signals the closure may emit:
    /// - Suppressed bits are cleared from the result
    /// - SIG_ERROR is added (squelch converts suppressed signals to errors)
    ///
    /// When the mask doesn't suppress anything the closure emits, returns
    /// the template signal unchanged (no spurious SIG_ERROR added).
    pub fn effective_signal(&self) -> Signal {
        if self.squelch_mask.is_empty() {
            return self.template.signal;
        }
        let template_bits = self.template.signal.bits;
        let actually_squelched = template_bits.intersection(self.squelch_mask);
        if actually_squelched.is_empty() {
            // Mask doesn't suppress anything this closure actually emits.
            return self.template.signal;
        }
        // Clear squelched bits; add SIG_ERROR (squelch converts to error)
        let new_bits = template_bits
            .subtract(self.squelch_mask)
            .union(crate::signals::SIG_ERROR);
        Signal {
            bits: new_bits,
            propagates: self.template.signal.propagates,
        }
    }

    /// Returns the underlying template signal, accounting for any squelch mask.
    /// Prefer effective_signal() for external consumers.
    /// Use template.signal directly in JIT contexts where squelch must not
    /// affect code generation (the underlying bytecode still yields; squelch
    /// enforcement happens at the call boundary, not inside the JIT'd code).
    pub fn signal(&self) -> Signal {
        self.effective_signal()
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
            && self.template.capture_params_mask == other.template.capture_params_mask
            && self.template.capture_locals_mask == other.template.capture_locals_mask
            && self.template.symbol_names == other.template.symbol_names
            && self.template.location_map == other.template.location_map
            && self.template.doc == other.template.doc
            && self.template.vararg_kind == other.template.vararg_kind
            && self.template.num_params == other.template.num_params
            && self.template.name == other.template.name
            && self.squelch_mask == other.squelch_mask
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
            capture_params_mask: 0,
            capture_locals_mask: 0,

            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            rotation_safe: false,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
            result_is_immediate: false,
            has_outward_heap_set: false,
            wasm_func_idx: None,
            spirv: std::cell::OnceCell::new(),
        })
    }

    #[test]
    fn test_closure_signal() {
        let closure = Closure {
            template: make_template(),
            env: InlineSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        };
        assert_eq!(closure.signal(), Signal::silent());
        assert_eq!(closure.effective_signal(), Signal::silent());
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
            capture_params_mask: 0,
            capture_locals_mask: 0,

            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            rotation_safe: false,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
            result_is_immediate: false,
            has_outward_heap_set: false,
            wasm_func_idx: None,
            spirv: std::cell::OnceCell::new(),
        });
        let closure = Closure {
            template,
            env: crate::value::arena::alloc_inline_slice::<Value>(&[Value::NIL, Value::NIL]),
            squelch_mask: SignalBits::EMPTY,
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
            capture_params_mask: 0,
            capture_locals_mask: 0,

            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            rotation_safe: false,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
            result_is_immediate: false,
            has_outward_heap_set: false,
            wasm_func_idx: None,
            spirv: std::cell::OnceCell::new(),
        });
        let closure2 = Closure {
            template: template2,
            env: crate::value::arena::alloc_inline_slice::<Value>(&[Value::NIL]),
            squelch_mask: SignalBits::EMPTY,
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
            capture_params_mask: 0,
            capture_locals_mask: 0,

            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            rotation_safe: false,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
            result_is_immediate: false,
            has_outward_heap_set: false,
            wasm_func_idx: None,
            spirv: std::cell::OnceCell::new(),
        });
        let closure3 = Closure {
            template: template3,
            env: InlineSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        };
        assert_eq!(closure3.env_capacity(), 3);
    }

    #[test]
    fn test_effective_signal_no_squelch() {
        let closure = Closure {
            template: make_template(),
            env: InlineSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        };
        assert_eq!(closure.effective_signal(), Signal::silent());
    }

    #[test]
    fn test_effective_signal_squelch_clears_bits() {
        use crate::signals::SIG_YIELD;
        let template = Rc::new(ClosureTemplate {
            signal: Signal::yields(),
            ..(*make_template()).clone()
        });
        let closure = Closure {
            template,
            env: InlineSlice::empty(),
            squelch_mask: SIG_YIELD,
        };
        let eff = closure.effective_signal();
        assert_eq!(eff.bits, crate::signals::SIG_ERROR);
        assert_eq!(eff.propagates, 0);
    }

    #[test]
    fn test_effective_signal_squelch_no_effect_on_silent() {
        use crate::signals::SIG_YIELD;
        let closure = Closure {
            template: make_template(), // signal = silent()
            env: InlineSlice::empty(),
            squelch_mask: SIG_YIELD,
        };
        assert_eq!(closure.effective_signal(), Signal::silent());
    }

    #[test]
    fn test_effective_signal_partial_squelch() {
        use crate::signals::{SIG_ERROR, SIG_IO, SIG_YIELD};
        let template = Rc::new(ClosureTemplate {
            signal: Signal {
                bits: SIG_YIELD.union(SIG_IO),
                propagates: 0,
            },
            ..(*make_template()).clone()
        });
        let closure = Closure {
            template,
            env: InlineSlice::empty(),
            squelch_mask: SIG_YIELD, // only squelch yield
        };
        let eff = closure.effective_signal();
        // SIG_YIELD cleared, SIG_IO still set, SIG_ERROR added
        assert!(eff.bits.contains(SIG_ERROR), "SIG_ERROR should be set");
        assert!(!eff.bits.contains(SIG_YIELD), "SIG_YIELD should be cleared");
        assert!(eff.bits.contains(SIG_IO), "SIG_IO should remain set");
    }

    #[test]
    fn test_effective_signal_composable() {
        use crate::signals::{SIG_ERROR, SIG_IO, SIG_YIELD};
        let template = Rc::new(ClosureTemplate {
            signal: Signal {
                bits: SIG_YIELD.union(SIG_IO),
                propagates: 0,
            },
            ..(*make_template()).clone()
        });
        let closure1 = Closure {
            template: template.clone(),
            env: InlineSlice::empty(),
            squelch_mask: SIG_YIELD,
        };
        // Simulate composing a second squelch
        let closure2 = Closure {
            template: template.clone(),
            env: InlineSlice::empty(),
            squelch_mask: closure1.squelch_mask.union(SIG_IO),
        };
        let eff = closure2.effective_signal();
        assert!(eff.bits.contains(SIG_ERROR), "SIG_ERROR should be set");
        assert!(!eff.bits.contains(SIG_YIELD), "SIG_YIELD should be cleared");
        assert!(!eff.bits.contains(SIG_IO), "SIG_IO should be cleared");
    }
}
