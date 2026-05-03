//! Fiber lifecycle primitives.
//!
//! Core fiber operations: creation, resumption, signaling, status, and
//! value extraction. Introspection and management primitives (bits, mask,
//! parent, child, propagate, cancel, fiber?) are in `fiber_introspect.rs`.

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{
    Fiber, FiberStatus, SignalBits, SIG_ERROR, SIG_OK, SIG_RESUME, SIG_YIELD,
};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Return a keyword Value for a FiberStatus.
fn status_keyword(status: FiberStatus) -> Value {
    Value::keyword(status.as_str())
}

/// Resolve a Value to SignalBits.
///
/// Accepts three forms:
/// - Integer: passthrough as `SignalBits(value as u32)`
/// - Keyword: lookup in global registry, return `SignalBits(1 << bit_position)`
/// - Set of keywords: iterate elements, look up each, OR the bits together
///
/// `context` is used in error messages (e.g., "fiber/new", "fiber/signal").
/// Resolve a slice of Values (from array) to SignalBits by OR-ing keyword bits.
fn resolve_keyword_slice(
    elems: &[Value],
    context: &str,
) -> Result<SignalBits, (SignalBits, Value)> {
    crate::signals::registry::with_registry(|reg| {
        let mut bits = SignalBits::EMPTY;
        for elem in elems {
            let name = elem.as_keyword_name().ok_or_else(|| {
                (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: array elements must be keywords, got {}",
                            context,
                            elem.type_name()
                        ),
                    ),
                )
            })?;
            let b = reg.to_signal_bits(&name).ok_or_else(|| {
                (
                    SIG_ERROR,
                    error_val(
                        "signal-error",
                        format!("{}: unknown signal keyword :{}", context, name),
                    ),
                )
            })?;
            bits = bits.union(b);
        }
        Ok(bits)
    })
}

pub(crate) fn resolve_signal_bits(
    val: &Value,
    context: &str,
) -> Result<SignalBits, (SignalBits, Value)> {
    // 1. Integer passthrough (existing behavior)
    if let Some(i) = val.as_int() {
        return Ok(SignalBits::from_i64(i));
    }

    // 2. Single keyword
    if let Some(name) = val.as_keyword_name() {
        return crate::signals::registry::with_registry(|reg| match reg.to_signal_bits(&name) {
            Some(bits) => Ok(bits),
            None => Err((
                SIG_ERROR,
                error_val(
                    "signal-error",
                    format!("{}: unknown signal keyword :{}", context, name),
                ),
            )),
        });
    }

    // 3. Set of keywords
    if let Some(set) = val.as_set() {
        return crate::signals::registry::with_registry(|reg| {
            let mut bits = SignalBits::EMPTY;
            for elem in set.iter() {
                let name = elem.as_keyword_name().ok_or_else(|| {
                    (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: set elements must be keywords, got {}",
                                context,
                                elem.type_name()
                            ),
                        ),
                    )
                })?;
                let b = reg.to_signal_bits(&name).ok_or_else(|| {
                    (
                        SIG_ERROR,
                        error_val(
                            "signal-error",
                            format!("{}: unknown signal keyword :{}", context, name),
                        ),
                    )
                })?;
                bits = bits.union(b);
            }
            Ok(bits)
        });
    }

    // 4. Array of keywords (immutable [...])
    if let Some(elems) = val.as_array() {
        return resolve_keyword_slice(elems, context);
    }

    // 5. Mutable array of keywords (@[...])
    if let Some(arr) = val.as_array_mut() {
        let elems = arr.borrow();
        return resolve_keyword_slice(&elems, context);
    }

    // 6. List of keywords (pair chain)
    if val.as_pair().is_some() {
        return crate::signals::registry::with_registry(|reg| {
            let mut bits = SignalBits::EMPTY;
            let mut current = *val;
            while let Some(pair) = current.as_pair() {
                let name = pair.first.as_keyword_name().ok_or_else(|| {
                    (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: list elements must be keywords, got {}",
                                context,
                                pair.first.type_name()
                            ),
                        ),
                    )
                })?;
                let b = reg.to_signal_bits(&name).ok_or_else(|| {
                    (
                        SIG_ERROR,
                        error_val(
                            "signal-error",
                            format!("{}: unknown signal keyword :{}", context, name),
                        ),
                    )
                })?;
                bits = bits.union(b);
                current = pair.rest;
            }
            Ok(bits)
        });
    }

    // 7. None of the above
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected integer, keyword, or collection of keywords, got {}",
                context,
                val.type_name()
            ),
        ),
    ))
}

