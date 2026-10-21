// audited: 2026-10-21
//! `vm/config` and `vm/config-set` over the VM's runtime configuration.
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
            "home",
            "path",
            "plugin-suffix",
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
            "home" => match &crate::config::get().home {
                Some(home) => ctx.string(home),
                None => Value::NIL,
            },
            "path" => Self::search_path_value(ctx),
            "plugin-suffix" => ctx.string(std::env::consts::DLL_EXTENSION),
            _ => return None,
        })
    }

    /// The depth cap as an Elle integer. The setter admits only positive
    /// integers, so the cap always fits.
    fn max_depth_value(max_depth: usize) -> Value {
        Value::int(i64::try_from(max_depth).unwrap_or(i64::MAX))
    }

    /// The `--path` entries as an array of strings, in order. An empty entry
    /// names nothing, so it is dropped. `--path` and `ELLE_PATH` are set once
    /// per process, so every VM answers the same array.
    fn search_path_value(ctx: &mut crate::primitives::ctx::Alloc) -> Value {
        let entries = crate::config::get()
            .path
            .as_deref()
            .unwrap_or("")
            .split(':')
            .filter(|entry| !entry.is_empty())
            .map(|entry| ctx.string(entry))
            .collect();
        ctx.array(entries)
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
}
