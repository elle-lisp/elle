//! Liveness analysis for functional HIR.
//!
//! Computes which bindings are live after each HIR node. Not CFG-based —
//! computed structurally on the HIR tree with fixpoint iteration for loops.

use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind};

use std::collections::HashMap;

/// Dense bitvector keyed by binding index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    pub fn new(num_bits: usize) -> Self {
        let num_words = num_bits.div_ceil(64);
        BitSet {
            words: vec![0; num_words],
        }
    }

    pub fn set(&mut self, bit: usize) {
        let word = bit / 64;
        if word < self.words.len() {
            self.words[word] |= 1u64 << (bit % 64);
        }
    }

    pub fn clear(&mut self, bit: usize) {
        let word = bit / 64;
        if word < self.words.len() {
            self.words[word] &= !(1u64 << (bit % 64));
        }
    }

    pub fn contains(&self, bit: usize) -> bool {
        let word = bit / 64;
        if word < self.words.len() {
            self.words[word] & (1u64 << (bit % 64)) != 0
        } else {
            false
        }
    }

    /// Union with another bitset. Returns true if self changed.
    pub fn union_with(&mut self, other: &BitSet) -> bool {
        let mut changed = false;
        for (i, &w) in other.words.iter().enumerate() {
            if i < self.words.len() {
                let old = self.words[i];
                self.words[i] |= w;
                if self.words[i] != old {
                    changed = true;
                }
            }
        }
        changed
    }

    /// Iterate over set bit indices.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(wi, &word)| {
            let base = wi * 64;
            (0..64).filter_map(move |bit| {
                if word & (1u64 << bit) != 0 {
                    Some(base + bit)
                } else {
                    None
                }
            })
        })
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }
}

/// Liveness analysis context.
pub(crate) struct LivenessAnalyzer {
    pub binding_index: HashMap<Binding, usize>,
    pub live_out: HashMap<HirId, BitSet>,
    num_bindings: usize,
}

impl LivenessAnalyzer {
    pub fn new(binding_index: HashMap<Binding, usize>, num_bindings: usize) -> Self {
        LivenessAnalyzer {
            binding_index,
            live_out: HashMap::new(),
            num_bindings,
        }
    }

    pub(crate) fn empty_set(&self) -> BitSet {
        BitSet::new(self.num_bindings)
    }

    /// Compute liveness for a HIR node. `live_after` is the set of bindings
    /// live after this node. Returns the set of bindings live before this node.
    pub fn analyze(&mut self, hir: &Hir, live_after: &BitSet) -> BitSet {
        self.live_out.insert(hir.id, live_after.clone());

        match &hir.kind {
            // Leaves
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::Error => live_after.clone(),

            HirKind::Var(b) => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(b) {
                    live.set(idx);
                }
                live
            }

            HirKind::Begin(exprs) => self.analyze_sequence(exprs, live_after),

            HirKind::Block { body, .. } => self.analyze_sequence(body, live_after),

            HirKind::Let { bindings, body } => {
                let live_body = self.analyze(body, live_after);
                let mut live = live_body;
                // Process bindings right-to-left: init's live_out is the
                // live set needed after it (including the bound variable,
                // since it will be used in the body). Then remove the bound
                // variable to get live_in at the Let level.
                for (b, init) in bindings.iter().rev() {
                    // live currently has whatever the body/later bindings need.
                    // The init's live_out IS live (which may include b if used in body).
                    live = self.analyze(init, &live);
                    // After processing init, remove b — it's defined by this Let,
                    // so it's not live before the Let.
                    if let Some(&idx) = self.binding_index.get(b) {
                        live.clear(idx);
                    }
                }
                live
            }

            HirKind::Letrec { bindings, body } => {
                let mut live = self.analyze(body, live_after);
                // Remove all bound names first (mutually recursive)
                for (b, _) in bindings {
                    if let Some(&idx) = self.binding_index.get(b) {
                        live.clear(idx);
                    }
                }
                // Walk inits
                for (_, init) in bindings.iter().rev() {
                    live = self.analyze(init, &live);
                }
                live
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let live_then = self.analyze(then_branch, live_after);
                let live_else = self.analyze(else_branch, live_after);
                let mut live_cond_after = live_then;
                live_cond_after.union_with(&live_else);
                self.analyze(cond, &live_cond_after)
            }

            HirKind::Lambda { captures, body, .. } => {
                // Lambda body is a separate liveness scope
                let body_live_after = self.empty_set();
                self.analyze(body, &body_live_after);

                // The lambda node generates uses for its captures
                let mut live = live_after.clone();
                for cap in captures {
                    if let Some(&idx) = self.binding_index.get(&cap.binding) {
                        live.set(idx);
                    }
                }
                live
            }

            HirKind::Call { func, args, .. } => {
                let mut live = live_after.clone();
                // Process args right-to-left
                for a in args.iter().rev() {
                    live = self.analyze(&a.expr, &live);
                }
                self.analyze(func, &live)
            }

            HirKind::Define { binding, value } => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(binding) {
                    live.clear(idx);
                }
                self.analyze(value, &live)
            }

