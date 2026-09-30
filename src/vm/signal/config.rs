// audited: 2026-09-29
//! `vm/config` and `vm/config-set` over the VM's runtime configuration, and
//! the `arena/allocs` measurement.
//!
//! docs/config.md

use super::*;

/// What `vm/config-set` asks of a tier: off, compile on the first call, or
/// compile after that many calls.
enum TierSetting {
    Off,
    Eager,
    After(usize),
}

impl VM {
    /// The trace keys as a keyword set.
    ///
    /// Each key already has a spelling: `--trace` admits only
    /// `TRACE_KEYWORDS`, which the vocabulary carries, and `vm/config-set`
    /// keeps only a key that resolves (docs/impl/symbol.md).
    fn trace_keywords(&self, ctx: &mut crate::primitives::ctx::Alloc) -> Value {
        let set: std::collections::BTreeSet<Value> = self
            .runtime_config
            .trace
            .iter()
            .map(|k| Value::keyword(k))
            .collect();
        ctx.set(set)
    }

    /// Handle `(vm/config)` read — returns config struct or specific field.
    ///
    /// A tier threshold reads nil when the tier is off, 0 when it compiles on
    /// the first call, and the count otherwise. `:wasm` exists only in a build
    /// that carries the WebAssembly backend (docs/config.md).
    pub(super) fn dispatch_vm_config_read(
        &self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        use crate::value::TableKey;
        use std::collections::BTreeMap;

        const FIELDS: &[&str] = &[
            "jit",
            "mlir",
            #[cfg(feature = "wasm")]
            "wasm",
            "trace",
            "stats",
            "debug-bytecode",
            "unicode",
            "max-depth",
        ];

        if arg.is_nil() {
            let mut map = BTreeMap::new();
            for field in FIELDS {
                let value = self.config_field(ctx, field).expect("a listed field");
                map.insert(
                    TableKey::from_value(&Value::keyword(field)).expect("a keyword key"),
                    value,
                );
            }
            (SIG_OK, ctx.struct_from(map))
        } else if let Some(kw) = self.keyword_spelling(arg) {
            match FIELDS
                .contains(&kw.as_str())
                .then(|| self.config_field(ctx, &kw))
                .flatten()
            {
                Some(value) => (SIG_OK, value),
                None => (
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

    /// The value `(vm/config :field)` reads, or `None` for a field this build
    /// does not have.
    fn config_field(&self, ctx: &mut crate::primitives::ctx::Alloc, field: &str) -> Option<Value> {
        let rc = &self.runtime_config;
        let threshold = |reading: Option<usize>| match reading {
            Some(n) => Value::int(i64::try_from(n).unwrap_or(i64::MAX)),
            None => Value::NIL,
        };
        Some(match field {
            "jit" => threshold(rc.jit.reading()),
            "mlir" => threshold(rc.mlir.reading()),
            #[cfg(feature = "wasm")]
            "wasm" => Value::keyword(rc.wasm.keyword()),
            "trace" => self.trace_keywords(ctx),
            "stats" => Value::bool(rc.stats),
            "debug-bytecode" => Value::bool(rc.debug_bytecode),
            "unicode" => self.unicode_version_value(ctx),
            "max-depth" => Self::max_depth_value(rc.max_depth),
            _ => return None,
        })
    }

    /// The depth cap as an Elle integer. The setter admits only positive
    /// integers, so the cap always fits.
    fn max_depth_value(max_depth: usize) -> Value {
        Value::int(i64::try_from(max_depth).unwrap_or(i64::MAX))
    }

    /// The VM's Unicode generation as a `[major minor patch]` array.
    fn unicode_version_value(&self, ctx: &mut crate::primitives::ctx::Alloc) -> Value {
        let (major, minor, patch) = self.unicode_generation.version();
        ctx.array(vec![
            Value::int(major as i64),
            Value::int(minor as i64),
            Value::int(patch as i64),
        ])
    }

    /// Handle `(vm/config-set key value)` — mutates the VM's RuntimeConfig.
    ///
    /// Every refusal raises: the error comes back as `SIG_ERROR`, so a program
    /// that sets a field it may not set stops rather than reading the error
    /// struct as a result (docs/config.md).
    pub(super) fn handle_vm_config_set(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        match self.config_set(arg) {
            Ok(()) => (SIG_OK, Value::NIL),
            Err((kind, msg)) => (SIG_ERROR, ctx.error(kind, msg)),
        }
    }

    fn config_set(&mut self, arg: Value) -> Result<(), (&'static str, String)> {
        let pair = arg.as_pair().ok_or((
            "type-error",
            "vm/config-set: expected (key . value)".to_string(),
        ))?;
        let (key, val) = (pair.first, pair.rest);
        let kw = self.keyword_spelling(key).ok_or_else(|| {
            (
                "type-error",
                format!(
                    "vm/config-set: key must be a keyword, got {}",
                    key.type_name()
                ),
            )
        })?;

        match kw.as_str() {
            "jit" => {
                use crate::config::JitPolicy;
                let on = self.runtime_config.jit.enabled();
                self.runtime_config.jit =
                    match self.tier_arg(&kw, val, on, cfg!(feature = "jit"))? {
                        TierSetting::Off => JitPolicy::Off,
                        TierSetting::Eager => JitPolicy::Eager,
                        TierSetting::After(threshold) => JitPolicy::Adaptive { threshold },
                    };
            }
            "mlir" => {
                use crate::config::MlirPolicy;
                let on = self.runtime_config.mlir.enabled();
                let policy = match self.tier_arg(&kw, val, on, cfg!(feature = "mlir"))? {
                    TierSetting::Off => MlirPolicy::Off,
                    TierSetting::Eager => MlirPolicy::Eager,
                    TierSetting::After(threshold) => MlirPolicy::Adaptive { threshold },
                };
                // The call path reads this beside the policy.
                #[cfg(feature = "mlir")]
                {
                    self.mlir_enabled = policy.enabled();
                }
                self.runtime_config.mlir = policy;
            }
            "trace" => {
                let set = val.as_set().ok_or_else(|| {
                    (
                        "type-error",
                        format!(
                            "vm/config-set :trace: expected set, got {}",
                            val.type_name()
                        ),
                    )
                })?;
                let keywords = set
                    .iter()
                    .filter_map(|v| self.keyword_spelling(*v))
                    .collect();
                self.runtime_config.set_trace(keywords);
            }
            "stats" => self.runtime_config.stats = val.is_truthy(),
            "max-depth" => match val.as_int() {
                Some(n) if n > 0 => {
                    self.runtime_config.max_depth = usize::try_from(n).unwrap_or(usize::MAX);
                }
                Some(n) => {
                    return Err((
                        "argument-error",
                        format!("vm/config-set :max-depth: expected a positive integer, got {n}"),
                    ))
                }
                None => {
                    return Err((
                        "type-error",
                        format!(
                            "vm/config-set :max-depth: expected integer, got {}",
                            val.type_name()
                        ),
                    ))
                }
            },
            "unicode" => {
                return Err((
                    "argument-error",
                    "vm/config-set :unicode: the Unicode generation is fixed at VM construction"
                        .to_string(),
                ))
            }
            _ => {
                return Err((
                    "argument-error",
                    format!("vm/config-set: unknown field :{}", kw),
                ))
            }
        }
        Ok(())
    }

    /// A tier setting: a positive threshold, or, in the process that runs
    /// `elle test`, `:off` or `:eager` (docs/config.md). Any other process is
    /// refused both as an argument error; any other keyword is no setting at
    /// all, and reaches [`Self::threshold_arg`] as the wrong type.
    ///
    /// `tier_on` is whether this run has the tier on, and `carried` whether
    /// the build compiles it in. The runner turns a tier off itself, so there a
    /// threshold needs only `carried`: that is how it puts back what it read.
    fn tier_arg(
        &self,
        field: &str,
        val: Value,
        tier_on: bool,
        carried: bool,
    ) -> Result<TierSetting, (&'static str, String)> {
        let runner = crate::config::get().test_runner;
        let setting = match self.keyword_spelling(val).as_deref() {
            Some("off") => TierSetting::Off,
            Some("eager") => TierSetting::Eager,
            _ => {
                return Self::threshold_arg(field, val, tier_on || (runner && carried))
                    .map(TierSetting::After)
            }
        };
        if !runner {
            return Err((
                "argument-error",
                format!(
                    "vm/config-set :{field}: only elle test turns a tier off or makes it eager"
                ),
            ));
        }
        if matches!(setting, TierSetting::Eager) && !carried {
            return Err((
                "argument-error",
                format!("vm/config-set :{field}: this build carries no {field} tier"),
            ));
        }
        Ok(setting)
    }

    /// A tier threshold a program may set: a positive integer, for a tier this
    /// run has on. The type is checked first, so a keyword is a type error
    /// whether or not the tier is on.
    fn threshold_arg(
        field: &str,
        val: Value,
        tier_on: bool,
    ) -> Result<usize, (&'static str, String)> {
        let n = val.as_int().ok_or_else(|| {
            (
                "type-error",
                format!(
                    "vm/config-set :{field}: expected a positive integer, got {}",
                    val.type_name()
                ),
            )
        })?;
        if n < 1 {
            return Err((
                "argument-error",
                format!("vm/config-set :{field}: expected a positive integer, got {n}"),
            ));
        }
        if !tier_on {
            return Err((
                "argument-error",
                format!("vm/config-set :{field}: this run has no {field} tier to set"),
            ));
        }
        Ok(usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// Handle `arena/allocs` — snapshot count, call thunk, snapshot again.
    ///
    /// Runs the thunk through [`VM::run_thunk_to_completion`] (re-entrant VM
    /// call that drives the `fiber/resume` `SIG_SWITCH` trampoline), so a thunk
    /// that spawns and resumes fibers is measured to completion — the resume's
    /// allocations fall between the two snapshots and `(result . net)` carries
    /// the thunk's real result, not the resumed child's value
    /// (`tests/impl/arena.lisp`, the `fiber-spawn-10` scenario in
    /// `tests/impl/resource.lisp`). The thunk must still be non-*yielding* (it
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
            // Propagate the error/signal — fiber.signal is already set by inner
            // execution. A suspend-class park leaves as a value, abandoned with
            // its host.
            self.abandon_hosted_park(bits);
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
        let answer = ctx.pair(result, Value::int(net));
        // The thunk's result left its compiled body through the return
        // convention carrying one owed reference — this boundary's to consume,
        // since the pair (fresh in the call's own region) is the value the
        // caller releases, not the result inside it. The pair's alloc-time
        // scan already counted the embedding, so consuming the mint after the
        // pair is built leaves the result held by the pair and freed by its
        // cascade (the `allocs-result` oracle probe).
        let heap = unsafe { &mut *self.heap_ptr };
        let region = crate::value::arena::region_of(heap, result);
        if let Some(region) = region {
            heap.decref_region_if_present(region);
        }
        (SIG_OK, answer)
    }
}
