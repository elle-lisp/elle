//! Call and TailCall instruction handlers.
//!
//! Handles:
//! - Native function calls (routes to signal dispatch in signal.rs)
//! - Closure calls with environment setup
//! - Yield-through-calls (suspended frame chain building)
//! - Tail call optimization
//! - Environment building and parameter binding

use crate::error::LocationMap;
use crate::value::error_val;
use crate::value::fiber::CallFrame;
use crate::value::{BytecodeFrame, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_OK};
// SmallVec was tried here but benchmarks showed no improvement over Vec
// for the common 0-8 arg case. The inline storage (64 bytes) touches a
// full cache line regardless of arg count, and the is-inline branch on
// every push adds overhead that cancels out the allocation savings.
use std::rc::Rc;

use super::core::VM;

/// Helper: set an error signal on the fiber.
fn set_error(fiber: &mut crate::value::Fiber, kind: &str, msg: impl Into<String>) {
    fiber.signal = Some((SIG_ERROR, error_val(kind, msg)));
}

impl VM {
    /// Handle the Call instruction.
    ///
    /// Pops the function and arguments from the stack, calls the function,
    /// and pushes the result. Handles native functions, VM-aware functions,
    /// and closures with proper environment setup.
    ///
    /// Returns `Some(SignalBits)` if execution should return immediately,
    /// or `None` if the dispatch loop should continue.
    pub(super) fn handle_call(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> Option<SignalBits> {
        let bc: &[u8] = bytecode;
        let arg_count = self.read_u8(bc, ip) as usize;
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on Call");

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(
                self.fiber
                    .stack
                    .pop()
                    .expect("VM bug: Stack underflow on Call"),
            );
        }
        args.reverse();

        self.call_inner(
            func,
            args,
            bytecode,
            constants,
            closure_env,
            ip,
            instr_ip,
            location_map,
        )
    }