            HirKind::Assign { target, value } => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(target) {
                    live.clear(idx);
                }
                self.analyze(value, &live)
            }

            HirKind::Loop { bindings, body } => self.analyze_loop(bindings, body, live_after),

            HirKind::Recur { args } => {
                // Recur generates uses of its args — they flow to loop bindings.
                // The actual binding happens at the loop node. Here we just
                // ensure the args are live.
                let mut live = live_after.clone();
                for a in args.iter().rev() {
                    live = self.analyze(a, &live);
                }
                live
            }

            HirKind::Break { value, .. } => {
                // Break exits the block — value needs to be live
                self.analyze(value, live_after)
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                // Short-circuit: any expr could be the last one evaluated.
                // Conservative: union of live-in from each suffix.
                let mut live = live_after.clone();
                for e in exprs.iter().rev() {
                    let live_e = self.analyze(e, &live);
                    live.union_with(&live_e);
                    live = live_e;
                }
                live
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut live = if let Some(eb) = else_branch {
                    self.analyze(eb, live_after)
                } else {
                    live_after.clone()
                };
                for (c, b) in clauses.iter().rev() {
                    let live_body = self.analyze(b, live_after);
                    live.union_with(&live_body);
                    live = self.analyze(c, &live);
                }
                live
            }

            HirKind::Match { value, arms } => {
                let mut live_after_scrutinee = self.empty_set();
                for (pat, guard, body) in arms {
                    let mut live_arm = self.analyze(body, live_after);
                    if let Some(g) = guard {
                        live_arm = self.analyze(g, &live_arm);
                    }
                    // Remove pattern bindings
                    for b in pat.bindings().bindings {
                        if let Some(&idx) = self.binding_index.get(&b) {
                            live_arm.clear(idx);
                        }
                    }
                    live_after_scrutinee.union_with(&live_arm);
                }
                self.analyze(value, &live_after_scrutinee)
            }

            HirKind::Emit { value, .. } => self.analyze(value, live_after),

            HirKind::MakeCell { value } => self.analyze(value, live_after),

            HirKind::DerefCell { cell } => self.analyze(cell, live_after),

            HirKind::SetCell { cell, value } => {
                let live = self.analyze(value, live_after);
                self.analyze(cell, &live)
            }

            HirKind::Destructure { pattern, value, .. } => {
                let mut live = live_after.clone();
                for b in pattern.bindings().bindings {
                    if let Some(&idx) = self.binding_index.get(&b) {
                        live.clear(idx);
                    }
                }
                self.analyze(value, &live)
            }

            HirKind::Eval { expr, env } => {
                let live = self.analyze(env, live_after);
                self.analyze(expr, &live)
            }

            HirKind::Parameterize { bindings, body } => {
                let mut live = self.analyze(body, live_after);
                for (k, v) in bindings.iter().rev() {
                    live = self.analyze(v, &live);
                    live = self.analyze(k, &live);
                }
                live
            }

            HirKind::While { cond, body } => {
                let live_body = self.analyze(body, live_after);
                let mut live = live_after.clone();
                live.union_with(&live_body);
                self.analyze(cond, &live)
            }

            HirKind::Intrinsic { args, .. } => {
                let mut live = live_after.clone();
                for a in args.iter().rev() {
                    live = self.analyze(a, &live);
                }
                live
            }
        }
    }

    fn analyze_sequence(&mut self, exprs: &[Hir], live_after: &BitSet) -> BitSet {
        let mut live = live_after.clone();
        for e in exprs.iter().rev() {
            live = self.analyze(e, &live);
        }
        live
    }

    /// Analyze a Loop with fixpoint iteration.
    fn analyze_loop(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        live_after: &BitSet,
    ) -> BitSet {
        // Initialize: body live-out includes loop bindings (for Recur)
        let mut body_live_out = live_after.clone();
        for (b, _) in bindings {
            if let Some(&idx) = self.binding_index.get(b) {
                body_live_out.set(idx);
            }
        }

        // Fixpoint: compute body liveness, update body_live_out, repeat
        let mut prev = self.empty_set();
        for _ in 0..10 {
            let live_in = self.analyze(body, &body_live_out);
            if live_in == prev {
                break;
            }
            prev = live_in.clone();
            // Body live-out should include anything live at loop entry
            // (bindings that survive across iterations)
            body_live_out = live_after.clone();
            body_live_out.union_with(&live_in);
        }

        // Remove loop bindings from result, add inits
        let mut live = prev;
        for (b, _) in bindings {
            if let Some(&idx) = self.binding_index.get(b) {
                live.clear(idx);
            }
        }
        for (_, init) in bindings.iter().rev() {
            live = self.analyze(init, &live);
        }
        live
    }
}

