use crate::error::LResult;
use crate::value::{list, Value};

/// Prints a value with debug information
/// (debug-print value)
pub fn prim_debug_print(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(format!("debug-print: expected 1 argument, got {}", args.len()).into());
    }

    eprintln!("[DEBUG] {:?}", args[0]);
    Ok(args[0].clone())
}

/// Traces execution with a label
/// (trace name value)
pub fn prim_trace(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(format!("trace: expected 2 arguments, got {}", args.len()).into());
    }

    match &args[0] {
        Value::String(label) => {
            eprintln!("[TRACE] {}: {:?}", label, args[1]);
            Ok(args[1].clone())
        }
        Value::Symbol(label_id) => {
            eprintln!("[TRACE] {:?}: {:?}", label_id, args[1]);
            Ok(args[1].clone())
        }
        _ => Err("trace: first argument must be a string or symbol"
            .to_string()
            .into()),
    }
}

/// Times the execution of a thunk
/// (profile thunk)
pub fn prim_profile(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(format!("profile: expected 1 argument, got {}", args.len()).into());
    }

    // In production, would time execution of closure
    // For now, just return a placeholder timing
    match &args[0] {
        Value::Closure(_) | Value::NativeFn(_) => {
            Ok(Value::String("profiling-not-yet-implemented".into()))
        }
        _ => Err("profile: argument must be a function".to_string().into()),
    }
}

/// Returns memory usage statistics
/// (memory-usage)
/// Returns a list: (rss-bytes virtual-bytes)
pub fn prim_memory_usage(_args: &[Value]) -> LResult<Value> {
    let (rss_bytes, virtual_bytes) = get_memory_usage();
    Ok(list(vec![
        Value::Int(rss_bytes as i64),
        Value::Int(virtual_bytes as i64),
    ]))
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
