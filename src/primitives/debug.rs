//! Debugging toolkit primitives
//!
//! Provides introspection and profiling capabilities:
//! - Closure introspection (arity, captures, bytecode size, effects)
//! - Time measurement (instant, duration, CPU time)
//! - Bytecode and JIT disassembly
//! - Debug print, trace, memory usage

use crate::effects::Effect;
use crate::lir::Terminator;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::{error_val, list, Value};
use std::collections::BTreeMap;

// ============================================================================
// Introspection predicates
// ============================================================================

/// (closure? value) — true if value is a bytecode closure
pub fn prim_is_closure(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("closure?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_closure().is_some()))
}

/// (jit? value) — true if closure has JIT-compiled code
pub fn prim_is_jit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("jit?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.jit_code.is_some()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (pure? value) — true if closure has Pure yield behavior
pub fn prim_is_pure(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pure?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.effect.is_pure()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (mutates-params? value) — true if closure mutates any parameters
pub fn prim_mutates_params(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("mutates-params?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.cell_params_mask != 0))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (raises? value) — true if closure may raise
pub fn prim_raises(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("raises?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.effect.may_raise()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

// ============================================================================
// Additional introspection
// ============================================================================

/// (arity value) — closure arity as int, pair, or nil
pub fn prim_arity(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arity: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let result = match closure.arity {
            Arity::Exact(n) => Value::int(n as i64),
            Arity::AtLeast(n) => Value::cons(Value::int(n as i64), Value::NIL),
            Arity::Range(min, max) => Value::cons(Value::int(min as i64), Value::int(max as i64)),
        };
        (SIG_OK, result)
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (captures value) — number of captured variables, or nil
pub fn prim_captures(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("captures: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::int(closure.env.len() as i64))
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (bytecode-size value) — size of bytecode in bytes, or nil
pub fn prim_bytecode_size(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytecode-size: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::int(closure.bytecode.len() as i64))
    } else {
        (SIG_OK, Value::NIL)
    }
}

// ============================================================================
// VM-access introspection (SIG_QUERY)
// ============================================================================

/// (doc name) — look up documentation for any named form (primitive, special form, or macro)
///
/// Sends a SIG_QUERY to the VM which looks up the name in its
/// `docs` map and returns a formatted doc string.
pub fn prim_doc(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("doc: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_QUERY, Value::cons(Value::keyword("doc"), args[0]))
}

/// (vm/query op arg) — query VM state
///
/// The single gateway to SIG_QUERY. `op` is a string or keyword
/// naming the operation; `arg` is the operation-specific argument.
/// The VM's dispatch_query handles the rest.
///
/// Operations:
/// - "call-count" closure → int
/// - "doc" name → string
/// - "global?" symbol → bool
/// - "fiber/self" _ → fiber or nil
pub fn prim_vm_query(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vm/query: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    if !args[0].is_string() && args[0].as_keyword_name().is_none() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "vm/query: operation must be a string or keyword, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    (SIG_QUERY, Value::cons(args[0], args[1]))
}

/// (string->keyword str) — convert a string to a keyword
///
/// Creates a content-addressed keyword from the string name.
pub fn prim_string_to_keyword(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string->keyword: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(kw) = args[0].with_string(Value::keyword) {
        (SIG_OK, kw)
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string->keyword: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// (vm/list-primitives) or (vm/list-primitives :category) — list registered names
///
/// Returns a sorted list of strings. With no arguments, returns all names.
/// With a category keyword or string, filters by category (e.g., :math, :"special form").
pub fn prim_list_primitives(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "vm/list-primitives: expected 0-1 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let filter = if args.is_empty() { Value::NIL } else { args[0] };
    (
        SIG_QUERY,
        Value::cons(Value::keyword("list-primitives"), filter),
    )
}

/// (vm/primitive-meta name) — get structured metadata for a primitive
///
/// Returns a struct with keys: "name", "doc", "params", "category", "example", "arity", "effect".
/// Returns nil if the primitive is not found.
pub fn prim_primitive_meta(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vm/primitive-meta: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("primitive-meta"), args[0]),
    )
}

// ============================================================================
// Disassembly
// ============================================================================

/// (disbit closure) — disassemble bytecode as array of strings
pub fn prim_disbit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("disbit: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let mut lines = crate::compiler::disassemble_lines(&closure.bytecode);
        for (i, c) in closure.constants.iter().enumerate() {
            lines.push(format!("const[{}] = {:?}", i, c));
        }
        (
            SIG_OK,
            Value::array(lines.into_iter().map(Value::string).collect()),
        )
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "disbit: argument must be a closure".to_string(),
            ),
        )
    }
}

/// (disjit closure) — return Cranelift IR as array of strings, or nil
pub fn prim_disjit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("disjit: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let lir = match &closure.lir_function {
            Some(lir) => lir.clone(),
            None => return (SIG_OK, Value::NIL),
        };
        let compiler = match crate::jit::JitCompiler::new() {
            Ok(c) => c,
            Err(_) => return (SIG_OK, Value::NIL),
        };
        match compiler.clif_text(&lir, None) {
            Ok(lines) => (
                SIG_OK,
                Value::array(lines.into_iter().map(Value::string).collect()),
            ),
            Err(_) => (SIG_OK, Value::NIL),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "disjit: argument must be a closure".to_string(),
            ),
        )
    }
}

/// (fn/flow closure) — return LIR control flow graph as structured data
///
/// Returns a struct with keys:
/// - :name — function name (string or nil)
/// - :arity — arity as string (e.g., "2", "1+", "2-4")
/// - :regs — number of virtual registers (int)
/// - :locals — number of local slots (int)
/// - :entry — entry block label (int)
/// - :blocks — tuple of block structs, each with:
///   - :label — block label (int)
///   - :instrs — tuple of instruction strings (Debug format)
///   - :term — terminator string (Debug format)
///   - :edges — tuple of successor label ints
///
/// Returns nil if the closure has no LIR (e.g., native function or LIR discarded).
/// Errors if argument is not a closure.
pub fn prim_fn_flow(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fn/flow: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let lir = match &closure.lir_function {
            Some(lir) => lir,
            None => return (SIG_OK, Value::NIL),
        };

        // Build top-level struct
        let mut fields = BTreeMap::new();

        // :name
        fields.insert(
            TableKey::Keyword("name".to_string()),
            match &lir.name {
                Some(n) => Value::string(n.as_str()),
                None => Value::NIL,
            },
        );

        // :doc
        fields.insert(
            TableKey::Keyword("doc".to_string()),
            closure.doc.unwrap_or(Value::NIL),
        );

        // :arity — use Display impl: "2", "1+", "2-4"
        fields.insert(
            TableKey::Keyword("arity".to_string()),
            Value::string(format!("{}", lir.arity)),
        );

        // :regs
        fields.insert(
            TableKey::Keyword("regs".to_string()),
            Value::int(lir.num_regs as i64),
        );

        // :locals
        fields.insert(
            TableKey::Keyword("locals".to_string()),
            Value::int(lir.num_locals as i64),
        );

        // :entry
        fields.insert(
            TableKey::Keyword("entry".to_string()),
            Value::int(lir.entry.0 as i64),
        );

        // :blocks — tuple of block structs
        let blocks: Vec<Value> = lir
            .blocks
            .iter()
            .map(|block| {
                let mut block_fields = BTreeMap::new();

                // :label
                block_fields.insert(
                    TableKey::Keyword("label".to_string()),
                    Value::int(block.label.0 as i64),
                );

                // :instrs — tuple of Debug-formatted instruction strings
                let instrs: Vec<Value> = block
                    .instructions
                    .iter()
                    .map(|si| Value::string(format!("{:?}", si.instr)))
                    .collect();
                block_fields.insert(
                    TableKey::Keyword("instrs".to_string()),
                    Value::tuple(instrs),
                );

                // :term — Debug-formatted terminator string
                block_fields.insert(
                    TableKey::Keyword("term".to_string()),
                    Value::string(format!("{:?}", block.terminator.terminator)),
                );

                // :edges — tuple of successor label ints
                let edges: Vec<Value> = match &block.terminator.terminator {
                    Terminator::Return(_) | Terminator::Unreachable => vec![],
                    Terminator::Jump(label) => vec![Value::int(label.0 as i64)],
                    Terminator::Branch {
                        then_label,
                        else_label,
                        ..
                    } => {
                        vec![
                            Value::int(then_label.0 as i64),
                            Value::int(else_label.0 as i64),
                        ]
                    }
                    Terminator::Yield { resume_label, .. } => {
                        vec![Value::int(resume_label.0 as i64)]
                    }
                };
                block_fields.insert(TableKey::Keyword("edges".to_string()), Value::tuple(edges));

                Value::struct_from(block_fields)
            })
            .collect();

        fields.insert(
            TableKey::Keyword("blocks".to_string()),
            Value::tuple(blocks),
        );

        (SIG_OK, Value::struct_from(fields))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "fn/flow: argument must be a closure".to_string(),
            ),
        )
    }
}