/// Build the binding index from def-use information.
pub(crate) fn build_binding_index(
    def_site: &HashMap<Binding, super::expr::HirId>,
) -> (HashMap<Binding, usize>, Vec<Binding>) {
    let mut index_binding: Vec<Binding> = def_site.keys().copied().collect();
    index_binding.sort_by_key(|b| b.0);
    let binding_index: HashMap<Binding, usize> = index_binding
        .iter()
        .enumerate()
        .map(|(i, &b)| (b, i))
        .collect();
    (binding_index, index_binding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::dataflow::{analyze_dataflow, DataflowInfo};
    use crate::hir::functionalize::functionalize;
    use crate::hir::tailcall::mark_tail_calls;
    use crate::hir::{Analyzer, BindingArena};
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

    fn analyze(source: &str) -> (BindingArena, SymbolTable, DataflowInfo) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);

        let wrapped = format!(
            "(letrec [cond_var (fn () nil) f (fn (& args) nil) g (fn (& args) nil)] {})",
            source
        );
        let syntax = read_syntax(&wrapped, "<test>").expect("parse failed");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand failed");
        let mut arena = BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
        mark_tail_calls(&mut analysis.hir);
        functionalize(&mut analysis.hir, &mut arena);

        let info = analyze_dataflow(&analysis.hir);
        (arena, symbols, info)
    }

    fn find_binding(
        info: &DataflowInfo,
        arena: &BindingArena,
        symbols: &SymbolTable,
        name: &str,
    ) -> Option<Binding> {
        info.def_site
            .keys()
            .find(|&&b| symbols.name(arena.get(b).name) == Some(name))
            .copied()
    }

    fn is_live_anywhere(info: &DataflowInfo, b: Binding) -> bool {
        info.binding_index
            .get(&b)
            .is_some_and(|&idx| info.live_out.values().any(|live| live.contains(idx)))
    }

    #[test]
    fn test_dead_binding() {
        let (arena, symbols, info) = analyze("(let [x 1] 42)");
        if let Some(x) = find_binding(&info, &arena, &symbols, "x") {
            assert!(
                !is_live_anywhere(&info, x),
                "dead binding x should not be live"
            );
        }
    }

    #[test]
    fn test_live_binding() {
        let (arena, symbols, info) = analyze("(let [x 1] x)");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(
            is_live_anywhere(&info, x),
            "x should be live between def and use"
        );
    }

    #[test]
    fn test_if_branch_liveness() {
        let (arena, symbols, info) = analyze("(let [x 1] (if (cond_var) x 2))");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(is_live_anywhere(&info, x), "x should be live before if");
    }

    #[test]
    fn test_loop_liveness() {
        let (arena, symbols, info) = analyze("(begin (def @i 0) (while (< i 10) (set i (+ i 1))))");
        let i_bindings: Vec<Binding> = info
            .def_site
            .keys()
            .filter(|&&b| symbols.name(arena.get(b).name) == Some("i"))
            .copied()
            .collect();
        assert!(!i_bindings.is_empty());
        assert!(
            i_bindings.iter().any(|&b| is_live_anywhere(&info, b)),
            "loop param i should be live across iterations"
        );
    }

    #[test]
    fn test_lambda_capture_liveness() {
        let (arena, symbols, info) = analyze("(let [x 1] (let [ff (fn () x)] (ff)))");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(
            is_live_anywhere(&info, x),
            "captured x should be live at lambda"
        );
    }

    #[test]
    fn test_bitset_basic() {
        let mut bs = BitSet::new(128);
        assert!(!bs.contains(0));
        bs.set(0);
        assert!(bs.contains(0));
        bs.set(65);
        assert!(bs.contains(65));
        bs.clear(0);
        assert!(!bs.contains(0));
        assert!(bs.contains(65));
    }

    #[test]
    fn test_bitset_union() {
        let mut a = BitSet::new(128);
        let mut b = BitSet::new(128);
        a.set(0);
        b.set(1);
        let changed = a.union_with(&b);
        assert!(changed);
        assert!(a.contains(0));
        assert!(a.contains(1));
        let changed2 = a.union_with(&b);
        assert!(!changed2);
    }

    #[test]
    fn test_bitset_iter() {
        let mut bs = BitSet::new(128);
        bs.set(3);
        bs.set(67);
        bs.set(100);
        let bits: Vec<usize> = bs.iter().collect();
        assert_eq!(bits, vec![3, 67, 100]);
    }
}
