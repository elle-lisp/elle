use super::*;

impl Config {
    /// Parse CLI arguments into a Config and remaining positional args.
    ///
    /// Returns `(config, subcommand_or_none, remaining_args)`.
    /// `remaining_args` contains file args and everything after `--`.
    pub fn parse(args: &[String]) -> Result<(Config, Vec<String>), String> {
        // CLI effective defaults (distinct from the struct `Default`, which is
        // the library/test baseline). The command-line tool runs the sound
        // native-Call path by default: checked intrinsics ON, which forces the
        // optimizing tiers OFF (the inlined opcode / JIT accessors are not
        // escape-correct). Override with `--checked-intrinsics=off` or an
        // explicit `--jit`/`--mlir`.
        let mut config = Config {
            checked_intrinsics: true,
            jit: JitPolicy::Off,
            mlir: MlirPolicy::Off,
            ..Default::default()
        };
        let mut remaining = Vec::new();
        let mut i = 0;
        let mut eval_exprs: Vec<String> = Vec::new();
        // Track whether the optimizing tiers / checked-intrinsics were set
        // *explicitly* on the command line (vs. left at the default). The
        // post-loop normalization uses this to resolve the checked-vs-jit/mlir
        // conflict: an explicit `--jit`/`--mlir` enable wins over default-on
        // checked, while two explicit, contradictory flags still error.
        let mut jit_explicit = false;
        let mut mlir_explicit = false;
        let mut checked_explicit_on = false;

        while i < args.len() {
            let arg = &args[i];

            if arg == "--" {
                // Everything after -- goes to user args
                remaining.push("--".to_string());
                remaining.extend_from_slice(&args[i + 1..]);
                break;
            }

            // --key=value style
            if let Some(rest) = arg.strip_prefix("--jit=") {
                jit_explicit = true;
                config.jit = match rest {
                    "off" => JitPolicy::Off,
                    "eager" => JitPolicy::Eager,
                    "adaptive" => JitPolicy::Adaptive { threshold: 10 },
                    _ => {
                        let n: u32 = rest.parse().map_err(|_| {
                            format!(
                                "--jit: expected integer or policy name (off/eager/adaptive), got '{}'",
                                rest
                            )
                        })?;
                        if n == 0 {
                            JitPolicy::Off
                        } else if n == 1 {
                            JitPolicy::Eager
                        } else {
                            JitPolicy::Adaptive {
                                threshold: (n - 1) as usize,
                            }
                        }
                    }
                };
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--mlir=") {
                mlir_explicit = true;
                config.mlir = match rest {
                    "off" => MlirPolicy::Off,
                    "eager" => MlirPolicy::Eager,
                    "adaptive" => MlirPolicy::Adaptive { threshold: 10 },
                    _ => {
                        let n: u32 = rest.parse().map_err(|_| {
                            format!(
                                "--mlir: expected integer or policy name (off/eager/adaptive), got '{}'",
                                rest
                            )
                        })?;
                        if n == 0 {
                            MlirPolicy::Off
                        } else if n == 1 {
                            MlirPolicy::Eager
                        } else {
                            MlirPolicy::Adaptive {
                                threshold: (n - 1) as usize,
                            }
                        }
                    }
                };
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--wasm=") {
                config.wasm = match rest {
                    "off" => WasmPolicy::Off,
                    "full" => WasmPolicy::Full,
                    "lazy" => WasmPolicy::Lazy { threshold: 10 },
                    _ => {
                        let n: u32 = rest.parse().map_err(|_| {
                            format!(
                                "--wasm: expected integer or policy name (off/full/lazy), got '{}'",
                                rest
                            )
                        })?;
                        if n == 0 {
                            WasmPolicy::Off
                        } else {
                            WasmPolicy::Lazy {
                                threshold: (n - 1) as usize,
                            }
                        }
                    }
                };
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--anf=") {
                config.anf = match rest {
                    "on" | "true" | "1" => true,
                    "off" | "false" | "0" => false,
                    _ => {
                        return Err(format!(
                            "--anf: expected on/off (or true/false, 1/0), got '{}'",
                            rest
                        ));
                    }
                };
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--checked-intrinsics=") {
                match rest {
                    "on" | "true" | "1" => {
                        config.checked_intrinsics = true;
                        checked_explicit_on = true;
                        // jit/mlir forced off in the post-loop normalization.
                    }
                    "off" | "false" | "0" => {
                        config.checked_intrinsics = false;
                        // Restore the optimizing-tier defaults this flag would
                        // otherwise have forced off, unless the user set them
                        // explicitly (don't clobber an explicit `--jit`/`--mlir`).
                        if !jit_explicit {
                            config.jit = JitPolicy::Adaptive { threshold: 10 };
                        }
                        if !mlir_explicit {
                            config.mlir = MlirPolicy::Adaptive { threshold: 10 };
                        }
                    }
                    _ => {
                        return Err(format!(
                            "--checked-intrinsics: expected on/off (or true/false, 1/0), got '{}'",
                            rest
                        ));
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--flip=") {
                // Legacy: accepted for backwards compat but has no effect.
                match rest {
                    "on" | "true" | "1" | "off" | "false" | "0" => {}
                    _ => {
                        return Err(format!("--flip: expected on/off, got '{}'", rest));
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--trace=") {
                if rest == "all" {
                    for kw in TRACE_KEYWORDS {
                        config.trace_keywords.push(kw.to_string());
                    }
                } else {
                    for kw in rest.split(',') {
                        let kw = kw.trim();
                        if !kw.is_empty() {
                            config.trace_keywords.push(kw.to_string());
                        }
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--dump=") {
                if rest == "all" {
                    for kw in DUMP_KEYWORDS {
                        config.dump.insert((*kw).to_string());
                    }
                } else {
                    for kw in rest.split(',') {
                        let kw = kw.trim();
                        if kw.is_empty() {
                            continue;
                        }
                        if dump_bits::from_name(kw) == 0 {
                            return Err(format!(
                                "--dump: unknown stage '{}'. Valid: {}",
                                kw,
                                DUMP_KEYWORDS.join(", ")
                            ));
                        }
                        config.dump.insert(kw.to_string());
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--cache=") {
                config.cache = if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                };
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--region-page-size=") {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("--region-page-size: expected integer, got '{}'", rest))?;
                if n < 4096 || !n.is_power_of_two() {
                    return Err(format!(
                        "--region-page-size: must be a power of two >= 4096, got {}",
                        n
                    ));
                }
                config.region_page_size = n;
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--page-pool-max=") {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("--page-pool-max: expected integer, got '{}'", rest))?;
                config.page_pool_max = n;
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--home=") {
                config.home = Some(rest.to_string());
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--path=") {
                config.path = Some(rest.to_string());
                i += 1;
                continue;
            }

            // Boolean flags
            match arg.as_str() {
                "--json" => config.json = true,
                "--stats" => config.stats = true,
                "--wasm-no-stdlib" | "--no-stdlib" => config.no_stdlib = true,
                "--no-uring" => config.no_uring = true,
                // Old debug flags — kept as aliases for --trace=<kw>
                "--debug" => config.trace_keywords.push("bytecode".into()),
                "--debug-jit" => config.trace_keywords.push("jit".into()),
                "--debug-resume" => config.trace_keywords.push("fiber".into()),
                "--debug-stack" => config.trace_keywords.push("call".into()),
                "--debug-wasm" => config.trace_keywords.push("wasm".into()),
                "--wasm-dump" => config.wasm_dump = true,
                "--wasm-lir" => config.wasm_lir = true,
                "--wasm-chunk" => config.wasm_chunk = true,
                "--wasm-no-sparse-spill" => config.wasm_sparse_spill = false,
                "--checked-intrinsics" => {
                    config.checked_intrinsics = true;
                    checked_explicit_on = true;
                    // jit/mlir forced off in the post-loop normalization.
                }
                "--region-ownership" => {
                    // The ownership forest (docs/impl/region-model.md § "Adoption
                    // and subtree drop"). Runs on BOTH intrinsics settings: the
                    // adopt is emitted at the intrinsic containment store
                    // checked-off, and at the funnel call site checked-on (the
                    // funnel face — region-model.md § "The funnel adopt"). The
                    // adopt/group ops are lowered on VM and JIT, so only MLIR/WASM
                    // are forced off, in the post-loop normalization.
                    config.region_ownership = true;
                }
                "--eval" | "-e" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--eval requires an argument".to_string());
                    }
                    eval_exprs.push(args[i].clone());
                }
                _ => {
                    // Not a recognized flag — pass through as positional
                    remaining.push(arg.clone());
                }
            }

            i += 1;
        }

        // Prepend eval expressions as synthetic file args
        // They'll be handled specially in main
        for expr in eval_exprs.into_iter().rev() {
            remaining.insert(0, format!("--eval:{}", expr));
        }

        // Resolve checked-intrinsics vs the optimizing tiers. checked-intrinsics
        // is now default-ON (the sound native-Call path under move), and it
        // requires JIT and MLIR off (they would bypass the type checks / inline
        // escape-incorrect opcodes). Resolution:
        //  - an explicit `--jit`/`--mlir` *enable* wins over default-on checked
        //    (turns checked off), so existing `--jit=…` invocations keep working;
        //  - an explicit `--checked-intrinsics` together with an explicit enabled
        //    `--jit`/`--mlir` is a genuine contradiction and still errors;
        //  - otherwise checked (default-on or explicit) forces both tiers off.
        if jit_explicit && config.jit.enabled() {
            if checked_explicit_on {
                return Err(
                    "--checked-intrinsics is incompatible with --jit (JIT would bypass type checks)"
                        .to_string(),
                );
            }
            config.checked_intrinsics = false;
        }
        if mlir_explicit && config.mlir.enabled() {
            if checked_explicit_on {
                return Err(
                    "--checked-intrinsics is incompatible with --mlir (MLIR would bypass type checks)"
                        .to_string(),
                );
            }
            config.checked_intrinsics = false;
        }
        if config.checked_intrinsics {
            config.jit = JitPolicy::Off;
            config.mlir = MlirPolicy::Off;
        }
        // The ownership forest runs on BOTH intrinsics settings — the adopt is
        // emitted at the intrinsic containment store checked-off, and at the
        // funnel call site checked-on (the funnel face; region-model.md § "The
        // funnel adopt — the checked-on store face") — so `--region-ownership`
        // leaves `--checked-intrinsics` as configured (the production default,
        // on). The `AdoptRegion`/`FreeRegionGroup` ops are lowered on the JIT
        // (`elle_jit_adopt_region`/`elle_jit_free_region_group`) as well as the
        // interpreter, so JIT is left as configured too — VM≡JIT parity, the
        // precondition for the flag ever defaulting on. Only MLIR/WASM still trail
        // their structural-arena handling (step 5) and are forced off.
        if config.region_ownership {
            config.mlir = MlirPolicy::Off;
            config.wasm = WasmPolicy::Off;
        }

        Ok((config, remaining))
    }
}
