use super::*;

impl VM {
    /// Handle `(vm/config)` read — returns config struct or specific field.
    pub(super) fn dispatch_vm_config_read(
        &self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        use crate::value::TableKey;
        use std::collections::BTreeMap;

        let rc = &self.runtime_config;

        if arg.is_nil() {
            // Full config struct
            let mut map = BTreeMap::new();
            map.insert(
                TableKey::from_value(&Value::keyword("jit")).unwrap(),
                Value::keyword(rc.jit.keyword()),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("wasm")).unwrap(),
                Value::keyword(rc.wasm.keyword()),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("mlir")).unwrap(),
                Value::keyword(rc.mlir.keyword()),
            );
            // trace as a set of keywords
            let trace_set: Vec<Value> = rc.trace.iter().map(|k| Value::keyword(k)).collect();
            map.insert(
                TableKey::from_value(&Value::keyword("trace")).unwrap(),
                ctx.set(trace_set.into_iter().collect()),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("stats")).unwrap(),
                Value::bool(rc.stats),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("debug-bytecode")).unwrap(),
                Value::bool(rc.debug_bytecode),
            );
            map.insert(
                TableKey::from_value(&Value::keyword("flip")).unwrap(),
                Value::bool(crate::config::flip_enabled()),
            );
            (SIG_OK, ctx.struct_from(map))
        } else if let Some(kw) = arg.as_keyword_name() {
            match kw.as_str() {
                "jit" => (SIG_OK, Value::keyword(rc.jit.keyword())),
                "wasm" => (SIG_OK, Value::keyword(rc.wasm.keyword())),
                "mlir" => (SIG_OK, Value::keyword(rc.mlir.keyword())),
                "trace" => {
                    let trace_set: Vec<Value> =
                        rc.trace.iter().map(|k| Value::keyword(k)).collect();
                    (SIG_OK, ctx.set(trace_set.into_iter().collect()))
                }
                "stats" => (SIG_OK, Value::bool(rc.stats)),
                "flip" => (SIG_OK, Value::bool(crate::config::flip_enabled())),
                _ => (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!("vm/config: unknown field :{}", kw),
                    ),
                ),
            }
        } else {
            type_error!(ctx, arg, "vm/config", "keyword or nil")
        }
    }
    /// Handle `(vm/config-set key value)` — mutates the VM's RuntimeConfig.
    pub(super) fn handle_vm_config_set(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> Value {
        let pair = match arg.as_pair() {
            Some(c) => c,
            None => return ctx.error("type-error", "vm/config-set: expected (key . value)"),
        };
        let key = pair.first;
        let val = pair.rest;

        let kw = match key.as_keyword_name() {
            Some(k) => k,
            None => {
                return ctx.error(
                    "type-error",
                    format!(
                        "vm/config-set: key must be a keyword, got {}",
                        key.type_name()
                    ),
                )
            }
        };

        match kw.as_str() {
            "jit" => {
                if let Some(closure) = val.as_closure() {
                    let _ = closure; // TODO: store for actual dispatch
                    self.runtime_config.jit = crate::config::JitPolicy::Custom;
                } else if let Some(policy_kw) = val.as_keyword_name() {
                    match crate::config::JitPolicy::from_keyword(&policy_kw) {
                        Some(policy) => {
                            self.runtime_config.jit = policy;
                        }
                        None => {
                            return ctx.error(
                                "argument-error",
                                format!("vm/config-set :jit: unknown policy :{}", policy_kw),
                            )
                        }
                    }
                } else {
                    return ctx.error(
                        "type-error",
                        format!(
                            "vm/config-set :jit: expected keyword or closure, got {}",
                            val.type_name()
                        ),
                    );
                }
                Value::NIL
            }
            "wasm" => {
                if let Some(policy_kw) = val.as_keyword_name() {
                    match crate::config::WasmPolicy::from_keyword(&policy_kw) {
                        Some(policy) => {
                            self.runtime_config.wasm = policy;
                        }
                        None => {
                            return ctx.error(
                                "argument-error",
                                format!("vm/config-set :wasm: unknown policy :{}", policy_kw),
                            )
                        }
                    }
                } else {
                    return ctx.error(
                        "type-error",
                        format!(
                            "vm/config-set :wasm: expected keyword, got {}",
                            val.type_name()
                        ),
                    );
                }
                Value::NIL
            }
            "mlir" => {
                if let Some(policy_kw) = val.as_keyword_name() {
                    match crate::config::MlirPolicy::from_keyword(&policy_kw) {
                        Some(policy) => {
                            #[cfg(feature = "mlir")]
                            {
                                self.mlir_enabled = policy.enabled();
                            }
                            self.runtime_config.mlir = policy;
                        }
                        None => {
                            return ctx.error(
                                "argument-error",
                                format!("vm/config-set :mlir: unknown policy :{}", policy_kw),
                            )
                        }
                    }
                } else {
                    return ctx.error(
                        "type-error",
                        format!(
                            "vm/config-set :mlir: expected keyword, got {}",
                            val.type_name()
                        ),
                    );
                }
                Value::NIL
            }
            "trace" => {
                // Accept a set of keywords
                if let Some(set) = val.as_set() {
                    let mut keywords = std::collections::HashSet::new();
                    for v in set.iter() {
                        if let Some(k) = v.as_keyword_name() {
                            keywords.insert(k);
                        }
                    }
                    self.runtime_config.set_trace(keywords);
                } else {
                    return ctx.error(
                        "type-error",
                        format!(
                            "vm/config-set :trace: expected set, got {}",
                            val.type_name()
                        ),
                    );
                }
                Value::NIL
            }
            "stats" => {
                self.runtime_config.stats = val.is_truthy();
                Value::NIL
            }
            // Legacy: flip is always off (no-op). Accept for compat.
            "flip" => Value::NIL,
            _ => ctx.error(
                "argument-error",
                format!("vm/config-set: unknown field :{}", kw),
            ),
        }
    }
    /// Handle `arena/allocs` — snapshot count, call thunk, snapshot again.
    ///
    /// Runs the thunk through [`VM::run_thunk_to_completion`] (re-entrant VM
    /// call that drives the `fiber/resume` `SIG_SWITCH` trampoline), so a thunk
    /// that spawns and resumes fibers is measured to completion — the resume's
    /// allocations fall between the two snapshots and `(result . net)` carries
    /// the thunk's real result, not the resumed child's value (the
    /// `fiber-spawn-10` regression, `tests/elle/arena.lisp` /
    /// `tests/elle/resource.lisp`). The thunk must still be non-*yielding* (it
    /// must not suspend its own caller). Returns `(SIG_OK, pair(result, net))`
    /// on success, or `(SIG_ERROR, err)` / the propagated signal on failure.
    pub(super) fn handle_arena_allocs(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        thunk: Value,
    ) -> (SignalBits, Value) {
        let closure = match thunk.as_closure() {
            Some(c) => c.clone(),
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "arena/allocs: expected a closure"),
                );
            }
        };

        let before = unsafe { (*self.heap_ptr).visible_len() };

        let thunk_env = self
            .build_closure_env(&closure, &[])
            .expect("arena/allocs: zero-arg thunk env build cannot fail");

        // Hand the thunk its executing-closure register via the one-shot — the
        // measured-thunk entry runs a closure body like any other entrant.
        self.pending_entry_closure = thunk;
        let bits = self.run_thunk_to_completion(&closure.template.code(), &thunk_env);

        if !bits.is_empty() {
            // Propagate the error/signal — fiber.signal is already set by inner execution.
            let (sig, val) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            return (sig, val);
        }

        let result = self
            .fiber
            .signal
            .take()
            .map(|(_, v)| v)
            .unwrap_or(Value::NIL);

        let after = unsafe { (*self.heap_ptr).visible_len() };

        let net = (after as i64) - (before as i64);
        (SIG_OK, ctx.pair(result, Value::int(net)))
    }
}
