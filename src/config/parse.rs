use super::*;

impl Config {
    /// Parse CLI arguments into a Config and remaining positional args.
    ///
    /// Returns `(config, subcommand_or_none, remaining_args)`.
    /// `remaining_args` contains file args and everything after `--`.
    pub fn parse(args: &[String]) -> Result<(Config, Vec<String>), String> {
        // CLI effective defaults (distinct from the struct `Default`, which is
        // the library/test baseline): the optimizing tiers are opt-in
        // (`--jit`/`--mlir`).
        let mut config = Config {
            jit: JitPolicy::Off,
            mlir: MlirPolicy::Off,
            ..Default::default()
        };
        let mut remaining = Vec::new();
        let mut i = 0;
        let mut eval_exprs: Vec<String> = Vec::new();

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
            if let Some(rest) = arg.strip_prefix("--unicode=") {
                let components: Result<Vec<i64>, _> =
                    rest.split('.').map(|c| c.parse::<i64>()).collect();
                let request = match components {
                    Ok(req) if !req.is_empty() && req.len() <= 3 && req.iter().all(|c| *c >= 0) => {
                        req
                    }
                    _ => {
                        return Err(format!(
                            "--unicode: expected MAJ[.MIN[.PATCH]] (for example 16.0), got '{}'",
                            rest
                        ))
                    }
                };
                let gen = crate::segment::Generation::from_request(&request)
                    .map_err(|e| format!("--unicode: {}", e))?;
                config.unicode = Some(gen);
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

        Ok((config, remaining))
    }
}
