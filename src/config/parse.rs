// audited: 2026-09-29
//! `Config::parse`: read elle's own flags off the front of an argv, and hand back
//! the program and its arguments. Every flag `elle --help` lists is recognized here.
//!
//! docs/config.md

use super::*;

impl Config {
    /// Parse CLI arguments into a Config and the program's arguments.
    ///
    /// Elle's flags stop at the program name: the first argument that is not a
    /// flag (a path, or `-` for stdin), or `--`. That argument and every one after
    /// it come back unread as the second element, so a flag after the program
    /// belongs to the program. `-e`/`--eval` expressions come back first, each as
    /// a `--eval:EXPR` entry. A leading `--X` that is no flag of this build is an
    /// error naming it: read as a program name it would run a missing file.
    pub fn parse(args: &[String]) -> Result<(Config, Vec<String>), String> {
        let mut config = Config::default();
        let mut remaining = Vec::new();
        let mut eval_exprs: Vec<String> = Vec::new();
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];
            if arg == "--" || arg == "-" || !arg.starts_with('-') {
                remaining.extend_from_slice(&args[i..]);
                break;
            }
            if let Some(expr) = arg.strip_prefix("--eval:") {
                eval_exprs.push(expr.to_string());
            } else if arg == "--eval" || arg == "-e" {
                i += 1;
                let expr = args.get(i).ok_or("--eval requires an argument")?;
                eval_exprs.push(expr.clone());
            } else if !config.parse_flag(arg)? {
                return Err(format!("unknown option {arg}"));
            }
            i += 1;
        }

        for expr in eval_exprs.into_iter().rev() {
            remaining.insert(0, format!("--eval:{}", expr));
        }
        Ok((config, remaining))
    }

    /// Apply one flag to the config. Answers whether the flag is one of elle's.
    fn parse_flag(&mut self, arg: &str) -> Result<bool, String> {
        if let Some(rest) = arg.strip_prefix("--trace=") {
            self.parse_trace(rest)?;
        } else if let Some(rest) = arg.strip_prefix("--dump=") {
            self.parse_dump(rest)?;
        } else if let Some(rest) = arg.strip_prefix("--cache=") {
            self.cache = (!rest.is_empty()).then(|| rest.to_string());
        } else if let Some(rest) = arg.strip_prefix("--boot-image=") {
            self.boot_image = match rest {
                "off" | "" => None,
                "on" => Some(String::new()),
                dir => Some(dir.to_string()),
            };
        } else if let Some(rest) = arg.strip_prefix("--unicode=") {
            self.unicode = Some(parse_unicode(rest)?);
        } else if let Some(rest) = arg.strip_prefix("--region-page-size=") {
            self.region_page_size = parse_region_page_size(rest)?;
        } else if let Some(rest) = arg.strip_prefix("--page-pool-max=") {
            self.page_pool_max = rest
                .parse()
                .map_err(|_| format!("--page-pool-max: expected integer, got '{}'", rest))?;
        } else if let Some(rest) = arg.strip_prefix("--home=") {
            self.home = Some(rest.to_string());
        } else if let Some(rest) = arg.strip_prefix("--path=") {
            self.path = Some(rest.to_string());
        } else {
            match arg {
                "--help" | "-h" => self.help = true,
                "--version" => self.version = true,
                "--json" => self.json = true,
                "--no-stdlib" => self.no_stdlib = true,
                // Aliases for `--trace=<kw>`.
                "--debug" => self.trace_keywords.push("bytecode".into()),
                "--debug-jit" => self.trace_keywords.push("jit".into()),
                "--debug-resume" => self.trace_keywords.push("fiber".into()),
                "--debug-stack" => self.trace_keywords.push("call".into()),
                "--debug-wasm" => self.trace_keywords.push("wasm".into()),
                _ => return self.parse_wasm_flag(arg),
            }
        }
        Ok(true)
    }

    fn parse_trace(&mut self, rest: &str) -> Result<(), String> {
        if rest == "all" {
            self.trace_keywords
                .extend(TRACE_KEYWORDS.iter().map(|kw| kw.to_string()));
            return Ok(());
        }
        for kw in rest.split(',').map(str::trim).filter(|kw| !kw.is_empty()) {
            if !TRACE_KEYWORDS.contains(&kw) {
                return Err(format!(
                    "--trace: unknown keyword '{}'. Valid: {}",
                    kw,
                    TRACE_KEYWORDS.join(", ")
                ));
            }
            self.trace_keywords.push(kw.to_string());
        }
        Ok(())
    }

    /// `stats` runs the program and prints statistics at its end, so it sets
    /// the statistics switch and stays out of the dump set: a non-empty dump set
    /// is what stops a run.
    fn parse_dump(&mut self, rest: &str) -> Result<(), String> {
        if rest == "all" {
            self.dump
                .extend(DUMP_KEYWORDS.iter().map(|kw| kw.to_string()));
            return Ok(());
        }
        for kw in rest.split(',').map(str::trim).filter(|kw| !kw.is_empty()) {
            if kw == "stats" {
                self.stats = true;
            } else if dump_bits::from_name(kw) != 0 {
                self.dump.insert(kw.to_string());
            } else {
                return Err(format!(
                    "--dump: unknown stage '{}'. Valid: {}, stats",
                    kw,
                    DUMP_KEYWORDS.join(", ")
                ));
            }
        }
        Ok(())
    }

    /// The WebAssembly backend's flags, which only a `wasm` build has.
    #[cfg(feature = "wasm")]
    fn parse_wasm_flag(&mut self, arg: &str) -> Result<bool, String> {
        if let Some(rest) = arg.strip_prefix("--wasm=") {
            self.wasm = parse_wasm_policy(rest)?;
            return Ok(true);
        }
        match arg {
            "--wasm-no-stdlib" => self.no_stdlib = true,
            "--wasm-dump" => self.wasm_dump = true,
            "--wasm-lir" => self.wasm_lir = true,
            "--wasm-chunk" => self.wasm_chunk = true,
            "--wasm-no-sparse-spill" => self.wasm_sparse_spill = false,
            _ => return Ok(false),
        }
        Ok(true)
    }

    #[cfg(not(feature = "wasm"))]
    fn parse_wasm_flag(&mut self, _arg: &str) -> Result<bool, String> {
        Ok(false)
    }
}

