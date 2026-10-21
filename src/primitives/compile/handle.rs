// audited: 2026-10-21
//! The analysis handle `compile/analyze` answers, and the argument readers every query shares.
//!
//! docs/analysis/portrait.md

use super::*;

// ── Helper ─────────────────────────────────────────────────────────────

pub(super) fn kw(name: &str) -> TableKey {
    TableKey::keyword(name)
}

// ── Analysis handle ────────────────────────────────────────────────────

/// (byte_offset, byte_len) of a name token in source text.
pub(super) type NameSpan = (usize, usize);

/// Opaque handle wrapping the result of `analyze_file`.
///
/// Stored as `ctx.external("analysis", AnalysisHandle)`.  Query
/// primitives downcast the External to access the fields.
pub struct AnalysisHandle {
    pub hir: Hir,
    pub arena: BindingArena,
    pub symbol_index: SymbolIndex,
    pub diagnostics: Vec<Diagnostic>,
    /// Function name → Signal, built eagerly.
    pub signal_map: HashMap<String, Signal>,
    /// Function name → `Vec<CallEdge>`, built eagerly.
    pub call_graph: CallGraphData,
    /// Original source text.
    pub source: String,
    /// Binding → all source locations where the binding's name appears.
    pub binding_spans: HashMap<Binding, Vec<NameSpan>>,
}

#[derive(Debug, Clone)]
pub struct CallEdge {
    pub callee: String,
    pub line: u32,
    pub col: u32,
    pub is_tail: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CallGraphData {
    /// caller name → outgoing edges
    pub edges: HashMap<String, Vec<CallEdge>>,
    /// callee name → caller names
    pub reverse: HashMap<String, Vec<String>>,
    /// Functions with no callers.
    pub roots: Vec<String>,
    /// Functions that call no user-defined functions.
    pub leaves: Vec<String>,
}

// ── Extract the handle from an argument ────────────────────────────────

pub(super) fn get_handle<'a>(
    args: &'a [Value],
    name: &str,
    ctx: &mut NativeCtx,
) -> Result<&'a AnalysisHandle, (SignalBits, Value)> {
    match args[0].as_external::<AnalysisHandle>() {
        Some(h) => Ok(h),
        None => Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: expected analysis handle, got {}",
                    name,
                    args[0].type_name()
                ),
            ),
        )),
    }
}

/// Resolve a keyword argument to a function name string.
pub(super) fn resolve_name(
    args: &[Value],
    idx: usize,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    // Accept keyword or string.
    if let Some(name) = ctx.keyword_spelling(args[idx]) {
        return Ok(name.to_string());
    }
    if let Some(name) = args[idx].with_string(|s| s.to_string()) {
        return Ok(name);
    }
    Err((
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "{}: expected keyword or string for function name, got {}",
                prim_name,
                args[idx].type_name()
            ),
        ),
    ))
}
