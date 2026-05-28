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

/// Compute per-HirId last-use: for each value-producing HirId, the HirId
/// at which its value is last referenced.
///
/// For an allocation `A`:
/// - If `A` is bound to some binding `b` (i.e., `A` is a binding's init),
///   `last_use[A]` is the maximum "effective HirId" over all `Var(b)`
///   references. The effective HirId of a `Var(b)` is the immediate
///   parent expression's HirId when the parent *consumes* the value
///   (Call, Emit, Define, Assign, SetCell, MakeCell, Intrinsic, etc.);
///   otherwise it is the `Var(b)` HirId itself.
/// - If `A` has no binding (inline allocation passed as an argument or
///   value child of a consumer), `last_use[A]` is the consumer's HirId.
/// - If `A` is bound but the binding has no uses anywhere,
///   `last_use[A] = A` (decref immediately after the alloc).
///
/// The plan's region-inference invariant requires every region to have
/// exactly one `free_at` HirId; this function produces that mapping for
/// allocation HirIds.
pub fn compute_last_use(
    hir: &Hir,
    uses: &HashMap<Binding, Vec<HirId>>,
) -> HashMap<HirId, HirId> {
    let mut builder = LastUseBuilder {
        last_use: HashMap::new(),
        binding_init: HashMap::new(),
    };
    // The root has no parent; parent_consumes=false is the conservative
    // default (the root's value is the program's result, no further use).
    builder.walk(hir, false, hir.id);

    // Override last_use for binding-bound allocations to span all uses
    // of the binding. A single binding identity can have multiple init
    // sites at file scope (top-level re-defs like the destructure tests
    // that reuse `a`, `r` across `(def (a & r) ...)` and `(def (a b & r)
    // ...)` share the same Binding via analyze_file_letrec). Extend
    // last_use for every init site so each value's region survives
    // until the latest binding reference.
    for (binding, init_ids) in builder.binding_init.clone() {
        let max_effective = uses
            .get(&binding)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|use_id| {
                builder
                    .last_use
                    .get(use_id)
                    .copied()
                    .unwrap_or(*use_id)
            })
            .max();
        for init_id in init_ids {
            let chosen = max_effective.unwrap_or(init_id);
            builder.last_use.insert(init_id, chosen);
        }
    }

    builder.last_use
}

/// Helper for computing per-HirId last-use.
struct LastUseBuilder {
    last_use: HashMap<HirId, HirId>,
    /// For each binding, the HirIds of its initializers (Let/Letrec/Loop
    /// init or Define value). A single Binding can have multiple init
    /// sites at file scope (top-level re-defs share the same Binding
    /// via analyze_file_letrec), so this is a Vec rather than a single
    /// id; compute_last_use extends last_use for every init.
    binding_init: HashMap<Binding, Vec<HirId>>,
}