// ============================================================================
// Debug print, trace, memory
// ============================================================================

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

/// Declarative primitive definitions for debugging operations.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "closure?",
        func: prim_is_closure,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if value is a bytecode closure",
        params: &["value"],
        category: "predicate",
        example: "(closure? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "jit?",
        func: prim_is_jit,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure has JIT-compiled code",
        params: &["value"],
        category: "predicate",
        example: "(jit? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "pure?",
        func: prim_is_pure,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure has Pure effect (does not suspend)",
        params: &["value"],
        category: "predicate",
        example: "(pure? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "coro?",
        func: crate::primitives::coroutines::prim_is_coroutine,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if value is a coroutine (fiber-based)",
        params: &["value"],
        category: "predicate",
        example: "(coro? (coro/new (fn () 42)))",
        aliases: &["coroutine?"],
    },
    PrimitiveDef {
        name: "fn/mutates-params?",
        func: prim_mutates_params,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure mutates any parameters",
        params: &["value"],
        category: "fn",
        example: "(fn/mutates-params? (fn (x) (set x 1)))",
        aliases: &["mutates-params?"],
    },
    PrimitiveDef {
        name: "fn/raises?",
        func: prim_raises,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure may raise an error",
        params: &["value"],
        category: "fn",
        example: "(fn/raises? (fn (x) (/ 1 x)))",
        aliases: &["raises?"],
    },
    PrimitiveDef {
        name: "fn/arity",
        func: prim_arity,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns closure arity as int, pair, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/arity (fn (x y) x))",
        aliases: &["arity"],
    },
    PrimitiveDef {
        name: "fn/captures",
        func: prim_captures,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns number of captured variables, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/captures (let ((x 1)) (fn () x)))",
        aliases: &["captures"],
    },
    PrimitiveDef {
        name: "fn/bytecode-size",
        func: prim_bytecode_size,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns size of bytecode in bytes, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/bytecode-size (fn (x) x))",
        aliases: &["bytecode-size"],
    },
    PrimitiveDef {
        name: "doc",
        func: prim_doc,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Look up documentation for a primitive.",
        params: &["name"],
        category: "meta",
        example: "(doc \"cons\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vm/query",
        func: prim_vm_query,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Query VM state (call-count, doc, global?, fiber/self)",
        params: &["op", "arg"],
        category: "meta",
        example: "(vm/query \"call-count\" some-fn)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string->keyword",
        func: prim_string_to_keyword,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert a string to a keyword",
        params: &["str"],
        category: "conversion",
        example: "(string->keyword \"foo\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fn/disasm",
        func: prim_disbit,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Disassemble a closure's bytecode into a list of instruction strings.",
        params: &["closure"],
        category: "fn",
        example: "(fn/disasm (fn (x) x))",
        aliases: &["disbit", "fn/disbit"],
    },
    PrimitiveDef {
        name: "fn/disasm-jit",
        func: prim_disjit,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Disassemble a closure's JIT-compiled Cranelift IR, or nil if not JIT'd.",
        params: &["closure"],
        category: "fn",
        example: "(fn/disasm-jit (fn (x) x))",
        aliases: &["disjit", "fn/disjit"],
    },
    PrimitiveDef {
        name: "fn/flow",
        func: prim_fn_flow,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Return the LIR control flow graph of a closure as structured data.",
        params: &["closure"],
        category: "fn",
        example: "(fn/flow (fn (x y) (+ x y)))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vm/list-primitives",
        func: prim_list_primitives,
        effect: Effect::none(),
        arity: Arity::Range(0, 1),
        doc: "List registered names as a sorted list of strings. Optional category filter.",
        params: &["category?"],
        category: "meta",
        example: "(vm/list-primitives)\n(vm/list-primitives :math)\n(vm/list-primitives :\"special form\")",
        aliases: &["list-primitives"],
    },
    PrimitiveDef {
        name: "vm/primitive-meta",
        func: prim_primitive_meta,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get structured metadata for a primitive as a struct.",
        params: &["name"],
        category: "meta",
        example:
            "(struct-get (vm/primitive-meta \"cons\") \"doc\") #=> \"Construct a cons cell...\"",
        aliases: &["primitive-meta"],
    },
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
