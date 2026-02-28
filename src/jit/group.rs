//! Compilation group discovery for batch JIT compilation.
//!
//! When a function becomes hot, we scan its LIR for calls to other global
//! functions. If those functions are also JIT-compilable, we compile them
//! together into a single Cranelift module with direct calls between them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::lir::{LirFunction, LirInstr, Reg};
use crate::value::{SymbolId, Value};

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
pub fn discover_compilation_group(
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

        let lir = match &closure.lir_function {
            Some(lir) => lir.clone(),
            None => continue,
        };

        if lir.effect.may_suspend() {
            continue;
        }

        // Phase 1: must be capture-free
        if lir.num_captures > 0 {
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
    let mut reg_to_sym: HashMap<Reg, SymbolId> = HashMap::new();
    let mut targets: HashSet<SymbolId> = HashSet::new();

    for bb in &lir.blocks {
        for spanned in &bb.instructions {
            match &spanned.instr {
                LirInstr::LoadGlobal { dst, sym } => {
                    reg_to_sym.insert(*dst, *sym);
                }
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
            match &spanned.instr {
                LirInstr::MakeClosure { .. }
                | LirInstr::LoadResumeValue { .. }
                | LirInstr::CarOrNil { .. }
                | LirInstr::CdrOrNil { .. }
                | LirInstr::ArrayRefOrNil { .. }
                | LirInstr::ArraySliceFrom { .. }
                | LirInstr::IsArray { .. }
                | LirInstr::IsTable { .. }
                | LirInstr::ArrayLen { .. }
                | LirInstr::TableGetOrNil { .. }
                | LirInstr::Eval { .. } => return true,
                _ => {}
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::lir::{
        BasicBlock, Label, LirInstr, Reg, SpannedInstr, SpannedTerminator, Terminator,
    };
    use crate::syntax::Span;
    use crate::value::Arity;

    /// Build a simple LIR function that calls a global symbol.
    fn make_caller(name: &str, callee_sym: SymbolId) -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some(name.to_string());
        func.num_regs = 4;
        func.num_captures = 0;
        func.effect = Effect::none();

        let mut entry = BasicBlock::new(Label(0));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadGlobal {
                dst: Reg(1),
                sym: callee_sym,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::Call {
                dst: Reg(2),
                func: Reg(1),
                args: vec![Reg(0)],
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), Span::synthetic());

        func.blocks.push(entry);
        func.entry = Label(0);
        func
    }

    /// Build a simple identity LIR function (no calls).
    fn make_leaf() -> LirFunction {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.name = Some("leaf".to_string());
        func.num_regs = 1;
        func.num_captures = 0;
        func.effect = Effect::none();

        let mut entry = BasicBlock::new(Label(0));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());

        func.blocks.push(entry);
        func.entry = Label(0);
        func
    }

    /// Build a mock closure Value with the given LIR function.
    fn make_closure_value(lir: LirFunction) -> Value {
        use crate::error::LocationMap;
        use std::collections::HashMap;

        let closure = crate::value::Closure {
            bytecode: Rc::new(vec![]),
            arity: lir.arity,
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: lir.effect,
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: Some(Rc::new(lir)),
            doc: None,
        };
        Value::closure(closure)
    }

    #[test]
    fn test_find_global_call_targets() {
        let sym_g = SymbolId(10);
        let caller = make_caller("f", sym_g);
        let targets = find_global_call_targets(&caller);
        assert_eq!(targets.len(), 1);
        assert!(targets.contains(&sym_g));
    }

    #[test]
    fn test_find_global_call_targets_no_calls() {
        let leaf = make_leaf();
        let targets = find_global_call_targets(&leaf);
        assert!(targets.is_empty());
    }

    #[test]
    fn test_discover_empty_when_no_peers() {
        let leaf = make_leaf();
        let globals: Vec<Value> = vec![];
        let group = discover_compilation_group(&leaf, &globals);
        assert!(group.is_empty());
    }

    #[test]
    fn test_discover_finds_callee() {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);
        let callee = make_leaf();

        // Set up globals: index 5 has the callee closure
        let mut globals = vec![Value::NIL; 10];
        globals[5] = make_closure_value(callee);

        let group = discover_compilation_group(&caller, &globals);
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].0, sym_g);
    }

    #[test]
    fn test_discover_skips_suspending() {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        let mut callee = make_leaf();
        callee.effect = Effect::yields();

        let mut globals = vec![Value::NIL; 10];
        globals[5] = make_closure_value(callee);

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    }

    #[test]
    fn test_discover_skips_captures() {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        let mut callee = make_leaf();
        callee.num_captures = 1;

        let mut globals = vec![Value::NIL; 10];
        globals[5] = make_closure_value(callee);

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    }

    #[test]
    fn test_discover_skips_unsupported_instructions() {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        // Build a callee with MakeClosure (unsupported)
        let mut callee = LirFunction::new(Arity::Exact(1));
        callee.name = Some("callee_with_closure".to_string());
        callee.num_regs = 3;
        callee.num_captures = 0;
        callee.effect = Effect::none();

        let mut entry = BasicBlock::new(Label(0));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::MakeClosure {
                dst: Reg(1),
                func: Box::new(make_leaf()),
                captures: vec![],
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), Span::synthetic());
        callee.blocks.push(entry);
        callee.entry = Label(0);

        let mut globals = vec![Value::NIL; 10];
        globals[5] = make_closure_value(callee);

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    }

    #[test]
    fn test_discover_transitive() {
        // f calls g, g calls h. All should be discovered.
        let sym_g = SymbolId(5);
        let sym_h = SymbolId(6);

        let caller = make_caller("f", sym_g);
        let g = make_caller("g", sym_h);
        let h = make_leaf();

        let mut globals = vec![Value::NIL; 10];
        globals[5] = make_closure_value(g);
        globals[6] = make_closure_value(h);

        let group = discover_compilation_group(&caller, &globals);
        assert_eq!(group.len(), 2);

        let syms: HashSet<SymbolId> = group.iter().map(|(s, _)| *s).collect();
        assert!(syms.contains(&sym_g));
        assert!(syms.contains(&sym_h));
    }

    #[test]
    fn test_discover_no_duplicates_in_cycle() {
        // f calls g, g calls f (mutual recursion). Should not loop.
        let sym_f = SymbolId(4);
        let sym_g = SymbolId(5);

        let hot = make_caller("f", sym_g);
        let g = make_caller("g", sym_f);

        // f is the hot function (not in globals for discovery purposes,
        // but g calls sym_f which points to f's closure)
        let f_for_global = make_caller("f", sym_g);

        let mut globals = vec![Value::NIL; 10];
        globals[4] = make_closure_value(f_for_global);
        globals[5] = make_closure_value(g);

        let group = discover_compilation_group(&hot, &globals);
        // Should find g and f (from g's call back to f)
        let syms: HashSet<SymbolId> = group.iter().map(|(s, _)| *s).collect();
        assert!(syms.contains(&sym_g));
        assert!(syms.contains(&sym_f));
        // No duplicates
        assert_eq!(group.len(), syms.len());
    }

    #[test]
    fn test_discover_out_of_bounds_sym() {
        let sym_g = SymbolId(999);
        let caller = make_caller("f", sym_g);
        let globals = vec![Value::NIL; 10]; // Only 10 globals

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    }

    #[test]
    fn test_discover_non_closure_global() {
        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        let mut globals = vec![Value::NIL; 10];
        globals[5] = Value::int(42); // Not a closure

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    }

    #[test]
    fn test_discover_closure_without_lir() {
        use crate::error::LocationMap;
        use std::collections::HashMap;

        let sym_g = SymbolId(5);
        let caller = make_caller("f", sym_g);

        // Closure with no lir_function
        let closure = crate::value::Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(1),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: Effect::none(),
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
        };

        let mut globals = vec![Value::NIL; 10];
        globals[5] = Value::closure(closure);

        let group = discover_compilation_group(&caller, &globals);
        assert!(group.is_empty());
    }

    #[test]
    fn test_find_targets_with_tail_call() {
        let sym_g = SymbolId(10);

        let mut func = LirFunction::new(Arity::Exact(1));
        func.num_regs = 3;
        func.num_captures = 0;
        func.effect = Effect::none();

        let mut entry = BasicBlock::new(Label(0));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadGlobal {
                dst: Reg(1),
                sym: sym_g,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::TailCall {
                func: Reg(1),
                args: vec![Reg(0)],
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Unreachable, Span::synthetic());

        func.blocks.push(entry);
        func.entry = Label(0);

        let targets = find_global_call_targets(&func);
        assert_eq!(targets.len(), 1);
        assert!(targets.contains(&sym_g));
    }

    #[test]
    fn test_discover_respects_size_bound() {
        // Create a chain of functions f0 -> f1 -> f2 -> ... -> f(N)
        // Verify that discovery stops at MAX_GROUP_SIZE.
        let n = MAX_GROUP_SIZE + 5; // more than the limit
        let syms: Vec<SymbolId> = (0..n).map(|i| SymbolId(i as u32)).collect();

        // Build chain: f_i calls f_{i+1}
        let mut globals = vec![Value::NIL; n];
        for i in 0..n - 1 {
            let caller = make_caller(&format!("f{}", i), syms[i + 1]);
            globals[i] = make_closure_value(caller);
        }
        // Last function is a leaf
        globals[n - 1] = make_closure_value(make_leaf());

        // Hot function calls f0
        let hot = make_caller("hot", syms[0]);
        let group = discover_compilation_group(&hot, &globals);

        // Should be capped by MAX_GROUP_SIZE
        assert!(
            group.len() <= MAX_GROUP_SIZE,
            "Group size {} exceeds MAX_GROUP_SIZE {}",
            group.len(),
            MAX_GROUP_SIZE
        );
    }

    #[test]
    fn test_discover_respects_depth_bound() {
        // Create a chain longer than MAX_DISCOVERY_DEPTH.
        // Even though all functions are valid, depth limiting should cap discovery.
        let n = MAX_DISCOVERY_DEPTH + 3;
        let syms: Vec<SymbolId> = (0..n).map(|i| SymbolId(i as u32)).collect();

        let mut globals = vec![Value::NIL; n];
        for i in 0..n - 1 {
            let caller = make_caller(&format!("f{}", i), syms[i + 1]);
            globals[i] = make_closure_value(caller);
        }
        globals[n - 1] = make_closure_value(make_leaf());

        let hot = make_caller("hot", syms[0]);
        let group = discover_compilation_group(&hot, &globals);

        // Depth 1 = direct callees, depth 2 = their callees, etc.
        // Should not discover beyond MAX_DISCOVERY_DEPTH levels.
        assert!(
            group.len() <= MAX_DISCOVERY_DEPTH,
            "Group size {} exceeds MAX_DISCOVERY_DEPTH {} (depth bounding failed)",
            group.len(),
            MAX_DISCOVERY_DEPTH
        );
    }

    #[test]
    fn test_has_unsupported_instructions_clean() {
        let leaf = make_leaf();
        assert!(!has_unsupported_instructions(&leaf));
    }

    #[test]
    fn test_has_unsupported_instructions_with_eval() {
        let mut func = LirFunction::new(Arity::Exact(1));
        func.num_regs = 3;
        func.num_captures = 0;
        func.effect = Effect::none();

        let mut entry = BasicBlock::new(Label(0));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::Eval {
                dst: Reg(0),
                expr: Reg(1),
                env: Reg(2),
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());
        func.blocks.push(entry);
        func.entry = Label(0);

        assert!(has_unsupported_instructions(&func));
    }
}
