use super::*;

mod builder;
use builder::LastUseBuilder;

pub fn compute_last_use(
    hir: &Hir,
    uses: &HashMap<Binding, Vec<HirId>>,
    order: &HashMap<HirId, u32>,
) -> LastUseInfo {
    let low = compute_subtree_low(hir, order);
    let mut builder = LastUseBuilder {
        last_use: HashMap::new(),
        capture_loop_ext: HashMap::new(),
        binding_init: HashMap::new(),
        binding_scope: HashMap::new(),
        iter_scope_stack: Vec::new(),
        order,
        low: &low,
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
    //
    // Chains COUPLE these overrides: a use of binding B can be the very init
    // node of binding A — a capture-use registers at the Lambda's own HirId,
    // which is `(def a (fn () b))`'s init id, and a bare `(def a b)` init IS
    // the `Var(b)` node. So B's override must read A's *overridden* last_use,
    // transitively (`(def @acc …) (def u1 (fn () acc)) … (u1)`: acc's cell
    // must live until the `(u1)` call, reached only through u1's override).
    // The override equations must be solved to a fixpoint independent of the
    // hash-iteration order, or a chain resolves only to a random prefix and a
    // capture cell is released too early — a use-after-free with
    // nondeterministic codegen (region-capture-cell-noreassign-uaf.lisp).
    //
    // Solve the override equations to their fixpoint in two phases, both
    // independent of hash iteration order:
    //
    // Phase 1 — the one-time unconditional overrides, computed for every
    // binding from the UNTOUCHED walk map and applied together. This is the
    // original single-pass semantics (including the unused-binding narrowing
    // to the init itself), made order-free by reading only pre-override
    // values.
    //
    // Phase 2 — chain propagation as a worklist of GROW-ONLY updates: when an
    // init's last_use grows, re-process exactly the bindings whose use sites
    // read that entry. Each override is a max over its inputs, so monotone
    // chaotic re-evaluation converges to the unique least fixpoint regardless
    // of processing order — and without the per-round full-map clone that
    // makes a round-based sweep quadratic on a file letrec's chain of
    // sequential defs (i.e. the stdlib).
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let compute_chosen = |binding: &Binding,
                          last_use: &HashMap<HirId, HirId>,
                          capture_loop_ext: &HashMap<Binding, HirId>|
     -> Option<HirId> {
        let mut max_effective = uses
            .get(binding)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|use_id| last_use.get(use_id).copied().unwrap_or(*use_id))
            .max_by_key(|id| ord(*id));
        // Fold in the lambda-capture-in-loop extension: a binding captured by a
        // lambda built inside a loop (while bound outside it) must outlive the
        // loop, even though its capture-use's last_use sits inside the body.
        if let Some(&ext) = capture_loop_ext.get(binding) {
            if max_effective.is_none_or(|cur| ord(ext) > ord(cur)) {
                max_effective = Some(ext);
            }
        }
        max_effective
    };

    // use-node id → bindings whose override reads last_use[that id].
    let mut dependents: HashMap<HirId, Vec<Binding>> = HashMap::new();
    for binding in builder.binding_init.keys() {
        for use_id in uses.get(binding).into_iter().flat_map(|v| v.iter()) {
            dependents.entry(*use_id).or_default().push(*binding);
        }
    }

    // Phase 1: stage every override against the walk map, then apply. Several
    // bindings can share one init id (a Destructure registers the value's id
    // for every pattern binding), so collisions aggregate by ord-max — the
    // value must survive the latest sharer's uses.
    let mut staged: HashMap<HirId, HirId> = HashMap::new();
    for (binding, init_ids) in &builder.binding_init {
        let chosen = compute_chosen(binding, &builder.last_use, &builder.capture_loop_ext);
        for &init_id in init_ids {
            let chosen = chosen.unwrap_or(init_id);
            staged
                .entry(init_id)
                .and_modify(|cur| {
                    if ord(chosen) > ord(*cur) {
                        *cur = chosen;
                    }
                })
                .or_insert(chosen);
        }
    }
    let mut worklist = std::collections::VecDeque::new();
    let mut queued = std::collections::HashSet::new();
    for (init_id, chosen) in staged {
        if builder.last_use.get(&init_id) != Some(&chosen) {
            builder.last_use.insert(init_id, chosen);
            for dep in dependents.get(&init_id).into_iter().flatten() {
                if queued.insert(*dep) {
                    worklist.push_back(*dep);
                }
            }
        }
    }

    // Phase 2: grow-only propagation to the fixpoint.
    while let Some(binding) = worklist.pop_front() {
        queued.remove(&binding);
        let Some(init_ids) = builder.binding_init.get(&binding) else {
            continue;
        };
        let chosen = compute_chosen(&binding, &builder.last_use, &builder.capture_loop_ext);
        let Some(chosen) = chosen else { continue };
        for init_id in init_ids.clone() {
            let grows = builder
                .last_use
                .get(&init_id)
                .is_none_or(|cur| ord(chosen) > ord(*cur));
            if grows {
                builder.last_use.insert(init_id, chosen);
                for dep in dependents.get(&init_id).into_iter().flatten() {
                    if queued.insert(*dep) {
                        worklist.push_back(*dep);
                    }
                }
            }
        }
    }

    LastUseInfo {
        per_node: builder.last_use,
        capture_loop_ext: builder.capture_loop_ext,
    }
}

/// Result of `compute_last_use`.
pub struct LastUseInfo {
    /// Per-node effective last-use: a node's HirId → the HirId at which the
    /// value it produced is last used (after which its region may be freed).
    pub per_node: HashMap<HirId, HirId>,
    /// Bindings captured by a lambda created INSIDE an iterative scope while
    /// bound OUTSIDE it → the outermost enclosing iter-scope HirId the binding
    /// must outlive. Such a binding is re-captured every iteration, so its
    /// region must survive the loop. The capture-use registers at the lambda's
    /// own (non-`Var`) HirId, which the `Var` iter-scope extension in `walk`
    /// does not reach, so it is recorded here per-binding (NOT per-lambda-node:
    /// a lambda may also capture a loop-LOCAL binding whose region must still be
    /// freed per iteration). Folded into region `decref_point` selection.
    pub capture_loop_ext: HashMap<Binding, HirId>,
}
