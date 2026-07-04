//! Compilation group discovery for batch JIT compilation.
//!
//! When a function becomes hot, we scan its LIR for calls to other global
//! functions. If those functions are also JIT-compilable, we compile them
//! together into a single Cranelift module with direct calls between them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::lir::{LirFunction, LirInstr, Reg};
use crate::value::{Arity, SymbolId, Value};

/// Maximum number of functions in a compilation group.
/// Prevents Cranelift compilation time from spiking on large call graphs.
const MAX_GROUP_SIZE: usize = 16;

/// Maximum BFS depth for transitive call discovery.
/// Prevents pulling in distant, loosely-related functions.
const MAX_DISCOVERY_DEPTH: usize = 4;

/// Discover a compilation group starting from a hot function.
///
/// Scans the LIR for `LoadGlobal(sym)` → `Call`/`TailCall` patterns,
/// resolves each symbol against runtime globals, and transitively discovers
/// callee functions that are also JIT-compilable. Discovery is bounded by
/// both group size (`MAX_GROUP_SIZE`) and BFS depth (`MAX_DISCOVERY_DEPTH`).
///
/// Returns a list of `(SymbolId, Rc<LirFunction>)` pairs for all functions
/// in the group. The original hot function is NOT included (the caller
/// already has it). Returns an empty vec if no peers were found.
///
/// Phase 1 restriction: only includes capture-free functions
/// (num_captures == 0) since direct SCC calls pass null env.
pub(crate) fn discover_compilation_group(
    hot_lir: &LirFunction,
    globals: &[Value],
) -> Vec<(SymbolId, Rc<LirFunction>)> {
    let mut visited: HashSet<SymbolId> = HashSet::new();
    let mut group: Vec<(SymbolId, Rc<LirFunction>)> = Vec::new();

    let targets = find_global_call_targets(hot_lir);
    // BFS with depth tracking: (SymbolId, depth)
    let mut worklist: VecDeque<(SymbolId, usize)> =
        targets.into_iter().map(|sym| (sym, 1)).collect();

    while let Some((sym, depth)) = worklist.pop_front() {
        if group.len() >= MAX_GROUP_SIZE {
            break;
        }

        if !visited.insert(sym) {
            continue;
        }

        let idx = sym.0 as usize;
        if idx >= globals.len() {
            continue;
        }
        let val = &globals[idx];

        let closure = match val.as_closure() {
            Some(c) => c,
            None => continue,
        };

        let lir = match &closure.template.lir_function {
            Some(lir) => lir.clone(),
            None => continue,
        };

        if lir.signal.may_suspend() {
            continue;
        }

        // Phase 1: must be capture-free
        if lir.num_captures > 0 {
            continue;
        }

        // Variadic functions with struct/named varargs can't be JIT-compiled
        // (need fiber access for keyword error reporting). List variadics are OK.
        if matches!(lir.arity, Arity::AtLeast(_))
            && !matches!(lir.vararg_kind, crate::hir::VarargKind::List)
        {
            continue;
        }

        if has_unsupported_instructions(&lir) {
            continue;
        }

        group.push((sym, lir.clone()));

        // Recurse into this function's call targets (if within depth bound)
        if depth < MAX_DISCOVERY_DEPTH {
            let sub_targets = find_global_call_targets(&lir);
            for sub_sym in sub_targets {
                if !visited.contains(&sub_sym) {
                    worklist.push_back((sub_sym, depth + 1));
                }
            }
        }
    }

    group
}

/// Scan a LIR function for global call targets.
///
/// Builds a Reg -> SymbolId map from LoadGlobal instructions across all
/// basic blocks, then checks which of those registers are used as the func
/// argument in Call/TailCall. Cross-block tracking is sound because LIR is
/// SSA: each register is assigned exactly once, so a LoadGlobal in block 0
/// that defines Reg(5) is the only definition, and any Call using Reg(5)
/// in any block definitively targets that global.
fn find_global_call_targets(lir: &LirFunction) -> HashSet<SymbolId> {
    let reg_to_sym: HashMap<Reg, SymbolId> = HashMap::new();
    let mut targets: HashSet<SymbolId> = HashSet::new();

    for bb in &lir.blocks {
        for spanned in &bb.instructions {
            match &spanned.instr {
                LirInstr::Call { func, .. } | LirInstr::TailCall { func, .. } => {
                    if let Some(sym) = reg_to_sym.get(func) {
                        targets.insert(*sym);
                    }
                }
                _ => {}
            }
        }
    }

    targets
}

/// Check if a LIR function contains instructions the JIT can't handle.
///
/// This is a pre-filter for batch compilation discovery. It must be kept in
/// sync with the unsupported instruction arms in `translate.rs::translate_instr`.
/// If this list is stale (misses a newly unsupported instruction), the batch
/// compilation will fail with `UnsupportedInstruction` and `try_batch_jit`
/// will fall through to solo compilation — so staleness is a performance
/// issue, not a correctness issue.
fn has_unsupported_instructions(lir: &LirFunction) -> bool {
    for bb in &lir.blocks {
        for spanned in &bb.instructions {
            if let LirInstr::Eval { .. } = &spanned.instr {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests;
