//! Closure environment building.
//!
//! Handles constructing the `Vec<Value>` environment that a closure receives
//! at call time: captured variables, positional arguments (with optional lbox
//! wrapping), rest-parameter collection (list, struct, or strict struct), and
//! local variable slots.
//!
//! Entry points:
//! - `build_closure_env`: reuses `env_cache` to avoid a fresh allocation per call
//! - `populate_env`: fills a caller-supplied buffer; shared by `build_closure_env`
//!   and `tail_call_inner` (which uses `tail_call_env_cache`)

use crate::value::error_val;
use crate::value::{Value, SIG_ERROR};
use std::rc::Rc;

use super::core::VM;

/// Helper: set an error signal on the fiber.
fn set_error(fiber: &mut crate::value::Fiber, kind: &str, msg: impl Into<String>) {
    fiber.signal = Some((SIG_ERROR, error_val(kind, msg)));
}

impl VM {
    /// Build a closure environment from captured variables and arguments.
    ///
    /// Reuses `self.env_cache` to avoid a fresh Vec allocation per call.
    /// Returns `None` if `populate_env` fails (e.g., bad keyword args for `&keys`/`&named`).
    pub fn build_closure_env(
        &mut self,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> Option<Rc<Vec<Value>>> {
        if !Self::populate_env(&mut self.env_cache, &mut self.fiber, closure, args) {
            return None;
        }
        Some(Rc::new(self.env_cache.clone()))
    }

    /// Populate an environment buffer with captures, arguments, and local slots.
    ///
    /// Shared by `build_closure_env` (which uses `env_cache`) and
    /// `tail_call_inner` (which uses `tail_call_env_cache`). The two caches
    /// can't alias — a tail call may occur inside a closure call that is
    /// still using `env_cache`.
    ///
    /// Returns `false` if keyword argument collection fails (error set on fiber).
    pub(super) fn populate_env(
        buf: &mut Vec<Value>,
        fiber: &mut crate::value::Fiber,
        closure: &crate::value::Closure,
        args: &[Value],
    ) -> bool {
        buf.clear();
        let needed = closure.env_capacity();
        if buf.capacity() < needed {
            buf.reserve(needed - buf.len());
        }
        buf.extend((*closure.env).iter().cloned());

        match closure.template.arity {
            crate::value::Arity::AtLeast(min) => {
                // Total fixed slots = num_params - 1 (rest slot is last param)
                let fixed_slots = closure.template.num_params - 1;

                // Determine how many positional args to consume for fixed slots.
                // For &keys/&named, keyword args should not fill optional slots —
                // once we see a keyword past the required params, the rest are
                // keyword arguments for the collector.
                let collects_keywords = matches!(
                    closure.template.vararg_kind,
                    crate::hir::VarargKind::Struct | crate::hir::VarargKind::StrictStruct(_)
                );
                let provided_fixed = if collects_keywords {
                    // Always fill required slots, then fill optional slots
                    // only with non-keyword args
                    let mut count = args.len().min(min);
                    while count < fixed_slots && count < args.len() {
                        if args[count].as_keyword_name().is_some() {
                            break;
                        }
                        count += 1;
                    }
                    count
                } else {
                    args.len().min(fixed_slots)
                };

                // Push args for fixed slots (required + optional)
                for (i, arg) in args[..provided_fixed].iter().enumerate() {
                    Self::push_param(buf, closure, i, *arg);
                }
                // Fill missing optional slots with nil
                for i in provided_fixed..fixed_slots {
                    Self::push_param(buf, closure, i, Value::NIL);
                }

                // Collect remaining args into rest slot
                let rest_args = if args.len() > provided_fixed {
                    &args[provided_fixed..]
                } else {
                    &[]
                };
                let collected = match &closure.template.vararg_kind {
                    crate::hir::VarargKind::List => Self::args_to_list(rest_args),
                    crate::hir::VarargKind::Struct => {
                        match Self::args_to_struct_static(fiber, rest_args, None) {
                            Some(v) => v,
                            None => return false,
                        }
                    }
                    crate::hir::VarargKind::StrictStruct(ref keys) => {
                        match Self::args_to_struct_static(fiber, rest_args, Some(keys)) {
                            Some(v) => v,
                            None => return false,
                        }
                    }
                };
                Self::push_param(buf, closure, fixed_slots, collected);
            }
            crate::value::Arity::Range(_, max) => {
                // All slots are fixed (no rest param)
                // Push provided args
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, closure, i, *arg);
                }
                // Fill missing optional slots with nil
                for i in args.len()..max {
                    Self::push_param(buf, closure, i, Value::NIL);
                }
            }
            crate::value::Arity::Exact(_) => {
                for (i, arg) in args.iter().enumerate() {
                    Self::push_param(buf, closure, i, *arg);
                }
            }
        }

