//! Dataflow analysis facade for functional HIR.
//!
//! Combines def-use chains, value origin analysis, and liveness into
//! a single `DataflowInfo` struct.

use super::arena::BindingArena;
use super::binding::Binding;
use super::defuse::{DefUseBuilder, ValueOrigin};
use super::expr::{Hir, HirId};
use super::liveness::{build_binding_index, BitSet, LivenessAnalyzer};

use std::collections::HashMap;

/// Combined dataflow analysis results for a functionalized HIR tree.
pub struct DataflowInfo {
    /// Where each binding is defined
    pub def_site: HashMap<Binding, HirId>,
    /// Where each binding is used
    pub uses: HashMap<Binding, Vec<HirId>>,
    /// What each result-position expression produces
    pub value_origin: HashMap<HirId, ValueOrigin>,
    /// Which bindings are live after each node
    pub live_out: HashMap<HirId, BitSet>,
    /// Binding → dense bit index
    pub binding_index: HashMap<Binding, usize>,
    /// Dense bit index → Binding
    pub index_binding: Vec<Binding>,
}

/// Run the complete dataflow analysis on a functionalized HIR tree.
pub fn analyze_dataflow(hir: &Hir) -> DataflowInfo {
    // Phase 1: def-use chains + value origin
    let mut du = DefUseBuilder::new();
    du.walk(hir);

    // Phase 2: build binding index
    let (binding_index, index_binding) = build_binding_index(&du.def_site);

    // Phase 3: liveness
    let num_bindings = index_binding.len();
    let mut la = LivenessAnalyzer::new(binding_index.clone(), num_bindings);
    let empty = la.empty_set();
    la.analyze(hir, &empty);

    DataflowInfo {
        def_site: du.def_site,
        uses: du.uses,
        value_origin: du.value_origin,
        live_out: la.live_out,
        binding_index,
        index_binding,
    }
}

/// Format dataflow info as a human-readable dump string.
pub fn format_dataflow(
    info: &DataflowInfo,
    arena: &BindingArena,
    names: &HashMap<u32, String>,
) -> String {
    use std::fmt::Write;
    let mut buf = String::new();

    fn bname(b: Binding, arena: &BindingArena, names: &HashMap<u32, String>) -> String {
        let sym = arena.get(b).name;
        let base = names
            .get(&sym.0)
            .cloned()
            .unwrap_or_else(|| format!("_{}", b.0));
        format!("{}#{}", base, b.0)
    }

    writeln!(buf, ";; ── def-use chains ──").unwrap();
    let mut bindings: Vec<_> = info.def_site.keys().copied().collect();
    bindings.sort_by_key(|b| b.0);
    for b in &bindings {
        let name = bname(*b, arena, names);
        let def = info.def_site.get(b).map(|id| id.0).unwrap_or(0);
        let use_count = info.uses.get(b).map(|v| v.len()).unwrap_or(0);
        let use_ids: Vec<u32> = info
            .uses
            .get(b)
            .map(|v| v.iter().map(|id| id.0).collect())
            .unwrap_or_default();
        writeln!(
            buf,
            "  {:<20} def=@{:<4} uses={} {:?}",
            name, def, use_count, use_ids
        )
        .unwrap();
    }

    writeln!(buf).unwrap();
    writeln!(buf, ";; ── value origins ──").unwrap();
    let mut origins: Vec<_> = info.value_origin.iter().collect();
    origins.sort_by_key(|(id, _)| id.0);
    for (id, origin) in &origins {
        let origin_str = match origin {
            ValueOrigin::Immediate => "immediate".to_string(),
            ValueOrigin::Binding(b) => format!("binding({})", bname(*b, arena, names)),
            ValueOrigin::CallResult => "call-result".to_string(),
            ValueOrigin::Allocation => "allocation".to_string(),
            ValueOrigin::CellDeref => "cell-deref".to_string(),
            ValueOrigin::Mixed => "mixed".to_string(),
        };
        writeln!(buf, "  @{:<4} → {}", id.0, origin_str).unwrap();
    }

    writeln!(buf).unwrap();
    writeln!(buf, ";; ── liveness ──").unwrap();
    let mut live_entries: Vec<_> = info.live_out.iter().collect();
    live_entries.sort_by_key(|(id, _)| id.0);
    for (id, live) in &live_entries {
        if live.is_empty() {
            continue;
        }
        let live_names: Vec<String> = live
            .iter()
            .map(|idx| bname(info.index_binding[idx], arena, names))
            .collect();
        writeln!(
            buf,
            "  @{:<4} live_out: {{{}}}",
            id.0,
            live_names.join(", ")
        )
        .unwrap();
    }

    buf
}
