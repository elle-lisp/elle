//! Bytecode and JIT disassembly primitives

use crate::lir::{terminator_kind, Terminator};
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::collections::BTreeMap;

/// (vm/list-primitives) or (vm/list-primitives :category) — list registered names
///
/// Returns a sorted list of strings. With no arguments, returns all names.
/// With a category keyword or string, filters by category (e.g., :math, :"special form").
pub(crate) fn prim_list_primitives(args: &[Value]) -> (SignalBits, Value) {
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
/// Returns a struct with keys: "name", "doc", "params", "category", "example", "arity", "signal".
/// Returns nil if the primitive is not found.
pub(crate) fn prim_primitive_meta(args: &[Value]) -> (SignalBits, Value) {
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

/// (fn/disasm closure) — disassemble bytecode as array of strings
pub(crate) fn prim_disbit(args: &[Value]) -> (SignalBits, Value) {
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
        let mut lines = crate::compiler::disassemble_lines(&closure.template.bytecode);
        for (i, c) in closure.template.constants.iter().enumerate() {
            lines.push(format!("const[{}] = {:?}", i, c));
        }
        (
            SIG_OK,
            Value::array_mut(lines.into_iter().map(Value::string).collect()),
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

/// (fn/disasm-jit closure) — return Cranelift IR as array of strings, or nil
pub(crate) fn prim_disjit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("disjit: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    #[cfg(feature = "jit")]
    if let Some(closure) = args[0].as_closure() {
        let lir = match &closure.template.lir_function {
            Some(lir) => lir.clone(),
            None => return (SIG_OK, Value::NIL),
        };
        let compiler = match crate::jit::JitCompiler::new() {
            Ok(c) => c,
            Err(_) => return (SIG_OK, Value::NIL),
        };
        match compiler.clif_text(&lir, None) {
            Ok(lines) => {
                return (
                    SIG_OK,
                    Value::array_mut(lines.into_iter().map(Value::string).collect()),
                )
            }
            Err(_) => return (SIG_OK, Value::NIL),
        }
    }
    #[cfg(not(feature = "jit"))]
    if args[0].as_closure().is_some() {
        return (SIG_OK, Value::NIL);
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            "disjit: argument must be a closure".to_string(),
        ),
    )
}

/// Build the CFG struct from a closure's LIR.
fn flow_from_closure(closure: &crate::value::heap::Closure) -> (SignalBits, Value) {
    let lir = match &closure.template.lir_function {
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
        closure.template.doc.unwrap_or(Value::NIL),
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

    // :blocks — array of block structs
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

            // :instrs — array of Debug-formatted instruction strings
            let instrs: Vec<Value> = block
                .instructions
                .iter()
                .map(|si| Value::string(format!("{:?}", si.instr)))
                .collect();
            block_fields.insert(
                TableKey::Keyword("instrs".to_string()),
                Value::array(instrs),
            );

            // :display — array of compact human-readable instruction strings
            let display: Vec<Value> = block
                .instructions
                .iter()
                .map(|si| Value::string(format!("{}", si.instr)))
                .collect();
            block_fields.insert(
                TableKey::Keyword("display".to_string()),
                Value::array(display),
            );

            // :spans — array of "line:col" strings (nil for synthetic spans)
            let spans: Vec<Value> = block
                .instructions
                .iter()
                .map(|si| {
                    if si.span.line == 0 {
                        Value::NIL
                    } else {
                        Value::string(format!("{}:{}", si.span.line, si.span.col))
                    }
                })
                .collect();
            block_fields.insert(TableKey::Keyword("spans".to_string()), Value::array(spans));

            // :annotated — display strings with span annotations for CFG rendering
            let annotated: Vec<Value> = block
                .instructions
                .iter()
                .map(|si| {
                    let base = format!("{}", si.instr);
                    if si.span.line == 0 {
                        Value::string(base)
                    } else {
                        Value::string(format!("{} @{}:{}", base, si.span.line, si.span.col))
                    }
                })
                .collect();
            block_fields.insert(
                TableKey::Keyword("annotated".to_string()),
                Value::array(annotated),
            );

            // :term — Debug-formatted terminator string
            block_fields.insert(
                TableKey::Keyword("term".to_string()),
                Value::string(format!("{:?}", block.terminator.terminator)),
            );

            // :term-display — compact terminator string
            block_fields.insert(
                TableKey::Keyword("term-display".to_string()),
                Value::string(format!("{}", block.terminator.terminator)),
            );

            // :term-span — "line:col" string for the terminator (nil for synthetic)
            let term_span = if block.terminator.span.line == 0 {
                Value::NIL
            } else {
                Value::string(format!(
                    "{}:{}",
                    block.terminator.span.line, block.terminator.span.col
                ))
            };
            block_fields.insert(TableKey::Keyword("term-span".to_string()), term_span);

            // :term-kind — keyword identifying the terminator type
            block_fields.insert(
                TableKey::Keyword("term-kind".to_string()),
                Value::keyword(terminator_kind(&block.terminator.terminator)),
            );

            // :edges — array of successor label ints
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
                Terminator::Emit { resume_label, .. } => {
                    vec![Value::int(resume_label.0 as i64)]
                }
            };
            block_fields.insert(TableKey::Keyword("edges".to_string()), Value::array(edges));

            Value::struct_from(block_fields)
        })
        .collect();

    fields.insert(
        TableKey::Keyword("blocks".to_string()),
        Value::array(blocks),
    );

    (SIG_OK, Value::struct_from(fields))
}

/// (fn/flow target) — return LIR control flow graph as structured data
///
/// Returns a struct with keys:
/// - :name — function name (string or nil)
/// - :arity — arity as string (e.g., "2", "1+", "2-4")
/// - :regs — number of virtual registers (int)
/// - :locals — number of local slots (int)
/// - :entry — entry block label (int)
/// - :blocks — array of block structs, each with:
///   - :label — block label (int)
///   - :instrs — array of instruction strings (Debug format)
///   - :display — array of compact instruction strings (Display format)
///   - :spans — array of "line:col" strings, same length as :display (nil for synthetic spans)
///   - :annotated — array of display strings with " @line:col" suffix (for CFG rendering)
///   - :term — terminator string (Debug format)
///   - :term-display — compact terminator string (Display format)
///   - :term-span — "line:col" string for the terminator (nil for synthetic)
///   - :term-kind — keyword: :return, :jump, :branch, :yield, or :unreachable
///   - :edges — array of successor label ints
///
/// Returns nil if the closure has no LIR (e.g., native function or LIR discarded).
/// Errors if argument is not a closure or fiber, or if the fiber is currently executing.
pub(crate) fn prim_fn_flow(args: &[Value]) -> (SignalBits, Value) {
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
        flow_from_closure(closure)
    } else if let Some(handle) = args[0].as_fiber() {
        match handle.try_with(|fiber| flow_from_closure(&fiber.closure)) {
            Some(result) => result,
            None => (
                SIG_ERROR,
                error_val(
                    "state-error",
                    "fn/flow: fiber is currently executing".to_string(),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "fn/flow: argument must be a closure or fiber".to_string(),
            ),
        )
    }
}

/// Declarative primitive definitions for disassembly operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "fn/disasm",
        func: prim_disbit,
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the LIR control flow graph of a closure or fiber as structured data.",
        params: &["closure-or-fiber"],
        category: "fn",
        example: "(fn/flow (fn (x y) (+ x y)))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vm/list-primitives",
        func: prim_list_primitives,
        signal: Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 },
        arity: Arity::Range(0, 1),
        doc: "List registered names as a sorted list of symbols. Optional category filter.",
        params: &["category?"],
        category: "meta",
        example: "(vm/list-primitives)\n(vm/list-primitives :math)\n(vm/list-primitives :\"special form\")",
        aliases: &["list-primitives"],
    },
    PrimitiveDef {
        name: "vm/primitive-meta",
        func: prim_primitive_meta,
        signal: Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 },
        arity: Arity::Exact(1),
        doc: "Get structured metadata for a primitive as a struct.",
        params: &["name"],
        category: "meta",
        example:
            "(struct-get (vm/primitive-meta \"cons\") \"doc\") #=> \"Construct a cons cell...\"",
        aliases: &["primitive-meta"],
    },
];