    /// Handle the CallArrayMut instruction.
    ///
    /// Like Call, but instead of reading arg_count from bytecode and popping
    /// individual args, pops an args array and uses its elements as arguments.
    /// Used by splice: the lowerer builds an args array, then CallArrayMut
    /// calls the function with those args.
    ///
    /// Stack: \[func, args_array\] → \[result\]
    pub(super) fn handle_call_array(
        &mut self,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> Option<SignalBits> {
        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArrayMut");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on CallArrayMut");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_array() {
            tup.to_vec()
        } else {
            set_error(
                &mut self.fiber,
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            self.fiber.stack.push(Value::NIL);
            return None;
        };

        self.call_inner(
            func,
            args,
            bytecode,
            constants,
            closure_env,
            ip,
            instr_ip,
            location_map,
        )
    }

    /// Shared Call/CallArrayMut logic after argument extraction.
    ///
    /// Dispatches native functions, executes closures with environment setup,
    /// handles yield-through-calls and JIT compilation.
    #[allow(clippy::too_many_arguments)]
    fn call_inner(
        &mut self,
        func: Value,
        args: Vec<Value>,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        location_map: &Rc<LocationMap>,
    ) -> Option<SignalBits> {
        if let Some(f) = func.as_native_fn() {
            let (bits, value) = f(args.as_slice());
            return self.handle_primitive_signal(
                bits,
                value,
                bytecode,
                constants,
                closure_env,
                ip,
                location_map,
            );
        }

        if let Some((id, default)) = func.as_parameter() {
            if !args.is_empty() {
                set_error(
                    &mut self.fiber,
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", args.len()),
                );
                self.fiber.stack.push(Value::NIL);
                return None;
            }
            let value = self.resolve_parameter(id, default);
            self.fiber.stack.push(value);
            return None;
        }

        if let Some(closure) = func.as_closure() {
            self.fiber.call_depth += 1;
            if self.fiber.call_depth > 1000 {
                set_error(&mut self.fiber, "error", "Stack overflow");
                return Some(SIG_ERROR);
            }

            // Push call frame for stack traces
            self.fiber.call_stack.push(CallFrame {
                name: closure
                    .template
                    .name
                    .clone()
                    .unwrap_or_else(|| Rc::from("<anonymous>")),
                ip: instr_ip,
                frame_base: 0, // Closures always execute with fresh stack via execute_bytecode_saving_stack
                location_map: location_map.clone(),
            });

            // Validate argument count
            if !self.check_arity(&closure.template.arity, args.len()) {
                self.fiber.call_depth -= 1;
                self.fiber.call_stack.pop();
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // JIT compilation and dispatch.
            // Polymorphic closures are rejected by the JIT compiler itself.
            // Skip profiling for primitives (no LIR means not JIT-compilable).
            if closure.template.lir_function.is_some() {
                if let Some(bits) = self.try_jit_call(closure, &args, func) {
                    self.fiber.call_depth -= 1;
                    match bits {
                        Some(sig) if !sig.contains(SIG_ERROR) && !sig.contains(SIG_HALT) => {
                            // JIT function suspended — any bits except SIG_ERROR/SIG_HALT
                            // cause the caller frame to be appended for resumption.
                            // fiber.signal and fiber.suspended are set by the JIT yield
                            // helpers. Build the interpreter-level caller frame.
                            // Use unwrap_or_default() so this works whether the JIT callee
                            // populated fiber.suspended or not (tail-call-to-native path).
                            {
                                let (_, value) = self.fiber.signal.take().unwrap();
                                let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                                let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame {
                                    bytecode: bytecode.clone(),
                                    constants: constants.clone(),
                                    env: closure_env.clone(),
                                    ip: *ip,
                                    stack: caller_stack,
                                    active_allocator:
                                        crate::value::fiber_heap::save_active_allocator(),
                                    location_map: location_map.clone(),
                                });
                                let mut frames = self.fiber.suspended.take().unwrap_or_default();
                                frames.push(caller_frame);
                                self.fiber.signal = Some((sig, value));
                                self.fiber.suspended = Some(frames);
                            }
                            return Some(sig);
                        }
                        other => return other,
                    }
                }
            }

            // Build the new environment
            let new_env_rc = match self.build_closure_env(closure, &args) {
                Some(env) => env,
                None => {
                    self.fiber.call_depth -= 1;
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            };

            // Execute the closure, saving/restoring the caller's stack.
            // Essential for fiber/signal propagation and yield-through-nested-calls.
            let result = self.execute_bytecode_saving_stack(
                &closure.template.bytecode,
                &closure.template.constants,
                &new_env_rc,
                &closure.template.location_map,
            );

            self.fiber.call_depth -= 1;

            let bits = result.bits;
            if bits.is_ok() {
                let (_, value) = self.fiber.signal.take().unwrap();
                self.fiber.stack.push(value);
                self.fiber.call_stack.pop();
            } else if !bits.contains(SIG_ERROR) && !bits.contains(SIG_HALT) {
                // Suspending signal — any bits except SIG_ERROR/SIG_HALT
                // cause the caller frame to be appended for resumption.
                // Propagated from a nested call (interpreter or tail-call-to-native path).
                // We must always build the caller frame, whether or not the callee
                // already populated fiber.suspended. When the callee is a TailCall to
                // a native yielding primitive, it does NOT create a SuspendedFrame
                // (TCO), so fiber.suspended may be None here — use unwrap_or_default()
                // to cover both cases.
                {
                    let (_, value) = self.fiber.signal.take().unwrap();

                    let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame {
                        bytecode: bytecode.clone(),
                        constants: constants.clone(),
                        env: closure_env.clone(),
                        ip: *ip,
                        stack: caller_stack,
                        active_allocator: crate::value::fiber_heap::save_active_allocator(),
                        location_map: location_map.clone(),
                    });

                    let mut frames = self.fiber.suspended.take().unwrap_or_default();
                    frames.push(caller_frame);
                    self.fiber.signal = Some((bits, value));
                    self.fiber.suspended = Some(frames);
                }
                self.fiber.call_stack.pop();
                return Some(bits);
            } else {
                // Other signal (error, etc.) — propagate to caller.
                // The call frame is preserved on error for stack traces.
                return Some(bits);
            }
            return None;
        }

        // Cannot call this value
        set_error(
            &mut self.fiber,
            "type-error",
            format!("Cannot call {:?}", func),
        );
        self.fiber.stack.push(Value::NIL);
        None
    }

    /// Handle the TailCall instruction.
    ///
    /// Similar to Call but sets up a pending tail call instead of recursing,
    /// enabling tail call optimization.
    ///
    /// Returns `Some(SignalBits)` if execution should return immediately,
    /// or `None` if the dispatch loop should continue.
    pub(super) fn handle_tail_call(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
    ) -> Option<SignalBits> {
        let arg_count = self.read_u8(bytecode, ip) as usize;
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCall");

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(
                self.fiber
                    .stack
                    .pop()
                    .expect("VM bug: Stack underflow on TailCall"),
            );
        }
        args.reverse();

        self.tail_call_inner(func, args)
    }

