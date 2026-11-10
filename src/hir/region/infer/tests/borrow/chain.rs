// audited: 2026-10-21
//! A borrowing read's container outlives each name its result reaches, and no such name holds it.
//!
//! docs/impl/region/borrow.md
//!
//! Every shape binds the read's result to a name, or hands it on through a form
//! the borrow passes through, then reads it after the container's own last
//! mention. Each shape compiles through the whole pipeline with the graft off,
//! so the wrapper a grafted body leaves is written out here.
//!
//! The counter-factual: a read whose result carries nothing makes the bound
//! name hold nothing. The read's own pin then anchors the container at the
//! read's consumer, which for a bound result is the binder, ahead of the name's
//! later uses. In a begin-wrapped init that consumer is the inner `let`, and in
//! a chain the second read records no container at all.

use super::*;

/// `begin`-wrapped `%rest`, the shape a grafted `(defn step [p] … (%rest p))`
/// leaves: the read is the tail of a statement wrapper around the init.
const BEGIN_REST: &str = "(defn face [i] \
     (let [xs (%pair i (%pair (string \"s\" i) ()))] \
       (let [t (let [p (begin nil (%rest xs))] p)] \
         (length (first t)))))";

/// `begin`-wrapped `%first`. The reader is kept out of tail position: the
/// lowerer carries a release ahead of a tail call that does not name its
/// region, which would hide the solver's placement behind the lowerer's.
const BEGIN_FIRST: &str = "(defn face [i] \
     (let [xs (%pair (string \"s\" i) ())] \
       (let [t (let [p (begin nil (%first xs))] p)] \
         (string (length t)))))";

/// Two `%rest` reads: `q` borrows out of `p`, which borrows out of `xs`.
const CHAIN_REST: &str = "(defn face [i] \
     (let [xs (%pair i (%pair i (%pair (string \"s\" i) ())))] \
       (let [p (%rest xs)] \
         (when (%not (%pair? p)) (error {:e 1})) \
         (let [q (%rest p)] \
           (length (first q))))))";

/// Two `%first` reads.
const CHAIN_FIRST: &str = "(defn face [i] \
     (let [xs (%pair (%pair (string \"s\" i) ()) ())] \
       (let [p (%first xs)] \
         (when (%not (%pair? p)) (error {:e 1})) \
         (let [q (%first p)] \
           (length q)))))";

/// A second read in operand position, out of a borrowed name: its result is
/// never bound, so only the read's own pin can carry `xs` to the `first` call.
const OPERAND_CHAIN: &str = "(defn face [i] \
     (let [xs (%pair i (%pair i (%pair (string \"s\" i) ())))] \
       (let [p (%rest xs)] \
         (when (%not (%pair? p)) (error {:e 1})) \
         (length (first (%rest p))))))";

/// A borrow carried out of an `if`, one read per arm, each arm out of its own
/// container.
const IF_ARMS: &str = "(defn face [i c] \
     (let [xs (%pair (string \"s\" i) ()) ys (%pair (string \"t\" i) ())] \
       (let [t (let [u (if c (begin nil (%first xs)) (begin nil (%first ys)))] u)] \
         (string (length t)))))";

/// A borrow carried out of a `block` by a `break`, the block itself the tail
/// of a statement wrapper around the init.
const BLOCK_BREAK: &str = "(defn face [i] \
     (let [xs (%pair (string \"s\" i) ())] \
       (let [t (let [p (begin nil (block :b (break :b (%first xs))))] p)] \
         (string (length t)))))";

/// A tail block a `break` leaves with a borrow, beside a frame-replacing tail
/// call. That call keeps the break window from re-anchoring what the jump
/// passes over, so the block's own pin is all that carries the borrow out.
const TAIL_BLOCK_BREAK: &str = "(defn face [i c g] \
     (let [xs (%pair (string \"s\" i) ())] \
       (block :b (when c (break :b (%first xs))) (g 1))))";

/// A `def` destructure whose value is the read itself. The read is the
/// pattern's init, and with no name read the init's last use narrows to the
/// read, so the read's own pin stops short of the extraction.
const DESTRUCTURE_READ: &str = "(defn face [i] \
     (let [xs (%pair (%pair (string \"s\" i) (%pair i ())) ())] \
       (def (a b) (%first xs)) \
       nil))";

/// A container a closure captures, read through a borrow after the closure's
/// last call. The capture lets the ownership forest adopt the container into
/// the closure's subtree, which frees it at the closure's drop.
const CAPTURED_CONTAINER: &str = "(defn face [i] \
     (let [xs (%pair (string \"s\" i) ()) \
           f (fn [] (%pair? xs)) \
           t (let [p (begin nil (%first xs))] p)] \
       (f) \
       (length t)))";