/// (fiber/new fn mask [:deny bits]) → fiber
///
/// Create a fiber from a closure and a signal mask. The mask determines
/// which signals the parent catches when resuming this fiber.
///
/// Optional `:deny` keyword arg withholds capabilities from the fiber.
/// The child's `withheld` is the union of the explicit deny bits and the
/// parent's withheld (propagated at resume time by the VM).
pub(crate) fn prim_fiber_new(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "fiber/new: expected at least 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }

    let closure = match args[0].as_closure() {
        Some(c) => std::rc::Rc::new(c.clone()),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/new: expected closure, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let mask = match resolve_signal_bits(&args[1], "fiber/new") {
        Ok(bits) => bits,
        Err(err) => return err,
    };

    // Parse optional keyword arguments after the required (closure, mask) pair.
    let mut deny_bits = SignalBits::EMPTY;
    let mut i = 2;
    while i < args.len() {
        if args[i].as_keyword_name().as_deref() == Some("deny") {
            if i + 1 >= args.len() {
                return (
                    SIG_ERROR,
                    error_val("arity-error", "fiber/new: :deny requires a value"),
                );
            }
            deny_bits = match resolve_signal_bits(&args[i + 1], "fiber/new :deny") {
                Ok(bits) => bits,
                Err(err) => return err,
            };
            i += 2;
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "argument-error",
                    format!(
                        "fiber/new: unexpected keyword argument :{}",
                        args[i]
                            .as_keyword_name()
                            .unwrap_or_else(|| args[i].type_name().to_string())
                    ),
                ),
            );
        }
    }

    let mut fiber = Fiber::new(closure, mask);
    fiber.withheld = deny_bits;
    (SIG_OK, Value::fiber(fiber))
}

/// (fiber/resume fiber) → value
/// (fiber/resume fiber value) → value
///
/// Resume a fiber. If the fiber is New, starts execution. If Suspended,
/// delivers the value and continues from where it left off.
///
/// Returns SIG_RESUME — the VM handles the actual fiber swap.
pub(crate) fn prim_fiber_resume(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/resume: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/resume: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let resume_value = args.get(1).copied().unwrap_or(Value::NIL);

    // Validate fiber status and store resume value.
    // Error'd fibers are resumable — this is the restarts system.
    // Only Dead fibers are terminal.
    let status_err = handle.with_mut(|fiber| match fiber.status {
        FiberStatus::New | FiberStatus::Paused | FiberStatus::Error => {
            fiber.signal = Some((SIG_OK, resume_value));
            None
        }
        FiberStatus::Alive => Some(error_val(
            "state-error",
            "fiber/resume: fiber is already running",
        )),
        FiberStatus::Dead => Some(error_val(
            "state-error",
            "fiber/resume: cannot resume completed fiber",
        )),
    });

    if let Some(err) = status_err {
        return (SIG_ERROR, err);
    }

    // Return SIG_RESUME — VM will handle the fiber swap
    (SIG_RESUME, args[0])
}

/// (emit bits value) → suspends
///
/// Emit a signal from the current fiber. The signal bits and value are
/// returned directly — the VM's dispatch loop stores them in fiber.signal
/// and suspends the fiber.
pub(crate) fn prim_emit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("emit: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let bits = match resolve_signal_bits(&args[0], "emit") {
        Ok(bits) => bits,
        Err(err) => return err,
    };

    // Return the signal bits and value directly.
    // The VM's handle_primitive_signal catch-all stores (bits, value)
    // in fiber.signal and returns Some(bits), suspending the fiber.
    (bits, args[1])
}

