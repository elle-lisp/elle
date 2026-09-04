//! Debug print, trace, and memory usage primitives

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Prints a value with debug information
/// (debug-print value)
///
/// Renders through the instance memo: a bare `Debug` carries no table and
/// spells every symbol and keyword `#<symbol:hash>` (docs/impl/symbol.md
/// § "Reading a name, and not reading one").
pub(crate) fn prim_debug_print(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let symbols = ctx.vm().symbols().map(|s| &*s);
    eprintln!("[DEBUG] {}", args[0].debug_with(symbols));
    (SIG_OK, args[0])
}

/// Traces execution with a label
/// `(trace label value)` — prints `[TRACE] label: value` to stderr, returns value
///
/// Label can be a string or symbol. The label and the traced value both resolve
/// their names through the instance memo, so a keyword or symbol in either half
/// of the line is spelled rather than hashed.
pub(crate) fn prim_trace(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let symbols = ctx.vm().symbols().map(|s| &*s);
    let traced = args[1].debug_with(symbols);
    if args[0]
        .with_string(|s| {
            eprintln!("[TRACE] {}: {}", s, traced);
        })
        .is_some()
    {
        (SIG_OK, args[1])
    } else if let Some(sym_id) = args[0].as_symbol() {
        let name = symbols
            .and_then(|s| s.name(sym_id))
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("#<symbol:{:#x}>", sym_id.0));
        eprintln!("[TRACE] {}: {}", name, traced);
        (SIG_OK, args[1])
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "trace: first argument must be a string or symbol".to_string(),
            ),
        )
    }
}

/// Returns memory usage statistics
/// (memory-usage)
/// Returns a list: (rss-bytes virtual-bytes)
pub(crate) fn prim_memory_usage(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let (rss_bytes, virtual_bytes) = get_memory_usage();
    (
        SIG_OK,
        ctx.list(vec![
            Value::int(rss_bytes as i64),
            Value::int(virtual_bytes as i64),
        ]),
    )
}

#[cfg(target_os = "linux")]
fn get_memory_usage() -> (u64, u64) {
    use std::fs;

    // Try to read from /proc/self/status on Linux
    match fs::read_to_string("/proc/self/status") {
        Ok(content) => {
            let mut rss_pages = 0u64;
            let mut vms_bytes = 0u64;

            for line in content.lines() {
                if line.starts_with("VmRSS:") {
                    // Extract RSS in kilobytes and convert to bytes
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            rss_pages = kb * 1024;
                        }
                    }
                }
                if line.starts_with("VmSize:") {
                    // Extract virtual memory size in kilobytes and convert to bytes
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            vms_bytes = kb * 1024;
                        }
                    }
                }
            }
            (rss_pages, vms_bytes)
        }
        Err(_) => (0, 0),
    }
}

#[cfg(target_os = "macos")]
fn get_memory_usage() -> (u64, u64) {
    use std::process::Command;

    // Use ps command on macOS to get RSS and VSZ
    match Command::new("ps")
        .arg("-o")
        .arg("rss=,vsz=")
        .arg("-p")
        .arg(std::process::id().to_string())
        .output()
    {
        Ok(output) => {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                let parts: Vec<&str> = output_str.split_whitespace().collect();
                if parts.len() >= 2 {
                    let rss_kb = parts[0].parse::<u64>().unwrap_or(0);
                    let vsz_kb = parts[1].parse::<u64>().unwrap_or(0);
                    return (rss_kb * 1024, vsz_kb * 1024);
                }
            }
            (0, 0)
        }
        Err(_) => (0, 0),
    }
}

#[cfg(target_os = "windows")]
fn get_memory_usage() -> (u64, u64) {
    use std::process::Command;

    // Use Get-Process PowerShell command on Windows
    match Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(format!(
            "Get-Process -Id {} | Select-Object @{{Name='WS';Expression={{$_.WorkingSet64}}}},@{{Name='VM';Expression={{$_.VirtualMemorySize64}}}} | ConvertTo-Csv -NoTypeInformation",
            std::process::id()
        ))
        .output()
    {
        Ok(output) => {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                // Parse CSV output - should have WS and VM columns
                let lines: Vec<&str> = output_str.trim().lines().collect();
                if lines.len() >= 2 {
                    let values: Vec<&str> = lines[1].split(',').collect();
                    if values.len() >= 2 {
                        let ws = values[0]
                            .trim_matches('"')
                            .parse::<u64>()
                            .unwrap_or(0);
                        let vm = values[1]
                            .trim_matches('"')
                            .parse::<u64>()
                            .unwrap_or(0);
                        return (ws, vm);
                    }
                }
            }
            (0, 0)
        }
        Err(_) => (0, 0),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn get_memory_usage() -> (u64, u64) {
    // Unsupported platform
    (0, 0)
}

/// Returns the number of distinct spellings this instance's memo has
/// recorded, across both vocabularies (docs/impl/symbol.md).
/// (debug/symbol-count)
pub(crate) fn prim_symbol_count(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let n = ctx.vm().symbols().map_or(0, |s| s.len());
    (SIG_OK, Value::int(n as i64))
}

// Declarative primitive definitions for debug operations.
primitive! {
    "debug/print" => prim_debug_print {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Prints a value with debug information to stderr",
        params: &["value"],
        category: "debug",
        example: "(debug/print 42)",
        aliases: &["debug-print"],
        effect: RegionEffect::PassThrough,
    }
    "debug/trace" => prim_trace {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Traces execution with a label, prints to stderr, returns value",
        params: &["label", "value"],
        category: "debug",
        example: "(debug/trace \"x\" 42)",
        aliases: &["trace"],
        effect: RegionEffect::PassThrough,
    }
    "debug/memory" => prim_memory_usage {
        doc: "Returns memory usage statistics as (rss-bytes virtual-bytes)",
        category: "debug",
        example: "(debug/memory)",
        aliases: &["memory-usage"],
        effect: RegionEffect::Fresh,
    }
    "debug/symbol-count" => prim_symbol_count {
        doc: "Returns the number of interned symbols in the symbol table",
        category: "debug",
        example: "(debug/symbol-count)",
        effect: RegionEffect::Immediate,
    }
}
