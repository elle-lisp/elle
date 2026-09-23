// audited: 2026-09-23
// The interpreter's Call-position dispatch by callee kind: native, parameter,
// closure and collection, behind the capability gate.
// docs/impl/vm.md
use super::*;

mod park;
mod tail;

impl VM {
    /// Shared Call/CallArrayMut logic after argument extraction.
    ///
    /// Dispatches native functions, parameters and collections, and enters a
    /// compiled closure. An interpreted closure gets its environment built here
    /// and goes to `run_dispatch` as a `PendingCall`.
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
            let blocked = self.capability_blocked(def, &args);
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
            // A terminal :error hands the payload to a catcher, whose read
            // consumes one reference this frame's own routes do not fund — so the
            // raise mints it here where the payload is an argument of this call,
            // the Call-position twin of the tail exit's mint (`tail_call_inner`).
            // The site's own payload retain, where the compiler took one, is left
            // standing for the continuation past this call: a restart replays it,
            // and a fiber nobody restarts reaches it through the frame's release
            // table. A HALT takes no mint, for the reason `handle_emit` takes none.
            // Minted before the handler, which is where this fiber stops being the
            // one whose frames hold the payload. The `is_empty` test keeps the
            // classification off the ordinary-completion path, which every native
            // call takes.
            if !bits.is_empty()
                && crate::signals::dispatch::classify(bits, &value)
                    == crate::signals::dispatch::SignalAction::Error
            {
                self.mint_raised_argument_delivery(args.as_slice(), value);
            }
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
                closure.template.name().unwrap_or("<anon>"),
                args.len()
            );
            // Resource exhaustion, which the analyzer cannot predict: past the
            // depth cap the call halts, and a halt passes every signal mask on
            // its way to the top level.
            if !self.enter_call_depth() {
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // Push call frame for stack traces. The callee's operand stack
            // starts empty, its caller's waiting in a `PausedCaller`, so the
            // frame base is 0.
            self.fiber.call_stack.push(CallFrame {
                callee: closure.template.code(),
                caller: code.clone(),
                ip: instr_ip,
                frame_base: 0,
            });

            // Validate argument count (skip if compiler verified)
            if !checked && !self.check_arity(&closure.template.arity(), args.len()) {
                self.fiber.call_depth -= 1;
                self.fiber.call_stack.pop();
                self.fiber.stack.push(Value::NIL);
                return None;
            }

            // GPU capability check: if this closure has been GIT'd (has SPIR-V),
            // it requires GPU hardware. Check capability before dispatch.
            if closure.template.spirv_bytes().is_some() {
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

            // A compiled callee runs as a native call nested on the Rust stack.
            // While that stack is low, the callee runs here instead, on fiber
            // frames (docs/impl/vm.md § "What still uses the Rust stack").
            #[cfg(any(feature = "jit", feature = "wasm", feature = "mlir"))]
            let compiled_room =
                !crate::vm::native_stack::below(crate::vm::native_stack::COMPILED_CALL_RESERVE);

            // Tiered WASM compilation and dispatch.
            // Checked before JIT because WASM is the preferred fast path when enabled.
            #[cfg(feature = "wasm")]
            if compiled_room && closure.template.lir_function().is_some() {
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
            if compiled_room && self.mlir_enabled && closure.template.lir_function().is_some() {
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
            if compiled_room && closure.template.lir_function().is_some() {
                if let Some(bits) = self.try_jit_call(closure, &args, func) {
                    self.fiber.call_depth -= 1;
                    self.fiber.call_stack.pop();
                    match bits {
                        Some(sig) if !sig.intersects(SIG_ERROR) && !sig.intersects(SIG_HALT) => {
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
                                // MOVE what the caller's activation owes into its park — this
                                // activation unwinds with the suspending signal
                                // (docs/impl/region/owner.md § "Owner nodes").
                                let caller_dues = self.take_activation_dues();
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
                                        caller_dues,
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

            // Guard: WASM-compiled closures have empty bytecode. They
            // cannot be executed by the bytecode VM.
            if closure.template.bytecode().is_empty() {
                let err =
                    self.escaping_error("exec-error", "cannot execute WASM closure in bytecode VM");
                self.fiber.stack.push(err);
                self.fiber.call_depth -= 1;
                return Some(SIG_ERROR);
            }

            // Hand the callee to the dispatch loop's driver, which pauses this
            // caller in the fiber and runs the callee on the same loop; the
            // callee's closure value becomes its executing-closure register
            // (docs/impl/vm.md § "Non-tail calls"). What completing the call
            // needs to know about the callee is read here, while `func` is
            // certainly live.
            let signal = closure.template.signal();
            self.pending_call = Some(crate::vm::core::PendingCall {
                code: closure.template.code(),
                env: new_env_rc,
                closure: func,
                call_ip: instr_ip,
                site: crate::value::fiber::CallSite {
                    squelch_mask: closure.squelch_mask,
                    silent: signal.bits.is_empty() && signal.propagates == 0,
                    name: closure.template.name(),
                },
            });
            return Some(SIG_OK);
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
        self.set_error(
            "type-error",
            format!("Cannot call {}", self.show_value(func)),
        );
        self.fiber.stack.push(Value::NIL);
        None
    }
}