/// (fiber/status fiber) → keyword
///
/// Returns the fiber's lifecycle status as a keyword.
pub(crate) fn prim_fiber_status(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/status: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/status: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let status = handle.with(|fiber| fiber.status);
    (SIG_OK, status_keyword(status))
}

/// (fiber/value fiber) → value
///
/// Returns the signal payload from the fiber's last signal or return value.
/// Returns nil if the fiber has no signal.
pub(crate) fn prim_fiber_value(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/value: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/value: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let value = handle.with(|fiber| fiber.signal.as_ref().map(|(_, v)| *v).unwrap_or(Value::NIL));
    (SIG_OK, value)
}

/// (fiber/set-fuel fiber n) → nil
///
/// Set the instruction budget on a fiber. `n` must be a non-negative integer.
/// A fuel of 0 means the very next fuel checkpoint emits `:fuel`.
pub(crate) fn prim_fiber_set_fuel(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/set-fuel: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "fiber/set-fuel: expected fiber, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let fuel = match args[1].as_int() {
        Some(n) if n >= 0 => n as u32,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val("type-error", "fiber/set-fuel: fuel must be non-negative"),
            );
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "fiber/set-fuel: expected integer, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };

    handle.with_mut(|fiber| {
        fiber.fuel = Some(fuel);
    });

    (SIG_OK, Value::NIL)
}

/// (fiber/fuel fiber) → integer | nil
///
/// Read the remaining instruction budget. Returns an integer if fuel is set,
/// or `nil` if the fiber has unlimited fuel (the default).
pub(crate) fn prim_fiber_fuel(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/fuel: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("fiber/fuel: expected fiber, got {}", args[0].type_name()),
                ),
            );
        }
    };

    let fuel_val = handle.with(|fiber| {
        fiber
            .fuel
            .map(|f| Value::int(f as i64))
            .unwrap_or(Value::NIL)
    });

    (SIG_OK, fuel_val)
}

/// (fiber/clear-fuel fiber) → nil
///
/// Remove the instruction budget, restoring unlimited execution.
pub(crate) fn prim_fiber_clear_fuel(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fiber/clear-fuel: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let handle = match args[0].as_fiber() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "fiber/clear-fuel: expected fiber, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    handle.with_mut(|fiber| {
        fiber.fuel = None;
    });

    (SIG_OK, Value::NIL)
}