/// `--wasm=`: `off`, `full`, `lazy`, or a count N, which compiles a closure
/// after N-1 calls (0 is off).
#[cfg(feature = "wasm")]
pub(crate) fn parse_wasm_policy(rest: &str) -> Result<WasmPolicy, String> {
    Ok(match rest {
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
    })
}

fn parse_unicode(rest: &str) -> Result<crate::segment::Generation, String> {
    let components: Result<Vec<i64>, _> = rest.split('.').map(|c| c.parse::<i64>()).collect();
    let request = match components {
        Ok(req) if !req.is_empty() && req.len() <= 3 && req.iter().all(|c| *c >= 0) => req,
        _ => {
            return Err(format!(
                "--unicode: expected MAJ[.MIN[.PATCH]] (for example 16.0), got '{}'",
                rest
            ))
        }
    };
    crate::segment::Generation::from_request(&request).map_err(|e| format!("--unicode: {}", e))
}

/// The floor is the OS page, not a fixed 4096: a smaller page costs a whole OS
/// page anyway, and the size-class ladder is rooted at the OS page
/// (docs/impl/region/model.md).
fn parse_region_page_size(rest: &str) -> Result<usize, String> {
    let n: usize = rest
        .parse()
        .map_err(|_| format!("--region-page-size: expected integer, got '{}'", rest))?;
    let floor = crate::value::fiberheap::pagepool::base_page();
    if n < floor || !n.is_power_of_two() {
        return Err(format!(
            "--region-page-size: must be a power of two >= {}, got {}",
            floor, n
        ));
    }
    Ok(n)
}