    /// Handle the TailCallArrayMut instruction.
    ///
    /// Like TailCall, but pops an args array instead of individual args.
    /// Stack: \[func, args_array\] → (sets up pending tail call)
    pub(super) fn handle_tail_call_array(
        &mut self,
        ip: &mut usize,
        bytecode: &[u8],
    ) -> Option<SignalBits> {
        // Suppress unused warnings — these params match the dispatch signature
        let _ = (ip, bytecode);

        let args_val = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArrayMut");
        let func = self
            .fiber
            .stack
            .pop()
            .expect("VM bug: Stack underflow on TailCallArrayMut");

        // Extract args from the array
        let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
            arr.borrow().to_vec()
        } else if let Some(tup) = args_val.as_array() {
            tup.to_vec()
        } else {
            set_error(
                &mut self.fiber,
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            );
            return Some(SIG_ERROR);
        };

        self.tail_call_inner(func, args)
    }

    /// Shared TailCall/TailCallArrayMut logic after argument extraction.
    ///
    /// Dispatches native functions via tail signal handler, sets up pending
    /// tail call for closures with environment building.
    fn tail_call_inner(&mut self, func: Value, args: Vec<Value>) -> Option<SignalBits> {
        if let Some(f) = func.as_native_fn() {
            let (bits, value) = f(&args);
            return Some(self.handle_primitive_signal_tail(bits, value));
        }

        if let Some((id, default)) = func.as_parameter() {
            if !args.is_empty() {
                set_error(
                    &mut self.fiber,
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", args.len()),
                );
                return Some(SIG_ERROR);
            }
            let value = self.resolve_parameter(id, default);
            self.fiber.signal = Some((SIG_OK, value));
            return Some(SIG_OK);
        }

        if let Some(closure) = func.as_closure() {
            // Validate argument count
            if !self.check_arity(&closure.template.arity, args.len()) {
                // check_arity sets fiber.signal to (SIG_ERROR, ...)
                return Some(SIG_ERROR);
            }

            // Build proper environment using cached vector
            if !Self::populate_env(
                &mut self.tail_call_env_cache,
                &mut self.fiber,
                closure,
                &args,
            ) {
                return Some(SIG_ERROR);
            }
            let new_env_rc = Rc::new(self.tail_call_env_cache.clone());

            // Store the tail call information (Rc clones, not data copies)
            self.pending_tail_call = Some(crate::vm::core::TailCallInfo {
                bytecode: closure.template.bytecode.clone(),
                constants: closure.template.constants.clone(),
                env: new_env_rc,
                location_map: closure.template.location_map.clone(),
            });

            self.fiber.signal = Some((SIG_OK, Value::NIL));
            return Some(SIG_OK);
        }

        // Cannot call this value
        set_error(
            &mut self.fiber,
            "type-error",
            format!("Cannot call {:?}", func),
        );
        Some(SIG_ERROR)
    }

    // ── Environment building ────────────────────────────────────────

    /// Build a closure environment from captured variables and arguments.
    ///
    /// Reuses `self.env_cache` to avoid a fresh Vec allocation per call.
    /// Returns `None` if `populate_env` fails (e.g., bad keyword args for `&keys`/`&named`).
    pub(super) fn build_closure_env(
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
    fn populate_env(
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
            if i >= 64 || (closure.template.lbox_locals_mask & (1 << i)) != 0 {
                buf.push(Value::local_lbox(Value::NIL));
            } else {
                buf.push(Value::NIL);
            }
        }

        true
    }

    /// Push a parameter value into the environment buffer, wrapping in a
    /// LocalCell if the lbox_params_mask indicates it's needed.
    #[inline]
    fn push_param(buf: &mut Vec<Value>, closure: &crate::value::Closure, i: usize, val: Value) {
        if i < 64 && (closure.template.lbox_params_mask & (1 << i)) != 0 {
            buf.push(Value::local_lbox(val));
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
                "error",
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
                        "error",
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
                        "error",
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
                    "error",
                    format!("duplicate keyword argument :{}", key),
                );
                return None;
            }
            map.insert(table_key, args[i + 1]);
        }
        Some(Value::struct_from(map))
    }
}