impl LastUseBuilder {
    fn walk(&mut self, hir: &Hir, parent_consumes: bool, parent_id: HirId) {
        // The "effective last use" of this node's value is the parent's
        // HirId when the parent consumes (the value flows in and dies);
        // otherwise it's this node itself (the value either propagates
        // up through non-consuming wrappers or is the program's result).
        let my_last = if parent_consumes { parent_id } else { hir.id };
        self.last_use.insert(hir.id, my_last);

        match &hir.kind {
            // Consumer parents: every child position is a consumer.
            HirKind::Call { func, args, .. } => {
                self.walk(func, true, hir.id);
                for a in args {
                    self.walk(&a.expr, true, hir.id);
                }
            }
            HirKind::Emit { value, .. } => self.walk(value, true, hir.id),
            HirKind::Define { value, binding } => {
                self.binding_init.entry(*binding).or_default().push(value.id);
                self.walk(value, true, hir.id);
            }
            HirKind::Assign { value, .. } => self.walk(value, true, hir.id),
            HirKind::SetCell { cell, value } => {
                self.walk(cell, true, hir.id);
                self.walk(value, true, hir.id);
            }
            HirKind::MakeCell { value } => self.walk(value, true, hir.id),
            HirKind::DerefCell { cell } => self.walk(cell, true, hir.id),
            HirKind::Destructure { pattern, value, .. } => {
                // Register each destructured binding as bound to this
                // value's init id so the last-use override picks up
                // uses of the destructured names — same mechanism as
                // Let/Letrec. Without this, the value's allocation's
                // last_use stops at the destructure node and any
                // region it lives in is freed before the destructured
                // bindings are read.
                for b in pattern.bindings().bindings {
                    self.binding_init.entry(b).or_default().push(value.id);
                }
                self.walk(value, true, hir.id);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    self.walk(a, true, hir.id);
                }
            }
            HirKind::Recur { args } => {
                for a in args {
                    self.walk(a, true, hir.id);
                }
            }
            HirKind::Eval { expr, env } => {
                self.walk(expr, true, hir.id);
                self.walk(env, true, hir.id);
            }
            HirKind::Break { value, .. } => self.walk(value, true, hir.id),

            // Mixed: cond/scrutinee is consumed; branches/body propagate.
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk(cond, true, hir.id);
                self.walk(then_branch, false, hir.id);
                self.walk(else_branch, false, hir.id);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    self.walk(c, true, hir.id);
                    self.walk(b, false, hir.id);
                }
                if let Some(eb) = else_branch {
                    self.walk(eb, false, hir.id);
                }
            }
            HirKind::Match { value, arms } => {
                self.walk(value, true, hir.id);
                for (_pat, guard, body) in arms {
                    if let Some(g) = guard {
                        self.walk(g, false, hir.id);
                    }
                    self.walk(body, false, hir.id);
                }
            }
            HirKind::While { cond, body } => {
                self.walk(cond, true, hir.id);
                self.walk(body, false, hir.id);
            }
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    self.walk(k, true, hir.id);
                    self.walk(v, true, hir.id);
                }
                self.walk(body, false, hir.id);
            }

            // Binding forms: init is consumed (bound to a name); body
            // propagates (its value is the form's result).
            HirKind::Let { bindings, body } => {
                for (b, init) in bindings {
                    self.binding_init.entry(*b).or_default().push(init.id);
                    self.walk(init, true, hir.id);
                }
                self.walk(body, false, hir.id);
            }
            HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    self.binding_init.entry(*b).or_default().push(init.id);
                    self.walk(init, true, hir.id);
                }
                self.walk(body, false, hir.id);
            }
            HirKind::Loop { bindings, body } => {
                for (b, init) in bindings {
                    self.binding_init.entry(*b).or_default().push(init.id);
                    self.walk(init, true, hir.id);
                }
                self.walk(body, false, hir.id);
            }

            // Propagating: children flow up.
            HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    self.walk(e, false, hir.id);
                }
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    self.walk(e, false, hir.id);
                }
            }

            // Lambda: body is the closure's return path, not consumed
            // by the Lambda node itself. Captures generate uses at the
            // Lambda's own HirId (see DefUseBuilder).
            HirKind::Lambda { body, .. } => {
                self.walk(body, false, hir.id);
            }

            // Leaves.
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::Var(_)
            | HirKind::Error => {}
        }
    }
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
        crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);

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

    // ── per-HirId last-use tests (drive impl step 10) ────────────────

    fn analyze_with_hir(
        source: &str,
    ) -> (
        super::Hir,
        BindingArena,
        SymbolTable,
        DataflowInfo,
    ) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);
        let wrapped = format!(
            "(letrec [cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)] {})",
            source
        );
        let syntax = read_syntax(&wrapped, "<test>").expect("parse");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand");
        let mut arena = BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze");
        mark_tail_calls(&mut analysis.hir);
        functionalize(&mut analysis.hir, &mut arena);
        crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);
        let info = analyze_dataflow(&analysis.hir);
        (analysis.hir, arena, symbols, info)
    }

    /// Find every Call whose func is the named primitive.
    fn find_calls_to_primitive(
        hir: &super::Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
    ) -> Vec<HirId> {
        let mut out = Vec::new();
        fn walk(
            hir: &super::Hir,
            name: &str,
            arena: &BindingArena,
            symbols: &SymbolTable,
            out: &mut Vec<HirId>,
        ) {
            if let HirKind::Call { func, .. } = &hir.kind {
                if let HirKind::Var(b) = &func.kind {
                    if symbols.name(arena.get(*b).name).as_deref() == Some(name) {
                        out.push(hir.id);
                    }
                }
            }
            hir.for_each_child(|c| walk(c, name, arena, symbols, out));
        }
        walk(hir, name, arena, symbols, &mut out);
        out
    }

    /// Find every Var with the given binding name.
    fn find_vars_by_name(
        hir: &super::Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
    ) -> Vec<HirId> {
        let mut out = Vec::new();
        fn walk(
            hir: &super::Hir,
            name: &str,
            arena: &BindingArena,
            symbols: &SymbolTable,
            out: &mut Vec<HirId>,
        ) {
            if let HirKind::Var(b) = &hir.kind {
                if symbols.name(arena.get(*b).name).as_deref() == Some(name) {
                    out.push(hir.id);
                }
            }
            hir.for_each_child(|c| walk(c, name, arena, symbols, out));
        }
        walk(hir, name, arena, symbols, &mut out);
        out
    }

    /// Find the first Emit node.
    fn find_first_emit(hir: &super::Hir) -> Option<HirId> {
        if matches!(&hir.kind, HirKind::Emit { .. }) {
            return Some(hir.id);
        }
        let mut found = None;
        hir.for_each_child(|c| {
            if found.is_none() {
                found = find_first_emit(c);
            }
        });
        found
    }

    #[test]
    fn last_use_let_single_var_in_body() {
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (let [x (string \"a\")] x))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1, "expected exactly one (string ...) call");
        let alloc = allocs[0];

        let var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
        assert_eq!(var_uses.len(), 1, "expected exactly one Var(x)");
        let expected = var_uses[0];

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(expected),
            "alloc @{} should have last_use at Var(x) @{}, got {:?}",
            alloc.0,
            expected.0,
            got
        );
    }

    #[test]
    fn last_use_let_multiple_uses_in_body() {
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (let [x (string \"a\")] (begin x x)))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1);
        let alloc = allocs[0];

        let mut var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
        assert_eq!(var_uses.len(), 2, "expected two Var(x) uses");
        // The last use is the one whose live_out has no further reference to x.
        // Source order: first Var(x) earlier, second Var(x) later. The last
        // syntactic Var(x) is the second.
        var_uses.sort_by_key(|id| id.0);
        let last = *var_uses.last().unwrap();

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(last),
            "alloc @{} should have last_use at the second Var(x) @{}, got {:?}",
            alloc.0,
            last.0,
            got
        );
    }

    #[test]
    fn last_use_inline_call_arg_no_binding() {
        // `(string (string "a"))` — the inner string allocation has no
        // binding; its value flows directly into the outer string call.
        // Last use is at the outer Call. `string` is a real primitive
        // so the analyzer does not inline these calls (unlike the
        // letrec-bound `g` in the test wrapper).
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (string (string \"a\")))");

        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 2, "expected two (string ...) calls");
        // The inner call has the lower HirId; the outer call wraps it.
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let inner = sorted[0];
        let outer = sorted[1];

        let got = info.last_use.get(&inner).copied();
        assert_eq!(
            got,
            Some(outer),
            "inline alloc @{} should have last_use at the consuming Call @{}, got {:?}",
            inner.0,
            outer.0,
            got
        );
    }

    #[test]
    fn last_use_emit_yield() {
        // The yielded value's last use is the Emit node — the runtime
        // incref at handle_emit (step 14) keeps the region alive past
        // the matching DecrefRegion.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (emit :yield (string \"a\")))");

        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1, "expected exactly one (string ...) call");
        let alloc = allocs[0];

        let emit = find_first_emit(&hir).expect("expected an Emit node");

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(emit),
            "emit-yielded alloc @{} should have last_use at Emit @{}, got {:?}",
            alloc.0,
            emit.0,
            got
        );
    }

    #[test]
    fn last_use_across_nested_let() {
        // `(let [x (string "a")] (let [y 1] x))` — Var(x) lives inside the
        // inner let. last_use of the alloc must be that Var(x), not the
        // outer let's exit.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (let [x (string \"a\")] (let [y 1] x)))");

        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1);
        let alloc = allocs[0];

        let var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
        assert_eq!(var_uses.len(), 1, "expected exactly one Var(x)");
        let expected = var_uses[0];

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(expected),
            "nested-let alloc @{} should have last_use at inner Var(x) @{}, got {:?}",
            alloc.0,
            expected.0,
            got
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
