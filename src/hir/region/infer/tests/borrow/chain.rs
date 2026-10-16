// audited: 2026-10-15
//! A borrowing read's container outlives each name its result binds, through a wrapper or a second read.
//!
//! docs/impl/region/rules.md
//!
//! Every shape binds the read's result to a name, then reads the name after the
//! container's own last mention. Each shape compiles through the whole pipeline
//! with the graft off, so the wrapper a grafted body leaves is written out here.
//!
//! The counter-factual: a read whose result carries no region makes the bound
//! name hold nothing. The uncounted-read pin then anchors the container at the
//! read's own consumer, which for a bound result is the binder, ahead of the
//! name's later uses. In a begin-wrapped init that consumer is the inner `let`,
//! and in a chain the second read records no container at all.

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

/// Every region `container` holds is released at or after the `reader` call
/// that consumes the borrowed value.
fn assert_container_outlives_reader(source: &str, container: &str, reader: &str) {
    let (hir, arena, info) = pipeline(source);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let reader_site = *find_calls_to_primitive(&hir, reader, &arena)
        .last()
        .unwrap_or_else(|| panic!("the shape has a `{reader}` call"));
    let b = find_binding_by_name(&hir, container, &arena)
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
            ord(dp) >= ord(reader_site),
            "`{container}`'s region r{} is released at @{}, ahead of the `{reader}` \
             call @{} that reads the value borrowed out of it",
            r.0,
            ord(dp),
            ord(reader_site),
        );
    }
}

/// The name bound to a borrowing read's result holds every region its
/// container holds.
fn assert_reader_holds_container(source: &str, container: &str, name: &str) {
    let (hir, arena, info) = pipeline(source);
    let regions_of = |n: &str| {
        let b = find_binding_by_name(&hir, n, &arena).unwrap_or_else(|| panic!("binding `{n}`"));
        info.binding_source_regions
            .get(&b)
            .cloned()
            .unwrap_or_default()
    };
    let held = regions_of(name);
    let container_regions = regions_of(container);
    assert!(
        !container_regions.is_empty(),
        "precondition: `{container}` holds a region"
    );
    for r in &container_regions {
        assert!(
            held.contains(r),
            "`{name}` is bound to a borrow out of `{container}`, so it holds \
             `{container}`'s region r{}; it holds {:?}",
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
fn begin_wrapped_read_binds_its_container_regions() {
    assert_reader_holds_container(BEGIN_REST, "xs", "t");
    assert_reader_holds_container(BEGIN_FIRST, "xs", "t");
}

#[test]
fn chained_read_binds_the_first_container_regions() {
    assert_reader_holds_container(CHAIN_REST, "xs", "q");
    assert_reader_holds_container(CHAIN_FIRST, "xs", "q");
}
