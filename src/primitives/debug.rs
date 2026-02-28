use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, list, Value};

/// Prints a value with debug information
/// (debug-print value)
pub fn prim_debug_print(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("debug-print: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    eprintln!("[DEBUG] {:?}", args[0]);
    (SIG_OK, args[0])
}

/// Traces execution with a label
/// `(trace label value)` — prints `[TRACE] label: value` to stderr, returns value
///
/// Label can be a string or symbol. Symbols are resolved to their
/// name via the thread-local symbol table (same access pattern as
/// symbol->string).
pub fn prim_trace(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("trace: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    if args[0]
        .with_string(|s| {
            eprintln!("[TRACE] {}: {:?}", s, args[1]);
        })
        .is_some()
    {
        (SIG_OK, args[1])
    } else if let Some(sym_id) = args[0].as_symbol() {
        let name = crate::context::resolve_symbol_name(sym_id)
            .unwrap_or_else(|| format!("#<sym:{}>", sym_id));
        eprintln!("[TRACE] {}: {:?}", name, args[1]);
        (SIG_OK, args[1])
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "trace: first argument must be a string or symbol".to_string(),
            ),
        )
    }
}

/// Returns memory usage statistics
/// (memory-usage)
/// Returns a list: (rss-bytes virtual-bytes)
pub fn prim_memory_usage(_args: &[Value]) -> (SignalBits, Value) {
    let (rss_bytes, virtual_bytes) = get_memory_usage();
    (
        SIG_OK,
        list(vec![
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
                let parts: Vec<&str> = output_str.trim().split_whitespace().collect();
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

/// Declarative primitive definitions for debug operations.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "debug/print",
        func: prim_debug_print,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Prints a value with debug information to stderr",
        params: &["value"],
        category: "debug",
        example: "(debug/print 42)",
        aliases: &["debug-print"],
    },
    PrimitiveDef {
        name: "debug/trace",
        func: prim_trace,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Traces execution with a label, prints to stderr, returns value",
        params: &["label", "value"],
        category: "debug",
        example: "(debug/trace \"x\" 42)",
        aliases: &["trace"],
    },
    PrimitiveDef {
        name: "debug/memory",
        func: prim_memory_usage,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "Returns memory usage statistics as (rss-bytes virtual-bytes)",
        params: &[],
        category: "debug",
        example: "(debug/memory)",
        aliases: &["memory-usage"],
    },
];
