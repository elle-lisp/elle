// audited: 2026-09-05
// Guardfree pins for HOF-chain loop fusion: a fused walk must not free an intermediate a later stage still reads.
//
// docs/analysis/testing.md

use super::*;

// Guard — map-chain loop fusion (docs/impl/dissolution.md) inlines `(map f xs)`
// / `(map g (map f xs))` over a proven immutable array into one index-walk loop
// that mints a fresh @array accumulator, fills it, and freezes it, with the base
// array owned by the loop's `coll` binding. Driving that path with HEAP element
// values (strings, structs) must not free a base element under the loop's
// `(get coll i)` read, nor an accumulator member before the frozen result is
// consumed — either over-free SIGSEGVs under guardfree. The composed case also
// pins that the outer result's heap members outlive the dissolved intermediate
// array. Fires only on the fused shape; the plain-VM run asserts the values.
#[test]
fn region_map_fuse_uaf() {
    run_elle_script_with_args(
        "region-map-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Filter loop fusion (docs/impl/dissolution.md) dissolves `(filter p xs)` over a
// proven immutable array into an index-walk loop with a GUARDED push: the element
// is bound once, the predicate tested, and the element pushed into a fresh @array
// only when it passes. Driving that path with HEAP element values (strings,
// structs) must not free a base element under the predicate/push read, nor an
// accumulator member before the frozen result is consumed — either over-free
// SIGSEGVs under guardfree. The filter-of-filter case pins the guarded push of a
// heap value through nested `if`s. Fires only on the fused shape; the plain-VM
// run asserts the values.
#[test]
fn region_filter_fuse_uaf() {
    run_elle_script_with_args(
        "region-filter-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Mixed map/filter loop fusion (docs/impl/dissolution.md § "Mixed chains — one
// loop") collapses `(map f (filter p xs))` / `(filter q (map g xs))` into ONE
// index-walk loop where a `map` stage transforms the threaded element and a
// `filter` stage pushes it under a guard — the intermediate array between the two
// ops never exists. Driving that path with HEAP element values (strings, structs)
// must not free a base element under a transform's or guard's read, nor an
// accumulator member before the frozen result is consumed — either over-free
// SIGSEGVs under guardfree. Fires only on the fused shape; the plain-VM run asserts
// the values.
#[test]
fn region_mixed_fuse_uaf() {
    run_elle_script_with_args(
        "region-mixed-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Fold/reduce loop fusion (docs/impl/dissolution.md § "Fold — the scalar
// terminal") dissolves `(fold f init xs)` over a proven immutable array into an
// index-walk loop with a SCALAR accumulator reassigned one left-fold step per
// element, and fuses a map/filter prefix into the same loop with no intermediate
// array. Driving that path with HEAP values in three roles — heap base elements,
// a heap accumulator the fold rebuilds each step, and heap results threaded out —
// must not free the displaced prior accumulator under the read that builds its
// successor, nor a base element under a combinator/guard/transform read: either
// over-free SIGSEGVs under guardfree. Fires only on the fused shape; the plain-VM
// run asserts the values.
#[test]
fn region_fold_fuse_uaf() {
    run_elle_script_with_args(
        "region-fold-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Count loop fusion (docs/impl/dissolution.md § "Count — the terminal that is a
// guard plus a tally") dissolves `(count pred xs)` over a proven immutable array
// into an index-walk loop whose last stage is the predicate's guard and whose base
// case tallies a scalar. The tally discards the element value, so nothing downstream
// keeps the base's heap elements alive for the guard that reads them — and over a
// map prefix the freshly-minted heap value each element becomes is reachable only
// through the loop's own local, with no intermediate array holding it. Either
// over-free SIGSEGVs under guardfree. Fires only on the fused shape; the plain-VM
// run asserts the values.
#[test]
fn region_count_fuse_uaf() {
    run_elle_script_with_args(
        "region-count-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Search loop fusion (docs/impl/dissolution.md § "Search — the terminal that stops
// early") dissolves `(any? p xs)` / `(all? …)` / `(find …)` / `(find-index …)` over a
// proven immutable array into an index-walk loop whose last stage is the predicate's
// guard and whose base case writes a scalar answer and clears the sentinel the loop
// condition reads. Two roles put heap values on that path: the base's elements, which
// must stay live for the guard that reads them on every iteration up to the decision;
// and `find`'s answer, the only fused accumulator holding a value the loop did not
// allocate — a base element handed out past the loop's own `coll` binding, which must
// not be freed under the result. Either over-free SIGSEGVs under guardfree. Fires only
// on the fused shape; the plain-VM run asserts the values.
#[test]
fn region_search_fuse_uaf() {
    run_elle_script_with_args(
        "region-search-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Take-while loop fusion (docs/impl/dissolution.md § "Take-while — the stage that
// ends the walk") dissolves `(take-while pred xs)` over a proven immutable array
// into an index-walk loop whose guard pushes what it admits and, on the side it
// rejects, clears the sentinel that ends the run. Two roles put heap values on a
// path no other terminal takes. The walk stops SHORT of the base, so it leaves with
// the base's later elements never read while the accumulator holds heap values from
// the ones it did read — the accumulator must own them past the loop and the base's
// unread tail must survive. And the result is that accumulator itself, unfrozen, so
// the caller holds the very object the loop filled rather than a frozen copy.
// Either over-free SIGSEGVs under guardfree. Fires only on the fused shape; the
// plain-VM run asserts the values.
#[test]
fn region_take_while_fuse_uaf() {
    run_elle_script_with_args(
        "region-take-while-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Drop-while loop fusion (docs/impl/dissolution.md § "Drop-while — the stage that
// starts late") dissolves `(drop-while pred xs)` over a proven immutable array into
// an index-walk loop whose guard clears a `dropping` flag at the first element its
// predicate rejects, after which every element is pushed. Two roles put heap values
// on a path no other stage takes. The accumulator fills from the base's TAIL, so the
// leading run the predicate read and discarded must free while the base still owns
// what the accumulator now holds. And the predicate stops at the decision while the
// walk does not, so every later element is read and pushed by a path that never
// binds it to the predicate's parameter. The result is that accumulator itself,
// unfrozen, so the caller holds the very object the loop filled. Either over-free
// SIGSEGVs under guardfree. Fires only on the fused shape; the plain-VM run asserts
// the values.
#[test]
fn region_drop_while_fuse_uaf() {
    run_elle_script_with_args(
        "region-drop-while-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Map-indexed loop fusion (docs/impl/dissolution.md § "Map-indexed — the stage that
// carries the position") dissolves `(map-indexed f xs)` over a proven immutable array
// into an index-walk loop whose element statement binds the walk's induction variable
// to the function's first parameter and the element to its second. Two roles put heap
// values on a path no other stage takes. The stage binds TWO locals per element where
// every other stage binds one, so the element's region must survive the position
// binding that wraps it. And the result is the accumulator itself, unfrozen, so the
// caller holds the very object the loop filled — including where the function hands
// the BASE's own element straight through, which puts a base-owned heap value into an
// accumulator that outlives the loop. Either over-free SIGSEGVs under guardfree.
// Fires only on the fused shape; the plain-VM run asserts the values.
#[test]
fn region_map_indexed_fuse_uaf() {
    run_elle_script_with_args(
        "region-map-indexed-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Mapcat loop fusion (docs/impl/dissolution.md § "Mapcat — the stage that fans out")
// dissolves `(mapcat f xs)` over a proven immutable array into an index-walk loop
// whose element statement binds the collection `f` returns and walks it with a SECOND
// `while`, splicing the rest of the pipeline inside that inner walk. Three roles put
// heap values on a path no other stage takes. The per-element collection is a fresh
// region born and abandoned once per base element while the accumulator keeps values
// read out of it, so those must outlive the collection that carried them. The
// function may hand the BASE's own element through that collection, routing a
// base-owned heap value into an accumulator that outlives the loop. And the result is
// that accumulator itself, unfrozen, so the caller holds the very object the loop
// filled. Any of those over-frees SIGSEGVs under guardfree. Fires only on the fused
// shape; the plain-VM run asserts the values.
#[test]
fn region_mapcat_fuse_uaf() {
    run_elle_script_with_args(
        "region-mapcat-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// GREEN (regression guard) — the HOF manifestation of the native-tail-return
// retain: a pipeline whose tail is `(map …)`/`(filter …)` (the
// dns/parse-resolv-conf shape). The post-`TailCall` `Return` retains the
// heap result (exactly one mint per returned value — either the tail
// fall-through retain or, when ANF names the result, `lower_return`'s), so the
// returned collection survives the caller's release. Passes under guardfree;
// locks the retain against regressions.
#[test]
fn region_hof_tail_return_uaf() {
    run_elle_script_with_args(
        "region-hof-tail-return-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// The fold-shaped e2e witness of the CONST tail-arg borrow (GREEN since the
// `arg_leaf_is_borrowed` const route landed; the minimal shape and mechanism live
// in region_const_tail_move_borrow_uaf / region-const-tail-move-borrow-uaf.lisp).
// A driver thunk `(fn [] (fold-threaded + 0 [1 2 3]))` tail-passes the stdlib
// CONSTANT `+` into an owned-param callee; pure-moving it drained `+`'s region rc
// by one per call to a premature free, and a later `UpdateCapture` deref'd the
// freed page (SIGSEGV under guardfree). Diagnosis history worth keeping: this was
// long framed as a closure-LIFETIME gap of the threaded-arg / cell-held fold shape
// — the framing `src/core.lisp` `fold`'s letrec-capture form was chosen around —
// but the recursion was never the mechanism (a ZERO-iteration callee drains the
// same 1/call); the hole was the thunk's own tail call moving a constant the frame
// never owned. It is state-dependent (faults only once region ids recycle onto the
// freed one), so the fixture discards results and drives ~8000 reps to reach the
// collision deterministically — kept as the deep-churn regression witness beside
// the corpus file's minimal shapes.
#[test]
fn region_fold_closure_arg_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-fold-closure-arg-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}
