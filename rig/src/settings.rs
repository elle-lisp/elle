// audited: 2026-09-29
//! The configuration a file runs under on the rig: read from a sidecar or a profile, and printed back as one.
//!
//! rig/overview.md

use elle::config::{Config, JitPolicy, MlirPolicy, WasmPolicy, TRACE_KEYWORDS};
use std::collections::BTreeSet;
use std::path::Path;

/// The settings a sidecar or a profile can name.
pub struct Settings {
    jit: JitPolicy,
    mlir: MlirPolicy,
    wasm: WasmPolicy,
    trace: BTreeSet<String>,
}

/// How a read file combines with what is already there. A sidecar and a
/// profile read alike (rig/overview.md): each tier setting replaces the one
/// before it, and trace keywords join.
impl Settings {
    /// The settings `config` already carries: the build's tier defaults, and
    /// whatever `--trace=` the command line named.
    pub fn from_config(config: &Config) -> Self {
        Settings {
            jit: config.jit.clone(),
            mlir: config.mlir.clone(),
            wasm: config.wasm.clone(),
            trace: config.trace_keywords.iter().cloned().collect(),
        }
    }

    /// Read the sidecar or profile at `path` over these settings, or answer
    /// why the rig refuses it. The reason names the file, and the key or the
    /// value it could not read.
    pub fn apply_file(&mut self, path: &Path) -> Result<(), String> {
        let refuse = |why: String| format!("{}: {}", path.display(), why);
        let text = std::fs::read_to_string(path).map_err(|e| refuse(e.to_string()))?;
        let table: toml::Table = toml::from_str(&text).map_err(|e| refuse(e.to_string()))?;
        for (key, value) in &table {
            match key.as_str() {
                "jit" => self.jit = read_jit(value).map_err(refuse)?,
                "mlir" => self.mlir = read_mlir(value).map_err(refuse)?,
                "wasm" => self.wasm = read_wasm(value).map_err(refuse)?,
                "trace" => self.trace.extend(read_trace(value).map_err(refuse)?),
                other => return Err(refuse(format!("unknown key `{other}`"))),
            }
        }
        Ok(())
    }

    /// Install these settings on the configuration the program runs under.
    pub fn install(self, config: &mut Config) {
        config.jit = self.jit;
        config.mlir = self.mlir;
        config.wasm = self.wasm;
        config.trace_keywords = self.trace.into_iter().collect();
    }

    /// These settings as a sidecar the same build accepts: one line per
    /// setting, `mlir` and `wasm` only in a build that carries the tier.
    pub fn render(&self) -> String {
        let mut out = format!("jit = {}\n", tier(self.jit.reading()));
        if cfg!(feature = "mlir") {
            out.push_str(&format!("mlir = {}\n", tier(self.mlir.reading())));
        }
        if cfg!(feature = "wasm") {
            let wasm = match &self.wasm {
                WasmPolicy::Off => "\"off\"".to_string(),
                WasmPolicy::Full => "\"full\"".to_string(),
                WasmPolicy::Lazy { threshold } => (threshold + 1).to_string(),
            };
            out.push_str(&format!("wasm = {wasm}\n"));
        }
        let trace: Vec<String> = self.trace.iter().map(|kw| format!("\"{kw}\"")).collect();
        out.push_str(&format!("trace = [{}]\n", trace.join(", ")));
        out
    }
}

/// A JIT or MLIR policy as a sidecar spells it: `"off"`, `"eager"`, or the
/// threshold.
fn tier(reading: Option<usize>) -> String {
    match reading {
        None => "\"off\"".to_string(),
        Some(0) => "\"eager\"".to_string(),
        Some(n) => n.to_string(),
    }
}

/// A tier setting: `"off"`, `"eager"`, or a positive integer.
enum Tier {
    Off,
    Eager,
    After(usize),
}

fn read_tier(key: &str, value: &toml::Value) -> Result<Tier, String> {
    match value {
        toml::Value::String(s) if s == "off" => Ok(Tier::Off),
        toml::Value::String(s) if s == "eager" => Ok(Tier::Eager),
        toml::Value::Integer(n) if *n >= 1 => Ok(Tier::After(*n as usize)),
        toml::Value::Integer(n) => Err(format!(
            "`{key}`: a threshold is a positive integer, got {n}"
        )),
        other => Err(format!(
            "`{key}`: expected \"off\", \"eager\" or a positive integer, got {other}"
        )),
    }
}

fn read_jit(value: &toml::Value) -> Result<JitPolicy, String> {
    let setting = read_tier("jit", value)?;
    if !cfg!(feature = "jit") && !matches!(setting, Tier::Off) {
        return Err("`jit`: this build carries no JIT tier".to_string());
    }
    Ok(match setting {
        Tier::Off => JitPolicy::Off,
        Tier::Eager => JitPolicy::Eager,
        Tier::After(threshold) => JitPolicy::Adaptive { threshold },
    })
}

fn read_mlir(value: &toml::Value) -> Result<MlirPolicy, String> {
    if !cfg!(feature = "mlir") {
        return Err("`mlir`: this build carries no MLIR tier".to_string());
    }
    Ok(match read_tier("mlir", value)? {
        Tier::Off => MlirPolicy::Off,
        Tier::Eager => MlirPolicy::Eager,
        Tier::After(threshold) => MlirPolicy::Adaptive { threshold },
    })
}

/// `wasm`: `"off"`, `"full"`, or a count N, read as `--wasm=N` reads it: each
/// closure compiles after N-1 calls.
fn read_wasm(value: &toml::Value) -> Result<WasmPolicy, String> {
    if !cfg!(feature = "wasm") {
        return Err("`wasm`: this build carries no WebAssembly backend".to_string());
    }
    match value {
        toml::Value::String(s) if s == "off" => Ok(WasmPolicy::Off),
        toml::Value::String(s) if s == "full" => Ok(WasmPolicy::Full),
        toml::Value::Integer(n) if *n >= 1 => Ok(WasmPolicy::Lazy {
            threshold: (*n - 1) as usize,
        }),
        toml::Value::Integer(n) => Err(format!(
            "`wasm`: a threshold is a positive integer, got {n}"
        )),
        other => Err(format!(
            "`wasm`: expected \"off\", \"full\" or a positive integer, got {other}"
        )),
    }
}

/// `trace`: an array of the build's trace keywords.
fn read_trace(value: &toml::Value) -> Result<Vec<String>, String> {
    let toml::Value::Array(items) = value else {
        return Err(format!(
            "`trace`: expected an array of trace keywords, got {value}"
        ));
    };
    items
        .iter()
        .map(|item| match item {
            toml::Value::String(kw) if TRACE_KEYWORDS.contains(&kw.as_str()) => Ok(kw.clone()),
            toml::Value::String(kw) => Err(format!(
                "`trace`: unknown keyword `{kw}`. Valid: {}",
                TRACE_KEYWORDS.join(", ")
            )),
            other => Err(format!("`trace`: expected a keyword string, got {other}")),
        })
        .collect()
}