        // Add slots for locally-defined variables.
        // cell-wrapped locals (captured by nested closures) get LocalCell(NIL).
        // Non-cell locals get bare NIL — they use stack slots via StoreLocal/LoadLocal
        // and the env slot is never accessed.
        // Beyond index 63, the mask can't represent the local — conservatively
        // use LocalCell (matches the emitter's fallback to StoreUpvalue).
        let num_locally_defined = closure
            .template
            .num_locals
            .saturating_sub(closure.template.num_params);
        for i in 0..num_locally_defined {
            if i >= 64 || (closure.template.capture_locals_mask & (1 << i)) != 0 {
                buf.push(Value::capture_cell(Value::NIL));
            } else {
                buf.push(Value::NIL);
            }
        }

        true
    }

    /// Push a parameter value into the environment buffer, wrapping in a
    /// LocalCell if the capture_params_mask indicates it's needed.
    #[inline]
    fn push_param(buf: &mut Vec<Value>, closure: &crate::value::Closure, i: usize, val: Value) {
        if i < 64 && (closure.template.capture_params_mask & (1 << i)) != 0 {
            buf.push(Value::capture_cell(val));
        } else {
            buf.push(val);
        }
    }

    /// Collect values into an Elle list (cons chain terminated by EMPTY_LIST).
    fn args_to_list(args: &[Value]) -> Value {
        let mut list = Value::EMPTY_LIST;
        for arg in args.iter().rev() {
            list = Value::cons(*arg, list);
        }
        list
    }

    /// Collect alternating key-value args into an immutable struct.
    /// Returns `None` if odd number of args or non-keyword keys (error set on fiber).
    /// If `valid_keys` is `Some`, returns error on unknown keys (strict `&named` mode).
    fn args_to_struct_static(
        fiber: &mut crate::value::Fiber,
        args: &[Value],
        valid_keys: Option<&[String]>,
    ) -> Option<Value> {
        use crate::value::types::TableKey;
        use std::collections::BTreeMap;

        if args.is_empty() {
            return Some(Value::struct_from(BTreeMap::new()));
        }

        if !args.len().is_multiple_of(2) {
            set_error(
                fiber,
                "argument-error",
                format!("odd number of keyword arguments ({} args)", args.len()),
            );
            return None;
        }

        let mut map = BTreeMap::new();
        for i in (0..args.len()).step_by(2) {
            let key = match TableKey::from_value(&args[i]) {
                Some(TableKey::Keyword(k)) => k,
                _ => {
                    set_error(
                        fiber,
                        "argument-error",
                        format!(
                            "keyword argument key must be a keyword, got {}",
                            args[i].type_name()
                        ),
                    );
                    return None;
                }
            };

            // Strict validation for &named
            if let Some(valid) = valid_keys {
                if !valid.iter().any(|v| v == &key) {
                    set_error(
                        fiber,
                        "argument-error",
                        format!(
                            "unknown named parameter :{}, valid parameters are: {}",
                            key,
                            valid
                                .iter()
                                .map(|v| format!(":{}", v))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                    return None;
                }
            }

            let table_key = TableKey::Keyword(key.clone());
            if map.contains_key(&table_key) {
                set_error(
                    fiber,
                    "argument-error",
                    format!("duplicate keyword argument :{}", key),
                );
                return None;
            }
            map.insert(table_key, args[i + 1]);
        }
        Some(Value::struct_from(map))
    }
}
