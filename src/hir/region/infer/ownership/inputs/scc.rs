// audited: 2026-10-20
//! The strongly-connected components of the containment graph, found by one iterative Tarjan pass.
//!
//! docs/impl/region/ownership.md

use super::*;

/// One iterative Tarjan pass over the eligible containment graph (`children_of`,
/// `parent → child`), returning each node's component index and the component members.
///
/// The seed set is every graph node PLUS every `alloc_regions` candidate, so an isolated
/// candidate (no containment edge) still gets its own singleton component and `scc_of`
/// resolves for it. Iterative (an explicit frame stack, not recursion): the whole-stdlib
/// letrec's containment chains can be deep enough to blow a recursive DFS's call stack.
///
/// The component of `r` equals `{m : r reaches m and m reaches r}` — exactly the set the
/// group/activation walks previously computed as `{m ∈ reach(r) : r ∈ reach(m)}`, so this
/// is a behaviour-preserving replacement of that per-region reachability closure.
// `map_entry` does not apply: the child step branches ON `index_of`'s membership — an
// absent child is a tree edge (insert + push onto both the Tarjan and the DFS-frame
// stacks), a present child is a back edge (a distinct lowlink update, only when still
// on-stack). The entry API expresses neither the else-if nor the vacant-arm side effects.
#[allow(clippy::map_entry)]
pub(super) fn compute_sccs(
    children_of: &FxHashMap<Region, Vec<Region>>,
    alloc_regions: &FxHashSet<Region>,
) -> (FxHashMap<Region, usize>, Vec<FxHashSet<Region>>) {
    // Seed nodes: graph endpoints ∪ candidate roots (dedup, insertion order irrelevant —
    // SCCs are order-independent).
    let mut nodes: Vec<Region> = Vec::new();
    let mut seen: FxHashSet<Region> = FxHashSet::default();
    for (&p, kids) in children_of {
        if seen.insert(p) {
            nodes.push(p);
        }
        for &c in kids {
            if seen.insert(c) {
                nodes.push(c);
            }
        }
    }
    for &r in alloc_regions {
        if seen.insert(r) {
            nodes.push(r);
        }
    }

    let mut index_of: FxHashMap<Region, u32> = FxHashMap::default();
    let mut lowlink: FxHashMap<Region, u32> = FxHashMap::default();
    let mut on_stack: FxHashSet<Region> = FxHashSet::default();
    let mut tarjan_stack: Vec<Region> = Vec::new();
    let mut comp_id: FxHashMap<Region, usize> = FxHashMap::default();
    let mut comp_members: Vec<FxHashSet<Region>> = Vec::new();
    let mut next_index: u32 = 0;
    let empty: Vec<Region> = Vec::new();

    for &root in &nodes {
        if index_of.contains_key(&root) {
            continue;
        }
        // Explicit DFS frames `(node, next-child-cursor)`.
        index_of.insert(root, next_index);
        lowlink.insert(root, next_index);
        next_index += 1;
        tarjan_stack.push(root);
        on_stack.insert(root);
        let mut work: Vec<(Region, usize)> = vec![(root, 0)];
        while let Some(&(v, ci)) = work.last() {
            let kids = children_of.get(&v).unwrap_or(&empty);
            if ci < kids.len() {
                work.last_mut().unwrap().1 += 1;
                let w = kids[ci];
                if !index_of.contains_key(&w) {
                    // Tree edge: descend into an unvisited child.
                    index_of.insert(w, next_index);
                    lowlink.insert(w, next_index);
                    next_index += 1;
                    tarjan_stack.push(w);
                    on_stack.insert(w);
                    work.push((w, 0));
                } else if on_stack.contains(&w) {
                    // Back/cross edge to a node still on the stack: pull v's lowlink down.
                    let lw = index_of[&w];
                    if lw < lowlink[&v] {
                        lowlink.insert(v, lw);
                    }
                }
            } else {
                // v's children are exhausted. If it roots an SCC, pop the component.
                if lowlink[&v] == index_of[&v] {
                    let mut comp: FxHashSet<Region> = FxHashSet::default();
                    loop {
                        let w = tarjan_stack.pop().unwrap();
                        on_stack.remove(&w);
                        comp.insert(w);
                        comp_id.insert(w, comp_members.len());
                        if w == v {
                            break;
                        }
                    }
                    comp_members.push(comp);
                }
                work.pop();
                // Returning from v to its parent: propagate v's lowlink upward.
                if let Some(&(parent, _)) = work.last() {
                    let lv = lowlink[&v];
                    if lv < lowlink[&parent] {
                        lowlink.insert(parent, lv);
                    }
                }
            }
        }
    }
    (comp_id, comp_members)
}

#[cfg(test)]
mod tests;
