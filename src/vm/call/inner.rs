use super::*;

mod tail;

impl VM {
    /// Shared Call/CallArrayMut logic after argument extraction.
    ///
    /// Dispatches native functions, executes closures with environment setup,
    /// handles yield-through-calls and JIT compilation.
    #[allow(clippy::too_many_arguments)]
    ///
    /// When `checked` is true, the compiler verified arity at compile time
    /// and the runtime skips the arity check for primitives and closures.
    pub(super) fn call_inner(
        &mut self,
        func: Value,
        args: Vec<Value>,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        instr_ip: usize,
        checked: bool,
        region_id: StaticRegion,
    ) -> Option<SignalBits> {
        if let Some(def) = func.as_native_def() {
            etrace!(
                self,
                crate::config::trace_bits::CALL,
                "call",
                "native {} nargs={}",
                def.name,
                args.len()
            );
            let blocked = def
                .signal
                .bits
                .intersection(self.fiber.withheld)
                .intersection(crate::signals::CAP_MASK);
            if !blocked.is_empty() {
                return self.handle_capability_denial(def, blocked, &args, code, closure_env, ip);
            }
            if !checked && !def.arity.matches(args.len()) {
                self.set_error(
                    "arity-error",
                    format!(
                        "{}: expected {} argument(s), got {}",
                        def.name,
                        def.arity,
                        args.len()
                    ),
                );
                self.fiber.stack.push(Value::NIL);
                return None;
            }
            // Each native call mints its own fresh physical region for its
            // result (docs/regions/semantics.md — every value its own region), routes the
            // primitive's allocations into it, and hands the caller the
            // pass-through retain. Shared with the JIT (`elle_jit_call`) so both
            // tiers account identically; see `VM::dispatch_native_call`.
            let (bits, value) = self.dispatch_native_call(def, args.as_slice(), region_id);
            return self.handle_primitive_signal(bits, value, code, closure_env, ip);
        }

        if let Some((id, default)) = func.as_parameter() {
            if !args.is_empty() {
                self.set_error(
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", args.len()),
                );
                self.fiber.stack.push(Value::NIL);
                return None;
            }
            let value = self.resolve_parameter(id, default);
            // A parameter resolve is *always* a pass-through: it returns a value
            // stored in the dynamic-binding frame, never a fresh allocation into
            // this call's region. So hand the caller one owning reference so its
            // `DecrefValueRegion` at the `(param)` call's decref_point balances
            // against this extra ref instead of freeing the still-bound value out
            // from under the parameter frame. `incref_for_escape(None, …)` is a
            // no-op, so an immediate (no region) costs nothing. The retain is
            // unconditional: a static-vs-runtime region compare that could
            // spuriously skip it is prevented by the `StaticRegion`/`RuntimeRegion`
            // newtypes (the two cannot be compared without a compile error).
            let heap = unsafe { &mut *self.heap_ptr };
            let result_region = crate::value::arena::region_of(heap, value);
            crate::value::arena::incref_for_escape(
                heap,
                result_region,
                crate::value::arena::EscapeSite::ParameterResolve,
            );
            self.fiber.stack.push(value);
            return None;
        }

        if let Some(closure) = func.as_closure() {
            etrace!(
                self,
                crate::config::trace_bits::CALL,
                "call",
                "closure {} nargs={}",
                closure.template.name.as_deref().unwrap_or("<anon>"),
                args.len()
            );
            self.fiber.call_depth += 1;

            // Push call frame for stack traces
            self.fiber.call_stack.push(CallFrame {
                name: closure
                    .template
                    .name
                    .clone()
                    .unwrap_or_else(|| Rc::from("<anonymous>")),
                ip: instr_ip,
                frame_base: 0, // Closures always execute with fresh stack via execute_bytecode_saving_stack
                location_map: code.location_map.clone(),
            });

            // Stack overflow guard: resource exhaustion (not a signal-theoretic
            // error — the analyzer cannot predict this).  Uses SIG_HALT so the
            // condition bypasses all signal masks and propagates to the top-level
            // executor as a fatal error.
            if self.fiber.call_depth > MAX_CALL_DEPTH {
                self.fiber.call_depth -= 1;
                self.fiber.call_stack.pop();
                let err = self.escaping_error(
                    "stack-overflow",
                    format!("call depth exceeded maximum ({})", MAX_CALL_DEPTH),
                );
                self.fiber.signal = Some((SIG_HALT, err));
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // Validate argument count (skip if compiler verified)
            if !checked && !self.check_arity(&closure.template.arity, args.len()) {
                self.fiber.call_depth -= 1;
                self.fiber.call_stack.pop();
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // GPU capability check: if this closure has been GIT'd (has SPIR-V),
            // it requires GPU hardware. Check capability before dispatch.
            if closure.template.spirv.get().is_some() {
                let gpu_bit = crate::signals::SIG_GPU;
                let blocked = gpu_bit
                    .intersection(self.fiber.withheld)
                    .intersection(crate::signals::CAP_MASK);
                if !blocked.is_empty() {
                    self.fiber.call_depth -= 1;
                    self.fiber.call_stack.pop();
                    // The denial cons escapes into `fiber.signal`; born in a
                    // fresh region on this fiber's own heap.
                    let denial_region = unsafe { (*self.heap_ptr).new_runtime_region() };
                    let denial = crate::value::build::pair(
                        unsafe { &mut *self.heap_ptr },
                        Value::keyword("capability-denied"),
                        Value::keyword("gpu"),
                        denial_region,
                    );
                    self.fiber.signal = Some((blocked, denial));
                    return None;
                }
            }

            // Tiered WASM compilation and dispatch.
            // Checked before JIT because WASM is the preferred fast path when enabled.
            #[cfg(feature = "wasm")]
            if closure.template.lir_function.is_some() {
                if let Some(bits) = self.try_wasm_call(closure, &args, func) {
                    self.fiber.call_depth -= 1;
                    self.fiber.call_stack.pop();
                    return bits;
                }
            }

            // MLIR tier-2: GPU-eligible functions compiled through LLVM.
            // Checked before Cranelift — MLIR produces better optimized code
            // for numeric functions (LLVM vectorization, LICM, GVN).
            #[cfg(feature = "mlir")]
            if self.mlir_enabled && closure.template.lir_function.is_some() {
                if let Some(bits) = self.try_mlir_call(closure, &args) {
                    self.fiber.call_depth -= 1;
                    self.fiber.call_stack.pop();
                    return bits;
                }
            }

            // JIT compilation and dispatch.
            // Polymorphic closures are rejected by the JIT compiler itself.
            // Skip profiling for primitives (no LIR means not JIT-compilable).
            #[cfg(feature = "jit")]
            if closure.template.lir_function.is_some() {
                if let Some(bits) = self.try_jit_call(closure, &args, func) {
                    self.fiber.call_depth -= 1;
                    self.fiber.call_stack.pop();
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
                                let caller_region_frame = self
                                    .fiber
                                    .activation_region_maps
                                    .last()
                                    .cloned()
                                    .unwrap_or_default();
                                // Caller frame: on resume, the callee's return value
                                // flows as current_value and must be pushed as the Call
                                // instruction's result. The JIT callee runs without an
                                // interpreter region frame; `caller_region_frame` (=
                                // `activation_region_maps.last()`) is the caller's.
                                // MOVE the caller's owner node into its park — this
                                // activation unwinds with the suspending signal
                                // (docs/impl/region/owner.md § "Owner nodes").
                                let caller_owner_node = self.take_activation_owner_node();
                                // The JIT callee suspended without entering an
                                // interpreter activation, so `current_closure` is
                                // still this caller's — park it for the continuation.
                                let caller_closure = self.fiber.current_closure;
                                let caller_frame =
                                    SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                                        code.clone(),
                                        closure_env.clone(),
                                        *ip,
                                        caller_stack,
                                        true,
                                        caller_region_frame,
                                        caller_owner_node,
                                        caller_closure,
                                        self.heap(),
                                    ));
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

            // The closure call's env allocations — capture cells, rest-arg
            // conses, the `&keys` struct, captured-local cells — each get their
            // OWN fresh physical region inside `populate_env` (see
            // `env_value_region`). There is no shared per-call "env region" to
            // commingle (Rule 6) or leak; each env value mints its own fresh
            // region.
            let new_env_rc = match self.build_closure_env(closure, &args) {
                Some(env) => env,
                None => {
                    self.fiber.call_depth -= 1;
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            };

            // Extract squelch_mask before execute_bytecode_saving_stack to avoid
            // borrow lifetime conflicts: `closure` borrows from `func`, and we
            // need `closure_squelch_mask` after the call returns.
            let closure_squelch_mask = closure.squelch_mask;

            // Guard: WASM-compiled closures have empty bytecode. They
            // cannot be executed by the bytecode VM.
            if closure.template.bytecode.is_empty() {
                let err =
                    self.escaping_error("exec-error", "cannot execute WASM closure in bytecode VM");
                self.fiber.stack.push(err);
                self.fiber.call_depth -= 1;
                return Some(SIG_ERROR);
            }

            // Execute the closure, saving/restoring the caller's stack.
            // Essential for fiber/signal propagation and yield-through-nested-calls.
            // The per-activation region frame is pushed/popped inside
            // `execute_bytecode_saving_stack`.
            // Hand the callee its executing-closure register via the one-shot:
            // `execute_bytecode_saving_stack` installs it for the body and restores
            // the caller's on return, so a self-edge in the body resolves to `func`.
            self.pending_entry_closure = func;
            let result = self.execute_bytecode_saving_stack(&closure.template.code(), &new_env_rc);

            self.fiber.call_depth -= 1;

            let bits = result.bits;

            // Silence enforcement: if the closure declared (silence) and
            // the body produced ANY signal, that's a purity violation.
            // The programmer asserted purity — any signal (error, yield,
            // I/O) is a programmer bug. Abort with a clear diagnostic.
            if closure.template.signal.bits.is_empty()
                && closure.template.signal.propagates == 0
                && self.fiber.signal.as_ref().is_some_and(|(b, _)| !b.is_ok())
            {
                let (sig_bits, sig_val) = self.fiber.signal.take().unwrap();
                let name = closure.template.name.as_deref().unwrap_or("<anonymous>");
                let reg = crate::signals::registry::global_registry().lock().unwrap();
                eprintln!("panic: silence violation in '{}'", name);
                eprintln!("  A (silence)'d function signaled at runtime.");
                eprintln!("  silence asserts purity — any signal is a programmer bug.");
                eprintln!("  signal: {}", reg.format_signal_bits(sig_bits));
                eprintln!("  value:  {}", sig_val);
                if let Some(loc) = self.error_loc.as_ref() {
                    eprintln!("  at {}", loc);
                }
                std::process::abort();
            }

            // Squelch enforcement: if the closure has a squelch mask and the callee
            // returned a non-OK, non-error, non-halt signal that matches the mask,
            // convert to a signal-violation error.
            //
            // We do NOT intercept SIG_ERROR (already an error) or SIG_HALT (terminal).
            // We DO intercept SIG_YIELD and user-defined signals.
            //
            // Note: do_fiber_first_resume is intentionally exempt — fiber root bodies
            // execute outside any call_inner, so squelch enforcement does not apply
            // to the initial fiber execution.
            //
            // Discard suspended frames: we're converting to error, not suspending.
            if self.enforce_squelch(bits, closure_squelch_mask) {
                self.fiber.call_stack.pop();
                return Some(SIG_ERROR);
            }
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
                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::CALL)
                        && caller_stack.len() <= 5
                    {
                        eprintln!(
                            "[call_inner suspend] ip={} bc_len={} stack_depth={}",
                            *ip,
                            code.bytecode.len(),
                            caller_stack.len(),
                        );
                        for (si, sv) in caller_stack.iter().enumerate() {
                            eprintln!("  stack[{}] = {} {:?}", si, sv.type_name(), sv);
                        }
                    }
                    // The callee's `saving_stack` already popped its frame, so
                    // `activation_region_maps.last()` is now the caller's activation.
                    let caller_region_frame = self
                        .fiber
                        .activation_region_maps
                        .last()
                        .cloned()
                        .unwrap_or_default();
                    // MOVE the caller's owner node into its park — this activation
                    // unwinds with the suspending signal
                    // (docs/impl/region/owner.md § "Owner nodes").
                    let caller_owner_node = self.take_activation_owner_node();
                    // `saving_stack` restored `current_closure` to this caller on the
                    // callee's suspending return, so park the caller's value here; the
                    // callee's value rode out in `result.current_closure` (below).
                    let caller_closure = self.fiber.current_closure;
                    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                        code.clone(),
                        closure_env.clone(),
                        *ip,
                        caller_stack,
                        true,
                        caller_region_frame,
                        caller_owner_node,
                        caller_closure,
                        self.heap(),
                    ));

                    let mut frames = self.fiber.suspended.take().unwrap_or_default();

                    // When the callee was interrupted mid-execution by a
                    // non-yield signal (e.g. SIG_FUEL), the callee's inner
                    // frame lives in result.stack — not in fiber.suspended
                    // (only SIG_YIELD's handle_yield populates that).
                    // Without this, the callee's state is lost and resume
                    // injects nil as the Call's return value. Its region remap
                    // was captured into `result.activation_region_map` by
                    // saving_stack, its owner node (moved out the same way)
                    // into `result.activation_owner_node`.
                    if frames.is_empty() && !result.stack.is_empty() {
                        let inner = BytecodeFrame::suspend(
                            result.code,
                            result.env,
                            result.ip,
                            result.stack,
                            !bits.contains(SIG_FUEL),
                            result.activation_region_map,
                            result.activation_owner_node,
                            result.current_closure,
                            self.heap(),
                        );
                        frames.push(SuspendedFrame::Bytecode(inner));
                    }

                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::FIBER)
                    {
                        eprintln!(
                            "[call_inner] suspend: bits={} ip={} bc_len={} inner_frames={} env_len={}",
                            bits, *ip, code.bytecode.len(), frames.len(), closure_env.len(),
                        );
                    }
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

        // Callable collections: struct, array, set. Routed through
        // `dispatch_collection_call` for the per-execution region + Rule-5
        // pass-through retain (so a co-located/stored element survives the
        // collection's release under the consumer's borrow — the call-index UAF
        // family). The caller's `DecrefValueRegion` at the `(arr i)` decref_point
        // consumes that one owning reference, exactly as for a `get` result.
        if let Some(result) = self.dispatch_collection_call(&func, &args, region_id) {
            match result {
                Ok(value) => {
                    self.fiber.stack.push(value);
                    return None;
                }
                Err((kind, msg)) => {
                    self.set_error(kind, msg);
                    self.fiber.stack.push(Value::NIL);
                    return None;
                }
            }
        }

        // Cannot call this value
        eprintln!(
            "[DEBUG] Cannot call: tag={:#x} payload={:#x} type={} on_fiber_heap={}",
            func.tag,
            func.payload,
            func.type_name(),
            self.heap().value_in_region_store(func)
        );
        self.set_error("type-error", format!("Cannot call {:?}", func));
        self.fiber.stack.push(Value::NIL);
        None
    }
}