/// Declarative primitive definitions for fiber lifecycle operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "fiber/new",
        func: prim_fiber_new,
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Create a fiber with a signal mask. Optional :deny withholds capabilities.",
        params: &["closure", "mask"],
        category: "fiber",
        example: "(fiber/new (fn [] 42) |:error| :deny |:io|)",
        aliases: &["fiber"],
    },
    PrimitiveDef {
        name: "fiber/resume",
        func: prim_fiber_resume,
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_RESUME),
            propagates: 0,
        },
        arity: Arity::Range(1, 2),
        doc: "Resume a fiber, optionally delivering a value",
        params: &["fiber", "value"],
        category: "fiber",
        example: "(fiber/resume f)",
        aliases: &["resume"],
    },
    PrimitiveDef {
        name: "fiber/emit",
        func: prim_emit,
        signal: Signal::yields_errors(),
        arity: Arity::Exact(2),
        doc: "Emit a signal from the current fiber",
        params: &["bits", "value"],
        category: "fiber",
        example: "(emit 2 42)",
        aliases: &["emit"],
    },
    PrimitiveDef {
        name: "fiber/status",
        func: prim_fiber_status,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the fiber's lifecycle status (:new, :alive, :suspended, :dead, :error)",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/status f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/value",
        func: prim_fiber_value,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the signal payload from the fiber's last signal",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/value f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/set-fuel",
        func: prim_fiber_set_fuel,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Set the instruction budget on a fiber. n is a non-negative integer.",
        params: &["fiber", "n"],
        category: "fiber",
        example: "(fiber/set-fuel f 10000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/fuel",
        func: prim_fiber_fuel,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read remaining fuel. Returns integer or nil if unlimited.",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/fuel f)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fiber/clear-fuel",
        func: prim_fiber_clear_fuel,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Remove the instruction budget, restoring unlimited execution.",
        params: &["fiber"],
        category: "fiber",
        example: "(fiber/clear-fuel f)",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LocationMap;
    use crate::signals::Signal;
    use crate::value::fiber::{SIG_ERROR as FIBER_SIG_ERROR, SIG_YIELD};
    use crate::value::{Arity, Closure};
    use std::collections::HashMap;
    use std::rc::Rc;

    fn make_test_closure() -> Value {
        use crate::compiler::bytecode::Instruction;
        use crate::value::ClosureTemplate;
        let bytecode = vec![
            Instruction::LoadConst as u8,
            0,
            0,
            Instruction::Return as u8,
        ];

        let template = Rc::new(ClosureTemplate {
            bytecode: Rc::new(bytecode),
            arity: Arity::Exact(0),
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants: Rc::new(vec![Value::int(42)]),
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

        Value::closure(Closure {
            template,
            env: crate::value::inline_slice::InlineSlice::empty(),
            squelch_mask: SignalBits::EMPTY,
        })
    }

    #[test]
    fn test_fiber_new() {
        let closure = make_test_closure();
        let mask = Value::int((SIG_ERROR | SIG_YIELD).raw() as i64);
        let (sig, fiber_val) = prim_fiber_new(&[closure, mask]);
        assert_eq!(sig, SIG_OK);
        assert!(fiber_val.is_fiber());

        let handle = fiber_val.as_fiber().unwrap();
        handle.with(|fiber| {
            assert_eq!(fiber.status, FiberStatus::New);
            assert_eq!(fiber.mask, FIBER_SIG_ERROR | SIG_YIELD);
        });
    }

    #[test]
    fn test_fiber_new_wrong_type() {
        let (sig, _) = prim_fiber_new(&[Value::int(42), Value::int(0)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_new_wrong_arity() {
        let (sig, _) = prim_fiber_new(&[make_test_closure()]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_resume_returns_sig_resume() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, val) = prim_fiber_resume(&[fiber_val]);
        assert_eq!(sig, SIG_RESUME);
        assert!(val.is_fiber());
    }

    #[test]
    fn test_fiber_resume_dead_fiber() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        // Manually set to Dead
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.status = FiberStatus::Dead);
        let (sig, _) = prim_fiber_resume(&[fiber_val]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_resume_errored_fiber_is_allowed() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.status = FiberStatus::Error);
        let (sig, _) = prim_fiber_resume(&[fiber_val]);
        assert_eq!(
            sig, SIG_RESUME,
            "errored fibers must be resumable (restarts)"
        );
    }

    #[test]
    fn test_fiber_resume_alive_fiber() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.status = FiberStatus::Alive);
        let (sig, _) = prim_fiber_resume(&[fiber_val]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_resume_with_value() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_resume(&[fiber_val, Value::int(99)]);
        assert_eq!(sig, SIG_RESUME);
        // Check that the resume value was stored
        fiber_val.as_fiber().unwrap().with(|fiber| {
            assert_eq!(fiber.signal, Some((SIG_OK, Value::int(99))));
        });
    }

    #[test]
    fn test_fiber_signal() {
        let bits = Value::int(SIG_YIELD.raw() as i64);
        let value = Value::int(42);
        let (sig, val) = prim_emit(&[bits, value]);
        assert_eq!(sig, SIG_YIELD);
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn test_fiber_signal_wrong_arity() {
        let (sig, _) = prim_emit(&[Value::int(0)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_status() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, status) = prim_fiber_status(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert!(status.is_keyword(), "Expected keyword, got {:?}", status);
    }

    #[test]
    fn test_fiber_status_transitions() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);

        // All statuses should return keywords
        for status in [
            FiberStatus::New,
            FiberStatus::Alive,
            FiberStatus::Paused,
            FiberStatus::Dead,
            FiberStatus::Error,
        ] {
            fiber_val
                .as_fiber()
                .unwrap()
                .with_mut(|f| f.status = status);
            let (sig, val) = prim_fiber_status(&[fiber_val]);
            assert_eq!(sig, SIG_OK);
            assert!(
                val.is_keyword(),
                "Expected keyword for {:?}, got {:?}",
                status,
                val
            );
        }
    }

    #[test]
    fn test_fiber_value() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);

        // No signal yet — returns nil
        let (sig, val) = prim_fiber_value(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::NIL);

        // Set a signal
        fiber_val
            .as_fiber()
            .unwrap()
            .with_mut(|f| f.signal = Some((SIG_YIELD, Value::int(42))));
        let (sig, val) = prim_fiber_value(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn test_fiber_resume_wrong_type() {
        let (sig, _) = prim_fiber_resume(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_status_wrong_type() {
        let (sig, _) = prim_fiber_status(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_value_wrong_type() {
        let (sig, _) = prim_fiber_value(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    // ── fiber/set-fuel ─────────────────────────────────────────────────────

    #[test]
    fn test_fiber_set_fuel_stores_value() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_set_fuel(&[fiber_val, Value::int(1000)]);
        assert_eq!(sig, SIG_OK);
        fiber_val.as_fiber().unwrap().with(|fiber| {
            assert_eq!(fiber.fuel, Some(1000));
        });
    }

    #[test]
    fn test_fiber_set_fuel_zero() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_set_fuel(&[fiber_val, Value::int(0)]);
        assert_eq!(sig, SIG_OK);
        fiber_val.as_fiber().unwrap().with(|fiber| {
            assert_eq!(fiber.fuel, Some(0));
        });
    }

    #[test]
    fn test_fiber_set_fuel_overwrites() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        prim_fiber_set_fuel(&[fiber_val, Value::int(500)]);
        prim_fiber_set_fuel(&[fiber_val, Value::int(100)]);
        fiber_val.as_fiber().unwrap().with(|fiber| {
            assert_eq!(fiber.fuel, Some(100));
        });
    }

    #[test]
    fn test_fiber_set_fuel_wrong_arity() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_set_fuel(&[fiber_val]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_set_fuel_not_a_fiber() {
        let (sig, _) = prim_fiber_set_fuel(&[Value::int(42), Value::int(100)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_set_fuel_negative() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_set_fuel(&[fiber_val, Value::int(-1)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_set_fuel_non_integer() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_set_fuel(&[fiber_val, Value::keyword("oops")]);
        assert_eq!(sig, SIG_ERROR);
    }

    // ── fiber/fuel ──────────────────────────────────────────────────────────

    #[test]
    fn test_fiber_fuel_returns_nil_when_unlimited() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, val) = prim_fiber_fuel(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::NIL);
    }

    #[test]
    fn test_fiber_fuel_returns_integer_when_set() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        prim_fiber_set_fuel(&[fiber_val, Value::int(42)]);
        let (sig, val) = prim_fiber_fuel(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val, Value::int(42));
    }

    #[test]
    fn test_fiber_fuel_wrong_arity() {
        let (sig, _) = prim_fiber_fuel(&[]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_fuel_not_a_fiber() {
        let (sig, _) = prim_fiber_fuel(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    // ── fiber/clear-fuel ────────────────────────────────────────────────────

    #[test]
    fn test_fiber_clear_fuel_removes_limit() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        prim_fiber_set_fuel(&[fiber_val, Value::int(100)]);
        let (sig, _) = prim_fiber_clear_fuel(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        fiber_val.as_fiber().unwrap().with(|fiber| {
            assert_eq!(fiber.fuel, None);
        });
    }

    #[test]
    fn test_fiber_clear_fuel_on_unlimited_is_noop() {
        let closure = make_test_closure();
        let (_, fiber_val) = prim_fiber_new(&[closure, Value::int(0)]);
        let (sig, _) = prim_fiber_clear_fuel(&[fiber_val]);
        assert_eq!(sig, SIG_OK);
        fiber_val.as_fiber().unwrap().with(|fiber| {
            assert_eq!(fiber.fuel, None);
        });
    }

    #[test]
    fn test_fiber_clear_fuel_wrong_arity() {
        let (sig, _) = prim_fiber_clear_fuel(&[]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_fiber_clear_fuel_not_a_fiber() {
        let (sig, _) = prim_fiber_clear_fuel(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }
}