/// A name a `match` pattern binds out of a borrowed scrutinee, handed on as
/// the match's value. `t` itself is last used at the `match`, so only the
/// pattern carries the borrow to `y`.
const MATCH_PATTERN: &str = "(defn face [i] \
     (let [xs (%pair (%pair (string \"s\" i) ()) ())] \
       (let [t (%first xs)] \
         (let [y (match t (h . tl) h _ nil)] \
           (string (length y))))))";

/// A `destructure` of a borrowed value whose names nothing reads. The
/// extraction still reads the value, after the value's own last read.
const DESTRUCTURE_UNUSED: &str = "(defn face [i] \
     (let [xs (%pair (%pair (string \"s\" i) (%pair i ())) ())] \
       (let [t (let [p (begin nil (%first xs))] p)] \
         (let [(a b) t] nil))))";

/// Every region `container` holds is released at or after `site`.
fn assert_container_released_after(
    hir: &Hir,
    arena: &BindingArena,
    info: &RegionInfo,
    container: &str,
    site: HirId,
    what: &str,
) {
    let order = compute_order(hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let b = find_binding_by_name(hir, container, arena)
        .unwrap_or_else(|| panic!("binding `{container}`"));
    let regions = info
        .binding_source_regions
        .get(&b)
        .filter(|rs| !rs.is_empty())
        .unwrap_or_else(|| panic!("`{container}` holds a region"));
    for r in regions {
        let dp = info
            .region_data
            .get(r)
            .unwrap_or_else(|| panic!("`{container}`'s region r{} has a decref_point", r.0))
            .decref_point;
        assert!(
            ord(dp) >= ord(site),
            "`{container}`'s region r{} is released at @{}, ahead of {what} @{} \
             that reads the value borrowed out of it",
            r.0,
            ord(dp),
            ord(site),
        );
    }
}

/// Every region `container` holds is released at or after the last `reader`
/// call, the one that consumes the borrowed value.
fn assert_container_outlives_reader(source: &str, container: &str, reader: &str) {
    let (hir, arena, info) = pipeline(source);
    let reader_site = *find_calls_to_primitive(&hir, reader, &arena)
        .last()
        .unwrap_or_else(|| panic!("the shape has a `{reader}` call"));
    let what = format!("the `{reader}` call");
    assert_container_released_after(&hir, &arena, &info, container, reader_site, &what);
}

/// The name a borrow reaches borrows every region its container holds, and
/// holds none of them.
fn assert_borrows_without_holding(source: &str, container: &str, name: &str) {
    let (hir, arena, info) = pipeline(source);
    let binding =
        |n: &str| find_binding_by_name(&hir, n, &arena).unwrap_or_else(|| panic!("binding `{n}`"));
    let container_regions = info
        .binding_source_regions
        .get(&binding(container))
        .cloned()
        .unwrap_or_default();
    assert!(
        !container_regions.is_empty(),
        "precondition: `{container}` holds a region"
    );
    let b = binding(name);
    let borrowed = info
        .binding_borrow_regions
        .get(&b)
        .cloned()
        .unwrap_or_default();
    let held = info
        .binding_source_regions
        .get(&b)
        .cloned()
        .unwrap_or_default();
    for r in &container_regions {
        assert!(
            borrowed.contains(r),
            "`{name}` is bound to a borrow out of `{container}`, so it borrows \
             `{container}`'s region r{}; it borrows {:?}",
            r.0,
            borrowed,
        );
        assert!(
            !held.contains(r),
            "`{name}` holds no reference to `{container}`'s region r{}, so it is no \
             holder of it; it holds {:?}",
            r.0,
            held,
        );
    }
}

#[test]
fn begin_wrapped_rest_keeps_its_container_past_the_reader() {
    assert_container_outlives_reader(BEGIN_REST, "xs", "first");
}

#[test]
fn begin_wrapped_first_keeps_its_container_past_the_reader() {
    assert_container_outlives_reader(BEGIN_FIRST, "xs", "length");
}

#[test]
fn chained_rest_keeps_the_first_container_past_the_last_reader() {
    assert_container_outlives_reader(CHAIN_REST, "xs", "first");
}

#[test]
fn chained_first_keeps_the_first_container_past_the_last_reader() {
    assert_container_outlives_reader(CHAIN_FIRST, "xs", "length");
}

#[test]
fn operand_read_out_of_a_borrowed_name_keeps_the_first_container() {
    assert_container_outlives_reader(OPERAND_CHAIN, "xs", "first");
}

#[test]
fn borrow_out_of_either_arm_keeps_both_containers_past_the_reader() {
    assert_container_outlives_reader(IF_ARMS, "xs", "length");
    assert_container_outlives_reader(IF_ARMS, "ys", "length");
}

#[test]
fn borrow_a_break_carries_keeps_its_container_past_the_reader() {
    assert_container_outlives_reader(BLOCK_BREAK, "xs", "length");
}

#[test]
fn borrow_a_break_carries_out_of_a_tail_block_keeps_its_container_past_the_block() {
    // The jump leaves the block, so a release at the `break` or before it is
    // passed over on the path that breaks. The block's value is the borrow, so
    // `xs` is released no earlier than the block.
    let (hir, arena, info) = pipeline(TAIL_BLOCK_BREAK);
    let site = find_first(&hir, |h| matches!(h.kind, HirKind::Block { .. }))
        .expect("the shape has a block");
    assert_container_released_after(&hir, &arena, &info, "xs", site, "the block");
}

#[test]
fn a_borrower_bounds_the_tight_last_use_the_ownership_obligation_reads() {
    // A capture-adopted member is freed by its owner's drop alone, and the
    // obligation bounds that drop by the member's tight last use. A borrower's
    // read of the member's pages is a use of it.
    //
    // The counter-factual: the tight last use counts the holders only, so it
    // ends at the closure. The obligation admits the adopt, the closure's drop
    // frees `xs`, and `length` reads the freed car through `t`.
    let (hir, arena, info) = pipeline(CAPTURED_CONTAINER);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let reader = *find_calls_to_primitive(&hir, "length", &arena)
        .last()
        .expect("the shape has a `length` call");
    let xs = find_binding_by_name(&hir, "xs", &arena).expect("binding `xs`");
    for r in info
        .binding_source_regions
        .get(&xs)
        .expect("`xs` holds a region")
    {
        let tight = info
            .binding_last_use
            .get(r)
            .unwrap_or_else(|| panic!("`xs`'s region r{} has a tight last use", r.0));
        assert!(
            ord(*tight) >= ord(reader),
            "`xs`'s region r{} has its tight last use at @{}, ahead of the `length` \
             call @{} that reads through the borrow",
            r.0,
            ord(*tight),
            ord(reader),
        );
    }
}

#[test]
fn match_projection_of_a_borrow_keeps_its_container_past_the_reader() {
    assert_container_outlives_reader(MATCH_PATTERN, "xs", "length");
}

#[test]
fn destructure_of_a_borrow_keeps_its_container_to_the_extraction() {
    // The names `a` and `b` are never read, so the value's own last use
    // narrows to the value itself. The `destructure` node reads the fields
    // after it, and `xs` must still be live there.
    for source in [DESTRUCTURE_UNUSED, DESTRUCTURE_READ] {
        let (hir, arena, info) = pipeline(source);
        let site = find_first(&hir, |h| matches!(h.kind, HirKind::Destructure { .. }))
            .expect("the shape has a destructure");
        assert_container_released_after(&hir, &arena, &info, "xs", site, "the destructure");
    }
}

#[test]
fn begin_wrapped_read_borrows_its_container_without_holding_it() {
    assert_borrows_without_holding(BEGIN_REST, "xs", "t");
    assert_borrows_without_holding(BEGIN_FIRST, "xs", "t");
}

#[test]
fn chained_read_borrows_the_first_container_without_holding_it() {
    assert_borrows_without_holding(CHAIN_REST, "xs", "q");
    assert_borrows_without_holding(CHAIN_FIRST, "xs", "q");
}

#[test]
fn a_borrow_out_of_the_outer_pair_leaves_the_nest_merged() {
    // A name bound to a read out of a nested pair is no holder of the outer
    // pair, so the merge seed still finds the outer pair sole-held and folds
    // the inner pair into it.
    //
    // The counter-factual: a borrowed name counted as a holder makes the
    // outer pair held twice, and gate 5 refuses the nest.
    let (hir, _, info) = pipeline(
        "(begin (let [outer (%pair (%pair 1 ()) 2)] \
                  (let [t (begin nil (%first outer))] (%pair? t))) \
                nil)",
    );
    let edges = pair_store_edges(&hir, &info);
    assert_eq!(
        edges.len(),
        1,
        "precondition: the nest has one car/cdr store edge; got {edges:?}"
    );
    let (child, parent) = edges[0];
    assert_eq!(
        info.merged_parent.get(&child),
        Some(&parent),
        "the inner pair r{} merges into the outer pair r{} it is read out of; \
         merged_parent={:?}",
        child.0,
        parent.0,
        info.merged_parent,
    );
}
