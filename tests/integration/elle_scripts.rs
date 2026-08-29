// Elle scripts that must run under a PROCESS-GLOBAL runtime mode the `elle test`
// harness cannot vary per file.
//
// The corpus under tests/elle/ is owned by the agent-first runner (`elle test`,
// via `make smoke`/`smoke-elle`): it compiles and runs EVERY tests/elle/*.lisp
// once per JIT policy (`:off`→`vm`, `:eager`→`jit`) plus per-tier divergence for
// single-form files — strictly more than a one-off `elle FILE` run. So a plain
// "run this .lisp and assert exit 0" test here is pure duplication; those have
// been removed (see docs/testing.md, docs/test-runner.md).
//
// What the harness CANNOT do is set a process-global mode for one file: the
// page-guard UAF oracle (`--trace=guardfree`), the I/O backend (`--no-uring`),
// or a backend toggle paired with the adaptive JIT (`--jit=adaptive --mlir=off`).
// These live in config.rs as static, once-per-process settings (the runner
// shares one process across every file's worker thread), and a guardfree UAF
// deliberately SIGSEGVs — which would take the single-process harness down with
// it. So the few files that must run under such a mode are pinned below, each as
// its own subprocess `elle <flags> FILE`. (The eventual home is per-file mode
// declarations the runner honors — docs/test-runner.md § future work.)

use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run tests/elle/{name}.lisp with `extra_args` (the process-global backend/trace
/// flags that motivate keeping the script here) and assert it exits with code 0.
///
/// Panics with stdout+stderr if the script exits non-zero or fails to spawn.
fn run_elle_script_with_args(name: &str, extra_args: &[&str]) {
    run_elle_file_with_args(&format!("tests/elle/{}.lisp", name), extra_args);
}

/// Like `run_elle_script_with_args` but takes a path relative to the crate root,
/// for reproducers QUARANTINED outside tests/elle/ (e.g. a script that aborts on
/// plain runs, which would take the shared `make smoke` harness process down).
fn run_elle_file_with_args(script: &str, extra_args: &[&str]) {
    let elle_bin = get_elle_binary();

    let mut cmd = Command::new(elle_bin);
    cmd.args(extra_args).arg(script);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("Failed to spawn elle for {} {:?}: {}", script, extra_args, e));

    assert!(
        output.status.success(),
        "Elle script {} {:?} failed (exit {:?}):\nstdout: {}\nstderr: {}",
        script,
        extra_args,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// =============================================================================
// The guardfree UAF oracle (`--trace=guardfree --jit=adaptive --mlir=off`)
// =============================================================================
//
// `--trace=guardfree` mprotects every freed page PROT_NONE and leaks the
// mapping, so a use-after-free faults (SIGSEGV) at the exact dereference instead
// of silently reading a recycled slot. The harness runs these files under its
// vm/jit policies WITHOUT the oracle, where a regression UAF would read recycled
// memory and show a false green — so the deterministic-fault coverage only exists
// here. `--jit=adaptive` (not the harness's `:eager`) is load-bearing: several of
// these defects only manifest when the adaptive tier JIT-compiles the hot builder
// while pass-through results are still live.

// Region regression: a JIT-compiled function calling a native "pass-through"
// primitive (`first`/`rest`/`get`) must apply the same pass-through retain as
// the interpreter, or the result region is under-counted and freed while a
// freshly built cons still references it (UAF).
#[test]
fn region_jit_passthrough() {
    run_elle_script_with_args(
        "region-jit-passthrough",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// region-array-element-uaf is guardfree-clean (the call-index element survives
// the consuming native's borrow); armed under the UAF oracle to lock the retain,
// mirroring region_jit_passthrough. (The harness already covers its plain and
// interpreter-tier runs via the vm/jit policies.)
#[test]
fn region_array_element_uaf_guardfree() {
    run_elle_script_with_args(
        "region-array-element-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// A fn-local mutable accumulated across a `while` and handed back takes the
// 1-slot-container model, so each value the loop displaces is released at the
// overwrite and the last one leaves with the caller — the `Return`'s mint pays
// for the caller's reference and the cell's content drop, emitted after that
// mint, releases the cell's (docs/impl/region/bindings.md § "Returned fn-local
// reassigned mutables — the return claims the MINT's reference, not the
// cell's"). Armed under the UAF oracle because the model runs a free path the
// unsuppressed baseline never ran: were the content drop to consume the caller's
// reference instead, the returned chain would fault at the caller's read. The
// harness already covers the file's plain vm/jit runs and its bounded-rate face.
#[test]
fn region_loop_acc_return_uaf() {
    run_elle_script_with_args(
        "region-loop-acc-return",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a top-level mutable reassigned to a value referencing its old content
// (`(assign x (pair v x))`) must survive when the file runs as the `%file-body`
// whole-module thunk (the `elle test` shape). The solver must classify the
// file-letrec binding as module-scope (not fn-local via a spurious `in_lambda`),
// so the dead `__file_expr_N` statement wrapper's slot-routed decref does not
// free the just-stored value under the cell: `is_file_scope` routes it to the
// top-level container model. The advanced.lisp `match in loop` shape.
#[test]
fn region_toplevel_reassign_thunk_uaf() {
    run_elle_script_with_args(
        "region-toplevel-reassign-thunk-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `match` pattern binding that aliases into the scrutinee's region
// (`(a & rest)`, `(h . t)`, an immutable-array element, an immutable-struct
// value) must record the scrutinee's `binding_regions`, so the subject region's
// decref_point extends over the bound alias and the subject is not freed under
// the consumer's borrow (which would SIGSEGV under guardfree). The solver's
// `Match` arm propagates the scrutinee's regions to each arm binding, mirroring
// the `Destructure` HIR node. The advanced.lisp guard-with-rest shape.
#[test]
fn region_match_rest_uaf() {
    run_elle_script_with_args(
        "region-match-rest-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — native-tail-return of a heap pass-through result (`(first xs)`/
// `(get xs 0)`/`(xs i)`) must keep the ReturnValue retain, so the caller's
// DecrefValueRegion does not free it under its borrow (which would SIGSEGV
// under guardfree). The native-tail post-block retains a heap result before
// Return.
#[test]
fn region_native_tail_return_uaf() {
    run_elle_script_with_args(
        "region-native-tail-return-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a cons cell (`%pair`) storing a HEAP value keeps that value's region
// alive for the cons's lifetime via the runtime alloc-scan/free-cascade contract
// (`handle_list` → `incref_cross_region`; `find_object_cross_refs` Pair arm), NOT
// a compile-time containment edge (which double-counts against the single cascade
// decref — the same RC double-count captures avoid by recording no edge). This
// reads a deep chain of escaping conses' heap contents back after region-id churn:
// an under-incref of any element frees it under the reader (SIGSEGV under
// guardfree). Pairs with the oracle's `arg-result`/`cons-store` leak pins (the
// over-keep face of the same edge). docs/impl/region/ownership.md § "The outgoing
// edge table"; walk/intrinsic.rs (the %pair contents).
#[test]
fn region_pair_heap_content_uaf() {
    run_elle_script_with_args(
        "region-pair-heap-content-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

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

// Fusing a CAPTURING lambda (docs/impl/dissolution.md § "Captures") splices a body
// that reads an enclosing binding directly, so the loop holds a heap value the
// ENCLOSING frame owns rather than one reached through a closure environment. Three
// roles follow from that. The capture is read once per element across the whole
// walk, so the frame must still own it at the last one; the body may hand it INTO
// the accumulator, so a frame-owned value ends up in a structure that outlives the
// loop; and a captured mutable binding is written per element through its cell,
// whose displaced prior must free without taking the live one. Any of those
// over-frees SIGSEGVs under guardfree. Fires only on the fused shape; the plain-VM
// run asserts the values.
#[test]
fn region_capture_fuse_uaf() {
    run_elle_script_with_args(
        "region-capture-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — `break` TRANSFERS its value to the enclosing block
// (docs/impl/region/mechanism.md § "`break` transfers its value; it does not
// consume it"). The transfer moves the broken value's release out of the block
// body — which the break's jump to the exit label skips — and onto the `Block`
// node, emitted after that label. That placement is correct only while the
// block's result regions reach the binding naming it, so the binding-chain
// `decref_point` extension carries the release past every later read. Without
// that flow the release fires at the exit label and each read below touches
// freed pages — SIGSEGV under guardfree. Drives the broken value's heap contents
// through a post-block read for every placement (bare, `let`-bound, stored,
// branched, out of a `while`, out of a nested block, forwarded into a call) with
// a fresh subject per iteration so region ids recycle under the reader. The leak
// face is `region-break-transfer.lisp`.
#[test]
fn region_break_transfer_uaf() {
    run_elle_script_with_args(
        "region-break-transfer-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the same jump that strands the broken value strands every OTHER
// release between the break site and the exit label, and those are re-anchored
// to the block too (docs/impl/region/mechanism.md § "A release the break jumps
// over is not a release"). Moving a release later can only over-keep — while it
// still names the same value when it runs, which is what this drives: a window
// value read after the block, stored into a container, returned, captured by a
// closure, and reached across the two scopes the window stops at (a nested loop,
// whose body re-allocates per iteration, and a nested lambda, whose releases
// belong to another frame). A release hoisted out of either frees a live region
// and every read below touches freed pages — SIGSEGV under guardfree. The leak
// face is `region-break-skip.lisp`.
#[test]
fn region_break_skip_uaf() {
    run_elle_script_with_args(
        "region-break-skip-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a region live-in to a branch has ONE release, and it is anchored where
// every arm reaches it rather than inside the arm that happens to name it last
// (docs/impl/region/mechanism.md § "A release inside one arm is not a release on
// the other arms"). The release moves later, which can only over-keep — while it
// still drops the frame's own reference and no other, which is what this drives:
// an arm that stores the value into a container, hands it to a closure, returns
// it to its caller, and parks a fiber that resolves it through its own
// activation map after the branch; plus the three scopes the window stops at (a
// nested loop, whose body re-allocates per iteration, a nested lambda, whose
// releases belong to another frame, and a frame-replacing tail call, which never
// reaches the merge). Freeing a live region there faults on the read below —
// SIGSEGV under guardfree. The leak face is `region-branch-arm-window.lisp`.
#[test]
fn region_branch_arm_window_uaf() {
    run_elle_script_with_args(
        "region-branch-arm-window-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a fiber crossing leaves a COUNTED holder, so the frame-held admission
// rides it instead of refusing (docs/impl/region/mechanism.md § "A fiber crossing
// is a counted holder too"). Going out that reference is the park's `EmitEscape`
// retain; coming back it is the resume value's own mint, which nothing took before
// (docs/impl/region/owner.md § "A resume value crosses counted, or not at all").
// So this drives what must outlive the OTHER side's release: a body that keeps its
// resume value past a further park, two bodies keeping the same delivered value, a
// resumer reading what it was yielded after the emitting body ran on, and a value
// delivered from inside a branch arm whose release the window now anchors at the
// merge — plus the containment store the window must still refuse. Freeing any of
// them faults on the read below — SIGSEGV under guardfree. The leak face is
// `region-fiber-frontier-window.lisp`.
#[test]
fn region_fiber_frontier_window_uaf() {
    run_elle_script_with_args(
        "region-fiber-frontier-window-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the sequence reads and conversions declare `Opaque`, which says two
// things: the result may live anywhere, and no argument is stored uncounted
// (docs/impl/region/effects.md § `Opaque`). The second withdraws a store-facet
// escape seed, and with it the refusal that seed forced on every mechanism gated
// on `frame_held_regions`. So this drives what the refusal used to mask: a
// read's result consumed after the branch that produced it, the subject read
// again, the result returned to a caller or yielded to a resumer, and a genuine
// store escape that must still refuse the window. Freeing the container under any
// of those faults — SIGSEGV under guardfree. The leak face is
// `region-sequence-read-effect.lisp`.
#[test]
fn region_sequence_read_effect_uaf() {
    run_elle_script_with_args(
        "region-sequence-read-effect-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — `fiber/child` and `import` declare `Opaque`: each stores no argument, so
// neither seeds escape's store facet (docs/impl/region/effects.md § `Opaque`).
// Withdrawing that seed withdraws the refusal it forced on every mechanism gated
// on `frame_held_regions`, the branch-arm release window among them, so the
// argument's release moves from the arm that names it last to the merge every path
// reaches. This drives what must still outlive that merge: a fiber read again
// after the branch, resumed after it, stored into a container a sibling arm reads,
// returned to a caller that resumes it, captured by a closure called later, and
// held across the fiber frontier by an inner fiber — plus an import specifier read
// and stored the same way. Freeing any of them at the merge faults on the read
// below — SIGSEGV under guardfree. The leak face is
// `region-fiber-child-effect.lisp`.
#[test]
fn region_fiber_child_effect_uaf() {
    run_elle_script_with_args(
        "region-fiber-child-effect-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a fiber body owns one reference of every value it yields
// (docs/impl/region/owner.md § "Park/unpark symmetry"). A park's `EmitEscape`
// retain is the DELIVERY reference the resumer's result release consumes, so what
// a discarded fiber's discharge stands in for is the body's separate reference,
// released by the continuation past the yield. A body-allocated payload carries
// that reference itself; a BORROWED one — a capture, a parameter, a module-level
// binding — carries none unless the lowerer mints it, and the discharge then
// releases the delivery reference twice over, freeing the value under every holder
// that outlives the fiber. This drives each borrow shape past an abandoned
// suspended fiber and reads it afterwards — through the resume result, through
// `fiber/value`, through a container, and through the yielding frame's own binding
// — so an over-free faults under guardfree, with the four controls that must stay
// clean without a mint and a growth gauge that refuses a mint-everywhere fix.
#[test]
fn region_fiber_yield_borrow_uaf() {
    run_elle_script_with_args(
        "region-fiber-yield-borrow-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — what yields is the emit OPERATION, not the `Emit` node
// (docs/impl/region/owner.md § "What yields is the emit OPERATION, not the `Emit`
// node"). A first argument the compiler cannot read as a keyword set falls through
// to the `emit` primitive, so the park is an ordinary call and the body reference
// the discard discharge stands in for has to come from the call: a NON-TAIL one
// mints it at the payload argument, a TAIL one already holds the borrowed-argument
// retain and the suspending exit leaves it standing. Withhold either and the
// discharge releases the delivery reference the resumer already consumed, freeing
// the payload under every holder that outlives the fiber. This drives each borrow
// shape — a module-level binding in both positions, a captured local, a captured
// parameter, a second park of the same value — past an abandoned fiber and reads it
// afterwards, through the holder, through `fiber/value`, and through a container, so
// an over-free faults under guardfree. Four controls must stay clean without an extra
// reference, and a growth gauge refuses a mint-everywhere fix.
#[test]
fn region_dynamic_emit_borrow_uaf() {
    run_elle_script_with_args(
        "region-dynamic-emit-borrow-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a raised payload's delivery reference, where the raise leaves the emit
// PRIMITIVE in tail position (docs/impl/region/mechanism.md § "What the fall-through
// owes, a signal exit owes too"). The exit consumes the call's borrowed-argument
// retains, the block that would have consumed them being abandoned, so it mints the
// payload's delivery and records it — the same pair `handle_emit` performs on the
// literal path. Withhold the mint and the catcher's read of the delivered payload
// frees it under every holder that outlives the fiber; withhold the record and the
// frame's own reference to a payload it allocated is stranded. This drives every
// holder shape past a raise — a module-level binding, a captured local, a captured
// parameter, a `fiber/value` read, a container, an uncaught propagation, and a
// restarted fiber that replays the abandoned block — and reads each afterwards, so
// an over-free faults under guardfree. Six controls must stay clean with no mint,
// and a growth gauge refuses a mint-per-reference fix.
#[test]
fn region_dynamic_emit_terminal_uaf() {
    run_elle_script_with_args(
        "region-dynamic-emit-terminal-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a resume value delivered into a frame parked at a suspending PRIMITIVE
// call carries one owning reference (docs/impl/region/owner.md § "A delivery into
// a replayed frame carries one owning reference"). The replayed frame re-enters at
// that call's continuation and runs the call's compiler-emitted result release; a
// bytecode callee funds that release with its `Return` mint, but a primitive that
// suspends never returns, so the delivery owes it. This drives both parks that
// land in such a continuation — a dynamic `emit`, in bound and tail position and
// held across a further park, and a mediated capability denial — each paired with
// the literal `Emit`, whose resume block mints the reference in bytecode and is
// therefore correct without the delivery's. Freeing the delivered value early
// faults on the read below — SIGSEGV under guardfree. The leak face is the
// `primitive-resume-*` closed-control family in `tests/elle/oracle.lisp`, which
// refuses a delivery that mints more than one.
#[test]
fn region_primitive_resume_uaf() {
    run_elle_script_with_args(
        "region-primitive-resume-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the resume of a mediated capability denial releases the one reference
// the park has no body to release, and that decref answers for the payload's own
// left-over reference, never for a holder's (docs/impl/region/owner.md § "A park
// with no body reference owes one release at the resume"). The witness binds the
// payload in the mediating parent, resumes the fiber past the denial, churns the
// heap, then reads three payload fields; taking the holder's reference instead
// frees the struct under those reads — SIGSEGV under guardfree. The leak face is
// the region-count bound in the same file, which the object gauge in
// `tests/elle/oracle.lisp` cannot see.
#[test]
fn region_capability_denial_resume_uaf() {
    run_elle_script_with_args(
        "region-capability-denial-resume-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — `fiber/propagate` installs the child's parked payload as this fiber's
// own `signal`, which is a fresh park and owes its own delivery reference
// (docs/impl/region/owner.md § "Park/unpark symmetry"). The propagating fiber's
// resumer reads that payload as its resume result and runs the compiler-emitted
// release on it; the child's park funded its own resumer's release, not this one.
// One propagate hides the shortfall — an error unwind runs no continuation, so
// the raising body's stranded reference is what the release eats instead — so the
// witnesses remove that cover, by propagating twice or three times and by raising
// from a native, whose payload reaches `fiber.signal` owning nothing. Each reads a
// HEAP field of the payload after the carrying fibers are gone; a bare status
// check passes over a freed payload. Freeing it early faults on that read —
// SIGSEGV under guardfree. The leak face is the `propagate-*` closed-control
// family in `tests/elle/oracle.lisp`.
#[test]
fn region_fiber_propagate_uaf() {
    run_elle_script_with_args(
        "region-fiber-propagate-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — an inlined callee's body regions name the CALLEE's activation, so the
// caller names the call's own region for the result (docs/impl/region/mechanism.md
// § "A call's result is named by the call's own region"). The result therefore
// carries exactly one caller-side release, so what the callee hands back that is
// not freshly its own has no second caller-side holding and must ride a counted
// edge instead. This drives each such
// hand-off: an argument returned unchanged, one of two arguments picked per path,
// an element read out of an argument, a result stored into a module-level
// container, captured by a closure called later, yielded across the fiber
// frontier, fed forward as the next call's argument, read past a branch merge, and
// allocated in a self-recursive walk's base case. Freeing any of them early faults
// on the read below — SIGSEGV under guardfree. The leak face is
// `region-inline-result-naming.lisp`.
#[test]
fn region_inline_result_naming_uaf() {
    run_elle_script_with_args(
        "region-inline-result-naming-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `match` arm's pattern binding records its scope, so a read of it
// inside a loop no longer reads as a read of a loop-external binding and the
// scrutinee's release stays in the body that allocates it (docs/impl/region/
// mechanism.md § "Every binder records its scope"). The release moves EARLIER —
// from after the loop to once per iteration — so what it must not do is drop a
// projection someone else still holds. This drives every hand-off out of the
// iteration: the arm stores the projection into a fn-local cell, into a
// module-level container, captures it in a closure called after the loop, breaks
// out of the loop with it, yields it across the fiber frontier, reads an inner
// loop's projection from the outer body, reads into a nested container
// projection, and feeds it back into the next iteration's scrutinee. Freeing any
// of them at the iteration's end faults on the read — SIGSEGV under guardfree.
// The leak face is `region-match-bind-loop.lisp`.
#[test]
fn region_match_bind_loop_uaf() {
    run_elle_script_with_args(
        "region-match-bind-loop-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — everything the lowerer emits after a `TailCall` runs only on the NATIVE
// fall-through, so a release landing there is carried back ahead of the call
// (docs/impl/region/mechanism.md § "A release past a frame-replacing tail call is
// not a release"). This is the one release the region system moves EARLIER, and
// its legality is entirely the exemption: only a region the call itself cannot
// reach may move. This drives what the call CAN reach — an argument moved into
// the callee, a moved argument beside a hoisted sibling, the per-call callee
// closure the new activation takes over, a value the callee reads through its
// captured environment, a mutable accumulator the callee fills, a value already
// stored into a longer-lived container, a value returned through the callee, and
// an argument a parked frame resolves after the resume. Releasing any of them
// early faults on the read below — SIGSEGV under guardfree. The leak face is
// `region-tail-frame-exit.lisp`.
#[test]
fn region_tail_frame_exit_uaf() {
    run_elle_script_with_args(
        "region-tail-frame-exit-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a native tail call that leaves by a SIGNAL consumes the borrowed
// argument retains the abandoned post-`TailCall` block would have consumed
// (docs/impl/region/mechanism.md § "What the fall-through owes, a signal exit
// owes too"). Each is a release on a path that ran none before, so three faces
// must survive it: the value the signal PAYLOAD carries (a fiber carrier hands
// over its own fiber argument), the REPLAY of a parked or restarted frame that
// reaches the same release a second time, and an OUTER holder the caught error
// returns to. Every read below happens after the exit ran, so an over-release
// faults there — SIGSEGV under guardfree. The leak face is
// `region-tail-signal-exit.lisp`.
#[test]
fn region_tail_signal_exit_uaf() {
    run_elle_script_with_args(
        "region-tail-signal-exit-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a frame abandoned by an ERROR runs the releases it still owed, off
// the value-route slots the emitter recorded (docs/impl/region/mechanism.md
// § "An abandoned frame runs the releases it still owes"). Each is a release
// the frame genuinely had, run earlier than it would have been, so what must
// survive is everything that outlives the frame: the signal PAYLOAD the catcher
// receives, a value the frame STORED into a longer-lived container, a parked
// frame the RESTARTS system can replay, and the CATCHING frame's own values.
// Every read below happens after the unwind ran, so an over-release faults
// there — SIGSEGV under guardfree. The leak face is
// `region-error-unwind.lisp`.
#[test]
fn region_error_unwind_uaf() {
    run_elle_script_with_args(
        "region-error-unwind-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — an emit-raised error's payload keeps every frame-owed release: the
// raise minted the delivery reference itself, so the walk and the parked
// frame's discharge stop exempting the payload's region
// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
// still owes"). What must survive the withdrawn exemption is every reference
// the walk does not own: the delivery the catcher reads, a counted store's, a
// borrowed payload's owner, a native raise's unrecorded install, and a
// restarted frame's replay. Each faults under guardfree if the walk releases
// one it never had. The leak face is the `error-payload*` closed-control
// family in `tests/elle/oracle.lisp`.
#[test]
fn region_error_payload_uaf() {
    run_elle_script_with_args(
        "region-error-payload-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the COMPILED face of the same walk: a compiled frame's error exit
// reads its value route off the locals it spilled there and its slot route off
// the activation map its prologue pushed, then pops that map
// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
// still owes"). What must survive is every reference the walk does not own: the
// delivery the catcher reads, a counted store's, a borrowed payload's owner,
// and the CALLER's binding live across the compiled callee's exit — the one the
// map pop answers for, since a leftover callee map would resolve the caller's
// releases against the wrong frame. Eager JIT, so the raisers are compiled
// before the reads. The leak face is `region-jit-error-unwind.lisp`.
#[test]
fn region_jit_error_unwind_uaf() {
    run_elle_script_with_args(
        "region-jit-error-unwind-uaf",
        &["--jit=eager", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `def` evaluates to what it bound, so its initializer's demise must
// not be narrowed onto the initializer when nothing reads the binding
// (docs/impl/region/mechanism.md § "A binder's init release lands after the slot
// store"). Every other binder's value is its BODY, so an unread init really is
// dead at the init; a `def`'s value IS the init and flows straight on. This
// drives every way it leaves — handed to a callee, returned, bound to a second
// name, propagated through a `begin`, produced by a branch arm, stored into a
// container that outlives the frame, captured by a closure, and resolved by a
// parked frame after a yield. Freeing any of them at the initializer faults on
// the read — SIGSEGV under guardfree. The leak face is
// `region-define-init-release.lisp`.
#[test]
fn region_define_init_release_uaf() {
    run_elle_script_with_args(
        "region-define-init-release-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a whole-value read of a REASSIGNED CAPTURED CELL (fn-local upvalue
// read AND module-scope `def @cell`) must take a counted reference, or the
// cell's next overwrite (`capture_store_with_rebind` decrefs the displaced prior
// unconditionally) frees the value under the reader — the captured-alias UAF
// (SIGSEGV under guardfree). The reader takes Rule 5's "new reference"
// pass-through (an `IncrefValueRegion` at the read, balanced by the
// `DecrefValueRegion` at its last use). This is the std/process scheduler's
// `ready` double-buffer (`sched-run`'s `(let [batch ready] (assign ready @[])
// (each pid in batch (run-one pid)))`), whose regression SIGSEGVs
// tests/elle/process-io.lisp. docs/impl/region/bindings.md § "Captured
// reassigned cells".
#[test]
fn region_reassign_captured_cell_reader() {
    run_elle_script_with_args(
        "region-reassign-captured-cell-reader",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a self-recursive closure that is ALSO captured by a sibling (so it is
// cell-held) must NOT have its region released by a tail-call deferred release: the
// capturing cell owns that release, and its lifetime outlives the tail-call
// activation. Marking such a binding `stranded_self` frees its region under the
// live cell (a generation panic / SIGSEGV under guardfree at the next
// `tail_callee_release_region` deref). This is the scheduler's mutually recursive
// `handle-fiber-after-resume` group, whose regression SIGSEGVs
// tests/elle/process-io.lisp. Only CELL-FREE self-recursion is stranded
// (docs/impl/selfrec.md).
#[test]
fn region_selfrec_captured_tail_release() {
    run_elle_script_with_args(
        "region-selfrec-captured-tail-release",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a stranded recursive closure the recursion RETURNS keeps its tail-call
// deferred release, and that release must drop only the FRAME's reference. The
// caller's is minted by the callee's `Return`, which runs before `trampoline_loop`
// breaks and fires the deferred decref (docs/impl/selfrec.md § the deferral's escape
// gate). If the count is wrong, the returned handle's region is recycled and the
// self-call re-dispatch — which reads the executing closure out of that very region —
// derefs a foreign page. Covers all three stranding routes (`letrec` self, `def` self,
// merged mutual SCC) with allocation churn between the release and the re-entry.
#[test]
fn region_selfrec_return_release() {
    run_elle_script_with_args(
        "region-selfrec-return-release",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a stranded recursive closure handed across the FIBER frontier keeps its
// tail-call deferred release too, and that release must drop only the FRAME's
// reference. The crossing counts its own: the emit's park retain into `fiber.signal`,
// which the resumer's result release consumes, and `chan/send`'s send-site incref,
// held until a receive builds the result carrying the message (docs/impl/selfrec.md §
// "The deferral needs no escape gate"). If the count is wrong, the delivered handle's
// region is recycled and the self-call re-dispatch — which reads the executing closure
// out of that very region — derefs a foreign page. Drives both fiber-frontier seeds —
// the emit through both binder routes, the send through the `letrec` one — with
// allocation churn between the release and the re-entry.
#[test]
fn region_selfrec_fiber_release() {
    run_elle_script_with_args(
        "region-selfrec-fiber-release",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — `chan/send`'s message reference is counted at the send seam itself
// (`EscapeSite::ChanSend` in `prim_chan_send`): the channel buffer is external to
// the region system, so the seam's runtime retain is what holds the message until
// `release_received_message` lowers it at the receive. A compile-time `Sends` edge
// cannot carry that reference — it is keyed on a region pair, and at a real call
// site the channel is a module-level binding read as an upvalue, so no pair exists
// and no incref is emitted; the sending function's owned-parameter release then
// drains the message's region to zero while it still sits in the buffer, and the
// receive reads a freed region (SIGSEGV under guardfree). Drives the owned-param
// message through a top-level caller loop, an `ev/spawn`'d sender, and a
// tail-position `chan/recv`, plus the bounded-growth leak face of the same seam.
#[test]
fn region_chan_send_owned_param_uaf() {
    run_elle_script_with_args(
        "region-chan-send-owned-param-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a local mutual-recursion clique (`ev`/`od`) whose `letrec` body ends in a
// tail call to a NON-member (a native `%add`, the redefined-closure operator `+`, a
// foreign fn `g`, and a MIXED member+non-member `if`) must reclaim its merged arena
// soundly. The frame-replacing `TailCall` strands the arena's binding-scope drop, so
// a closure callee rides the explicit arena adopt (`TailCall::deferred_release_slot`) at
// recursion completion while a native callee falls through to the live scope-exit
// drop — mutually exclusive per call, exactly one release. A premature free leaves
// `ev`/`od` (whose regions ARE the merged arena) dereferencing recycled pages on the
// next recursion step (SIGSEGV under guardfree). Also drives the clique PER LOOP
// ITERATION, the per-call reclamation granularity an activation-owner-node cut would
// double-free. docs/impl/region/letrec.md § The letrec closure-cycle merge.
#[test]
fn region_native_tail_mutual_cycle_uaf() {
    run_elle_script_with_args(
        "region-native-tail-mutual-cycle-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a local mutual-recursion clique (`ev`/`od`) one of whose members is RETURNED
// must keep that handle live and re-enterable after its merged arena's release runs. The
// merge admits the return facet because the returned member lives IN the arena, so the
// callee's `Return` mint raises the arena's own count, and the member-callee tail
// deferral runs at the recursion's normal completion — after that mint — dropping only
// the frame's own reference. Were the ordering the other way (or the deferral the last
// reference), the caller would hold a closure whose env sits in a freed arena: a
// generation panic on the plain VM, a SIGSEGV under `--trace=guardfree`. Every returned
// handle is re-entered after the release across allocation churn that recycles a freed
// page, including handles held live across many later mint/free cycles, plus the refused
// residual (a non-member body tail) which must still run correctly.
// docs/impl/region/letrec.md § The frontier gate.
#[test]
fn region_letrec_return_cycle_uaf() {
    run_elle_script_with_args(
        "region-letrec-return-cycle-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a fresh `%pair` pushed into a fresh, let-bound `@[]` whose push result is
// DISCARDED, in a loop, must reclaim its Owned subtree without a double-free. The pair
// is a store-adopted member whose own slot-resolved `DecrefRegion` is a no-op only
// while it is still `Owned`, so it must be emitted before the container's subtree drop.
// At the let-body the pair and the container share a `decref_point`, and the container
// is freed by TWO releases there (its binding release and the discarded pass-through
// result of `%array-push`, which returns its container); order the pair's plain
// `DecrefRegion` after those and the drop reclaims the pair before its own decref — a
// phantom/double-free (SIGSEGV under guardfree). The topological release order over the
// adopt edge (`with_region_info::order_releases`, member → owner) keeps the member's
// release ahead of the container's.
// docs/impl/region/adopt.md § "The lifetime obligation the root carries".
#[test]
fn region_array_push_pair_loop_uaf() {
    run_elle_script_with_args(
        "region-array-push-pair-loop-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Correctness guard — the splice/`apply` manifestation of the native-tail-return
// retain. `(first ;argv)` lowers to `TailCallArrayMut`, whose post-block emits
// the ReturnValue retain (lower_call splice arm). Known limitation: the splice
// UAF is currently masked by the args-array leak, so this asserts the result
// value rather than faulting; it becomes a hard UAF guard once that leak is
// fixed. Run under guardfree to lock the retain alongside the non-splice guard
// above.
#[test]
fn region_splice_tail_return() {
    run_elle_script_with_args(
        "region-splice-tail-return",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// GREEN (live guard) — a closure that tail-passes a TOP-LEVEL binding as an
// owned-param argument must not over-free that binding's region
// (phantom/double-free; SIGSEGV under guardfree). The hazard the witness
// describes does NOT reproduce on HEAD: `tail_arg_is_borrowed`
// (src/lir/lower/control.rs) still flags ONLY captured upvalues, so a top-level
// reference is indeed pure-moved into the owned-param callee — yet the 500-iter
// loop is guardfree-clean. The over-free the witness hypothesized is balanced
// elsewhere: the top-level escape is increfed through the Rule 5 EscapeSite
// funnel, so the callee's owned-param release leaves the region's RC intact.
// Kept as a guard: if a regression drops that escape incref, the binding's
// region drains to zero mid-loop and the final read faults here. Pinned as a
// subprocess because the guardfree witness would be an uncatchable SIGSEGV.
#[test]
fn region_tail_move_toplevel_uaf() {
    run_elle_script_with_args(
        "region-tail-move-toplevel-uaf",
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

// A reassigned mutable binding fed by a CALL RESULT must hold a COUNTED
// reference to it (docs/impl/region/bindings.md): the call result's own
// placeholder release fires regardless, so the 1-slot container cannot also
// donate — and the counted store must be emitted before `StoreLocal` consumes
// the value register, or the retain lands on the displaced prior instead.
#[test]
fn region_reassign_callresult_store() {
    run_elle_script_with_args(
        "region-reassign-callresult-store",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// A top-level mutable that is BOTH captured by a closure (boxed in a
// MakeCaptureCell) AND reassigned. The hazard: routing the init value's release
// through the binding slot — which holds the CELL — makes `DecrefValueRegion`
// reload the slot and (via `result_region_of`, which unwraps a capture cell)
// free whatever the cell holds at the decref's runtime firing point. Once a
// reassignment has repointed the cell, that would free a different, live value
// (UAF) and double-release the displaced original.
//
// The lowerer avoids this by routing such a binding's init via
// `Lowerer::store_captured_cell_init`: it drops the init's alloc reference off
// the value register directly (timing-independent) and SKIPS the cell-slot
// routing (`region_info.captured_reassigned_bindings`). A captured binding that
// is never reassigned keeps the ordinary routing — its cell content is stable,
// so the unwrap always names the right value.
//
// Quarantined under tests/integration/fixtures/ (not tests/elle/) and pinned
// with `--trace=guardfree` so that a regression faults deterministically (rather
// than landing on a recycled page) in its own subprocess, instead of aborting
// the shared `make smoke` process.
#[test]
fn region_capture_cell_reassign_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-reassign-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// The same hazard with the `assign` moved INSIDE a closure the defining scope
// encloses. The binding still owns a compiled `MakeCaptureCell`, so the routing
// question is unchanged — but classifying the reassign by the ASSIGN SITE's scope
// calls it fn-local, keeps the cell-slot routing, and frees the value the frame
// hands back. The classification is a fact about the binding, not the write site
// (docs/impl/region/bindings.md § "Captured reassigned cells"). Compile-level
// twins over more shapes: `lir::lower::tests::release`'s
// `*_closure_reassign_leaves_no_cell_slot_release`.
#[test]
fn region_capture_cell_closure_reassign_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-closure-reassign-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// GREEN (live guard) — the BENIGN, non-reassigned sibling of the hazard above: a
// top-level mutable captured by one or more closures (boxed in a MakeCaptureCell)
// but never reassigned. The RC accounting balances; the hazard here is
// COMPILE-ORDER nondeterminism, which the lowerer pins down two ways:
//
//  1. `compute_last_use`'s binding-chain override must resolve each chain
//     fully, not a hash-ordered PREFIX. acc's cell reaches the `(u1)` call
//     site only through u1's override (the capture-use registers at the Lambda
//     node, which IS u1's init id); resolving acc first would land its cell's
//     decref_point before the closure calls — freed while still callable. The
//     override is iterated to its unique, order-independent fixpoint in
//     src/hir/liveness/lastuse.rs.
//  2. At the shared decref_point, the cell's page-FREEING `DecrefRegion` must
//     order after the init's page-READING `DecrefValueRegion` (which unwraps
//     the cell); a freeing-first permutation would tear the page the unwrap
//     reads. The topological release order in `Lowerer::with_region_info`
//     (`order_releases`) tie-breaks page-reads before page-frees, fixing the
//     order (docs/impl/region/rules.md Rule 4).
//
// A regression would fault only timing-dependently (~⅓ of runs under
// guardfree), so the guard loops: 25 runs at p≈0.37 witness a regression with
// probability > 0.9999, while the correct release path passes every run.
// Compile-level twins of this guard live in src/lir/lower/tests.rs
// (`release_order_value_gated_before_plain_in_shared_bucket`,
// `region_analysis_is_deterministic_across_compiles`).
#[test]
fn region_capture_cell_noreassign_uaf() {
    for _ in 0..25 {
        run_elle_file_with_args(
            "tests/integration/fixtures/region-capture-cell-noreassign-uaf.lisp",
            &["--trace=guardfree"],
        );
    }
}

// Guard — a `@`-mutable captured local materialized as a `populate_env` env cell
// (minted once per activation) and captured by a closure built in a loop must
// survive every iteration. The closure captures the CELL by indirection — a BORROW
// through a separately-owned env cell whose release is hoisted to once-per-activation
// — so the ownership forest must NOT fold the cell's contents into the closure's
// per-iteration Owned subtree. If it did, the closure's subtree drop would free the
// cell (and its still-referenced contents) at the end of iteration 1, and the next
// iteration's re-store of the cell derefs the freed page (`capture_store_with_rebind`
// reads the stale prior content). `capture_containment_edges` excludes cell-indirected
// captures for exactly this reason (the cell owns its contents, the closure only reads
// through it). The corpus runner exercises the (now unconditional) forest but never
// under `--trace=guardfree`, so this subprocess is the deterministic-fault guard for
// the env-cell-vs-capture-adopt interaction. Canonical shape:
// tests/elle/region-capture-cell-loop-uaf.lisp (single loop, nested loops, and
// per-iteration content variance).
#[test]
fn region_capture_cell_loop_uaf_ownership() {
    run_elle_script_with_args("region-capture-cell-loop-uaf", &["--trace=guardfree"]);
}

// GREEN (live guard) — `with-traits` attaches a trait-table struct to a value;
// the table lives in its OWN region (not inline), so it is a cross-region edge
// like any content field. `find_object_cross_refs` must enumerate the `traits`
// side-field, not just the inline content fields: otherwise the table keeps
// RC 1 and its constructor's DecrefValueRegion DIRECT-frees it while the host
// still references it — `(get (traits x) :tag)` then binary-searches the freed
// struct → SIGSEGV in TableKey::cmp (types.rs). `find_object_cross_refs`
// enumerates `obj.traits()` for all variants, so the alloc-scan increfs and the
// free-cascade decrefs symmetrically (Rule 5/7). Quarantined as a subprocess
// because a regression is an uncatchable SIGSEGV that would crash the shared
// smoke harness; armed under guardfree so the fault is deterministic if it
// returns.
#[test]
fn region_traits_table_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-traits-table-uaf.lisp",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// GREEN (live guard) — distinct from `region_traits_table_uaf` above, which was a
// RUNTIME RC gap (fixed). This is the COMPILE-TIME OWNERSHIP invariant the unconditional
// forest upholds: a closure captures a top-level struct and attaches it as a trait table
// with `with-traits`. `with-traits` declares `RegionEffect::Fresh` AND `embeds: &[1]`, so
// the walk records the `result ⊇ table` embed containment (`call_embeds` →
// `containment_edges`) — the compile-time analog of the runtime alloc-scan that counts the
// same embedding. With it the forest sees the captured table flow OUT through the escaping
// traited value and keeps it Shared instead of capture-adopting it. Without it the closure's
// subtree drop frees the table while the escaped value's `traits` field still references it
// — a wrong answer (`nil`) on plain runs and a SIGSEGV (context `UpdateCapture`) under
// guardfree. Quarantined as a subprocess because a regression is an uncatchable SIGSEGV that
// would crash the shared smoke harness; armed under guardfree so the fault is deterministic
// if it returns. Full repro + invariant in the fixture.
#[test]
fn region_traits_capture_adopt_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-traits-capture-adopt-uaf.lisp",
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

// The STRING sibling of region_fold_closure_arg_uaf: a helper accumulates a
// string by reassigning a `@`-capture cell in a loop (`(assign out (string out
// …))`) and RETURNS `out`; the caller reads the returned value one form later.
// Green pins that the loop-reassigned capture cell's returned region stays live
// across the caller's read (the mu `_safe-uri` / `_slug` builder shape). Runs as
// a guardfree SUBPROCESS via `run_elle_file_with_args`, so an over-free's
// SIGSEGV fails THIS test cleanly instead of taking a shared harness process
// down (that hazard is the tests/elle/*.lisp glob's shared process, not this
// one). Full shape in the fixture header.
#[test]
fn region_capture_cell_string_accum_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-string-accum-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Two sequential loops over ONE reassigned mutable chain the versions
// functionalization gives the name (`last#2 <- last#1 <- last#0`), so a middle
// version carries a 1-slot cell whose content is the reference the chain
// forwards on (docs/impl/region/bindings.md § "A chain of forwarding edges hands
// one reference along, so the fold follows it whole"). Green pins that exactly
// one link releases that reference: the cell holding it when its own slot is
// overwritten, or the last link at its scope demise. Subprocess guardfree run,
// same rationale as the twin above; the leak and read-back faces are in
// tests/elle/region-cell-forward-chain.lisp. Full shape in the fixture header.
#[test]
fn region_cell_forward_chain_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-cell-forward-chain-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// A fn-local 1-slot container whose INIT value carries a second name takes that
// value by a COUNTED store rather than by donation, so the alias keeps the
// producer's reference and the decref that releases it
// (docs/impl/region/bindings.md § "What the cell donates it must hold alone;
// what it counts it need not"). Green pins that every later read through the
// alias — after the cell has displaced the init, or after a cursor has walked
// off the chain head the alias names — still reads a live page. Subprocess
// guardfree run, same rationale as the twin above; the leak and read-back faces
// are in tests/elle/region-cell-aliased-init.lisp. Full shape in the fixture
// header.
#[test]
fn region_cell_aliased_init_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-cell-aliased-init-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// The CASCADE / stored-member twin of region_capture_cell_string_accum_uaf, and
// the e2e witness of the drop-time external-reference rescue
// (docs/impl/region/ownership.md § "The incoming edge table and the external-
// reference rescue"). A server fiber reads a request off a socket, stores a
// MEMBER of the parsed request (`(get req :params)`) into a module-level
// `@`-capture cell, then reads a SIBLING member (`(get req :id)`) inside a
// `protect` sub-fiber to frame the reply — so `req`'s region is capture-adopted
// into that fiber's closure and would die with its subtree drop at the fiber's
// completion, while the cell still holds the `:params` member inline in it (the
// mu lib/cont/ipc.lisp driver-callback shape: `(assign got-X params)` while the
// same dispatch reads the request's id). Green pins that the drop rescues the
// externally-referenced region to the RC baseline: the cell's read after the
// fiber completes sees the live member, and the region frees at the cell's
// release. The rescue unit family is `regionstore::tests::forest`. Subprocess
// guardfree run, same rationale as the twin above.
#[test]
fn region_capture_cell_member_cascade_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-member-cascade-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// A `moves_out` REMOVE (`%pop`) returns a HEAP element that was pushed into a
// LOCAL OWNED container via a funnel. `(%array-push a (list …))` on a local `@[]`
// the ownership forest made Owned emits an `AdoptRegion` moving the list into `a`'s
// Owned subtree (RC frozen). `%pop` moving it back out must EXTRACT it — un-record
// the container edge and move it `Owned → Counted(1)` (`extract_owned_region`) — or
// the list stays interior and `a`'s subtree drop frees it while the returned Value
// still points into it (a stale-region-deref UAF; state-dependent, so the fixture
// primes id churn then drives the raw tail-pop loop). Separately, the native-tail
// path's ReturnValue `IncrefValueRegion` over the moved-out element is redundant
// (the element already carries its one caller reference), so it is suppressed for a
// moves_out ∩ PassThrough site (`RegionInfo::moves_out_release_sites`) — without
// that, tail `%pop` leaks 1 region/op. Green proves both: no over-free and no
// per-op growth. Full repro in the fixture header.
#[test]
fn region_pop_tail_moves_out_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-pop-tail-moves-out-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// GREEN guard — the stdlib `pop` wrapper (`%pop`/`%pop-string`/`%pop-bytes`) stays
// balanced across all three mutable container types. The @array arm suppresses its
// moved-out element's redundant tail retain (and extracts an Owned element); the
// @string/@bytes arms return a FRESH grapheme / immediate that must KEEP their tail
// retain — over-suppressing them over-frees the returned grapheme (the Q1 hazard).
// The wrapper's owned-param container also strands across the match arms and is freed
// per-arm by the container compensation. Runs clean; a regression that unbalances any
// arm SIGSEGVs here. Full detail in the fixture header.
#[test]
fn region_pop_wrapper_types() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-pop-wrapper-types.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the general container-READ-escape sibling of the pop moves-out UAF. A heap
// element pushed into a LOCAL Owned @array via a raw `%array-push` is adopted into the
// container's Owned subtree, so reading it back out with `first`/`get`/`rest` and
// letting the result ESCAPE must NOT leave it interior — else the container's
// scope-exit subtree drop frees it under the escaped reference (a stale-region deref
// once ids recycle). Escape propagates through the container read (`analyze_escape`'s
// read-result → container-contents edge): an escaping element-read marks the
// container's stored contents escaping, so the ownership forest refuses to adopt them
// and the ordinary RC path keeps them live across the caller's read. DISTINCT from the
// pop case (a read BORROWS — the element stays in the container — so the fix is escape
// marking, not pop's extract). Runs clean; a regression that re-admits the adopt
// SIGSEGVs here. Full repro + trace in the fixture header.
#[test]
fn region_container_read_escape_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-container-read-escape-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the container-read BORROW, the LOCAL sibling of the escape face above: a
// value read out of a container with `get`/`first`/`rest` still lives INSIDE that
// container, and the two read forms are kept alive differently. An OPCODE read
// (`%get`/`%first`/`%rest`) raises no count, so the container's lifetime is the
// borrow's only protection and its `decref_point` extends to the reader
// (docs/impl/region/rules.md Rule 4, the borrowing node) — this bites even with a
// PARAM container, no ownership subtree in play. A NATIVE read takes the Rule 5
// pass-through retain, which the RC baseline honours but ADOPTION freezes: the
// ownership cut must refuse a subtree whose member a read alias can still name, and
// order the alias's page-reading release ahead of the container's where the two
// coincide. The fixture drives every face past a priming loop and then asserts
// region-count bounded, so a regression in either direction — over-free, or a
// stranded lifetime — is loud. Full mechanism in the fixture header.
#[test]
fn region_container_read_borrow_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-container-read-borrow-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the counted container read is retained by every BINDER FORM that records
// it (docs/impl/region/bindings.md § "Every binder form that records the read must
// emit the retain"). A name bound to a whole-value read of a re-storing container
// borrows a reference the next overwrite releases, so the reader takes one of its
// own — and the container is handed its donation on the strength of that. The
// analysis records the read from both binder arms of the walk, so a module-scope
// reader (the file-letrec binder) is as exposed as the fn-local `let` reader
// tests/elle/region-reassign-captured-cell-reader.lisp pins. Emitting the retain in
// only one of them runs both halves of the bargain against a reference nobody took:
// the overwrite frees the value under the reader, and the reader's own placeholder
// release decrefs it again. Full shape in the fixture header.
#[test]
fn region_container_read_toplevel_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-container-read-toplevel-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the opaque CALL between a container and the value read out of it. A funnel
// adopt freezes its member's RC, so the forest owes a bound on every alias that can name
// the member; it reads those bounds off a native read whose container argument IS the
// container. A call hides that: `(concat a @[1 2])` on a MUTABLE first argument returns
// `a` itself, so reading out of the call's result is reading out of `a` under a
// placeholder that relates to no member; and `(last a)` hands back the adopted element
// directly. Only `Fresh`/`Stores`/`Sends` (a result in the call's own minted region — the
// claim the effects oracle checks) or `Immediate` rule the aliasing out. The fixture
// drives both faces past a priming loop, then samples each shape's per-op region growth:
// the returned member must read bounded (the refusal puts it back on an RC baseline that
// still reclaims it), and the concat shapes are pinned shrink-only over the per-call
// residue a mutable-first-argument `concat` carries on its own. So a regression in either
// direction — over-free, or a subtree traded for a leak — is loud. Full mechanism in the
// fixture header.
#[test]
fn region_call_result_alias_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-call-result-alias-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the tail-call move is one reference per OCCURRENCE, not one per call
// (docs/impl/region/rules.md Rule 5). A tail call pure-moves its arguments, and the
// frame holds ONE reference to a region while the callee releases once per PARAMETER,
// so an argument list naming the same region twice — `concat`'s
// `(concat-seq a rest a false)`, or two aliased bindings — hands over one reference
// against two releases and the second zeroes it under the caller's live value. Only the
// first owned occurrence is funded by the move; later ones are minted as a borrowed
// argument is, and the fixture also samples steady-state growth so a mint that is never
// consumed reads as a leak rather than passing. Full mechanism in the fixture header.
#[test]
fn region_tail_repeated_arg_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-tail-repeated-arg-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the variadic TAIL-FORWARD reference balance. Forwarding a heap value into a
// `& rest` variadic through a tail call builds the callee env as a MOVE
// (`own_params = false`): the caller's owning reference transfers, but a rest arg
// lives in the collected rest-list (its own `alloc_obj` incref), so the moved-in
// reference is surplus and must be released (`args_to_list`'s caller in `vm::env`),
// applied only to a value appearing exactly once across all arg positions. Under-
// release leaks (the `store-wrapper` oracle probe); OVER-release (an aliased/borrowed
// arg) faults under guardfree once the freed page recycles. This drives both the
// minimal forward and the stdlib-`put` store-wrapper shape past a priming loop, then
// asserts region-count bounded — so a regression in either direction is loud. Full
// mechanism in the fixture header.
#[test]
fn region_variadic_tail_forward_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-variadic-tail-forward-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the abort-delivery retain (docs/impl/region/owner.md § "Park/unpark
// symmetry", the delivery rule). A replayed frame's pending release consumes
// one owning reference of the value it is resumed with; a normally-completing
// child funds it with its Return's ReturnValue retain, but an ABORTED child's
// error exit runs no Return — so the reference it consumes is the one
// `fiber/abort`'s injection minted, and the replay is one of the four consumers
// that single mint answers for. Without a mint anywhere the replay steals a
// reference the abort's caller still owns and the payload is freed under the
// caller's read (a stale-region deref once ids recycle).
// The shape needs an io-parked protect child under the scheduler and a FRESH
// heap payload (a constant payload has no region and masks the theft);
// tests/elle/grpc.lisp's `with-server` teardown is the full-network witness.
#[test]
fn region_fiber_abort_io_protect_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-fiber-abort-io-protect-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the fiber-member ownership refusal (docs/impl/region/adopt.md § "The
// fiber member — refused at the class level"): a fiber's region is never a
// member of a region-rooted Owned subtree, so a fiber read back out of runtime
// graph state (`fiber/child`) rides a genuinely counted pass-through retain. The
// counterfactual is the capture adopt of a sole-captured `fiber/new` result into
// its capturing closure's region: the read's retain lands inert on the frozen
// RC and the outer fiber's release subtree-drops the child under the returned
// borrow — a stale-region deref (generation stamp) at the exhumed fiber's next
// use. The churn face pins that the refusal reclaims on the RC baseline rather
// than trading the UAF for a leak. Full mechanism in the fixture header.
#[test]
fn region_fiber_exhume_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-fiber-exhume-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the per-path return frontier (docs/impl/region/mechanism.md § "The return
// frontier is per-path"). A returned region is the caller's to free only on the
// paths that hand it over; a branch arm that leaves without it, or one that leaves
// WITH it while a sibling arm holds the `decref_point`, still owes the callee-side
// release. Both compensations are RC-neutral only if they land on the right path:
// the dead-arm head release must not fire where the mint did, and the returning
// arm's release must follow its mint. Getting either wrong frees the value under
// the caller's read — silent on the plain tiers once the page recycles, a
// deterministic fault here. The file drives both arms of every shape past a priming
// loop and reads the result each time, so an over-free is loud and the leak face is
// pinned by the same region-count deltas. Full mechanism in the file header.
#[test]
fn region_return_arm_escape_uaf() {
    run_elle_script_with_args(
        "region-return-arm-escape-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a fn-local 1-slot container's content needs BOTH of the cell's release
// channels (docs/impl/region/bindings.md § "Reassigned mutable bindings are 1-slot
// containers"): drop-on-overwrite for each displaced prior, the content drop at the
// cell's demise for the final one, with the producer's separate claim released at the
// store. Leak faces assert bounded region growth across a loop-carried cell, a cell
// bound inside the loop body, and a cell written once. The over-free faces read the
// content back inside the loop, after it, and out of a container that outlives the
// cell — so a demise that fires early, or a producer release that frees what the cell
// still holds, faults here rather than recycling silently. Full mechanism in the
// fixture header.
#[test]
fn region_fn_local_cell_drop_uaf() {
    run_elle_script_with_args(
        "region-fn-local-cell-drop-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — branch compensation reads the ARM STRUCTURE, neither the branch's kind nor
// its arity (docs/impl/region/mechanism.md § "The return frontier is per-path"). A `match` arm
// that never touches a live local owes that local's release, exactly as a two-armed
// `if`'s dead arm does. The head release is the one admitted unconditionally past
// the return frontier, so landing it on the wrong arm frees the value under the arm
// that reads it — or under the caller that was just handed it. The file drives every
// arm of every shape past a priming loop and reads each result, so an over-free
// faults deterministically here while the leak face rides the same region-count
// deltas. Full mechanism in the file header.
#[test]
fn region_match_dead_arm_uaf() {
    run_elle_script_with_args(
        "region-match-dead-arm-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — park/unpark symmetry for fiber suspension (docs/impl/region/owner.md
// § "Park/unpark symmetry"): a parked-then-dropped / drained / cancelled /
// aborted / denied fiber reclaims its region and parked state, the nested
// tail-position resume frees the inner fiber, and a literal-lambda tail callee
// defers its closure release. Leak faces assert bounded region growth; the
// over-free face (a mis-fix releasing live parked state — e.g. a parked frame's
// stale activation-map entries) faults under guardfree once ids recycle. Full
// mechanism in the fixture header.
#[test]
fn region_fiber_park_symmetry_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-fiber-park-symmetry.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — stdlib `compose`/`comp` compose correctly, no over-free. A
// self-tail-recursive HOF (stdlib `fold`'s letrec `go`) reached from >= 2 call
// sites in a unit must not over-free a value its tail call transferred forward.
// The region walk's callee inline (`try_inline_call`, whose sole job is to
// surface a callee body's cross-region EDGES at the call site) binds the
// callee's params to the CALLER's arg regions; a `Return` reached inside that
// re-walk names the arg region, not the value the callee structurally returns.
// Recording it in `return_sites` pins the transferred arg's `decref_point` to
// the callee's base-case (sibling) arm, and under self-tail-call frame reuse the
// branch-union release over-frees the reducer result the tail call already moved
// into the next accumulator. The interprocedural return facet is escape.rs's
// authority (a summary, not a re-walk), so the inline records return-frontier
// extensions only on the structural walk — mirroring the `inline_depth == 0`
// gate the Letrec/Let cell mint already uses. `compose`/`comp` fold `identity`
// with a closure-returning reducer, so the composed closure is exactly such a
// transferred accumulator; this exercises the full user-visible surface plus the
// isolated single-step fold. Armed under guardfree so any regression faults
// deterministically at the freeing decref. The corpus witness is
// tests/elle/functional.lisp's compose section, surfaced by the batched smoke
// gate. Full mechanism in the fixture header.
#[test]
fn region_compose_closure_acc_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-compose-closure-acc-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — a `squelch`/`attune` wrapper closure run as a fiber body. The wrapper
// shares the inner closure's template and env (their backing lives in the INNER
// closure's region), but the wrapper VALUE itself lives in a fresh region. A fiber
// keeps its body's regions alive by scanning the body's `closure` for cross-region
// edges — env backing and template — AND its `closure_value` (the wrapper value it
// installs as the body's executing-closure register on resume). Omitting
// `closure_value` from that scan leaves the wrapper value's region uncounted: for a
// plain closure it coincides with the template/env region (still kept alive), but a
// squelch/attune wrapper puts the value in a DIFFERENT region, which then frees at
// its binding's decref_point while the fiber still holds it — the next region's
// free-time scan reads the freed page. Runs under guardfree so a regression faults
// deterministically at that stale read rather than reading a recycled page. The
// harness runs the file under its vm/jit policies WITHOUT the oracle, where the read
// is stale-but-intact and silent. Canonical shape:
// tests/elle/region-squelch-fiber-uaf.lisp (squelch + fiber, attune + yield).
#[test]
fn region_squelch_fiber_uaf() {
    run_elle_script_with_args("region-squelch-fiber-uaf", &["--trace=guardfree"]);
}

// Guard — `fiber/abort` injects a payload the CALLER owns, whose one reference
// answers the caller's ARGUMENT release alone; no raise minted a delivery for it.
// Exactly one release then fires on it as a RESULT, and `inject_error_at_suspension`
// mints that reference once for whichever of the four consumers the injected error
// reaches (docs/impl/region/effects.md § `Delivers`). Under-mint and the payload's
// region is freed while a fiber and the caller still point into it — a stale read the
// harness's ordinary vm/jit policies see as an intact recycled page, and which only
// guardfree faults on deterministically. Over-mint never faults, so the leak face is
// the `abort-*` probe family in `tests/elle/oracle.lisp`, one probe per route and per
// recorded mint. The bounded-growth face of the same declaration is
// tests/elle/region-fiber-install-clique-leak.lisp.
#[test]
fn region_fiber_abort_delivery_uaf() {
    run_elle_script_with_args("region-fiber-abort-delivery-uaf", &["--trace=guardfree"]);
}

// Guard — a `@`-mutable parameter reassigned in its body, whose (post-reassign)
// value is MOVED into a tail call. The param is materialized as a capture cell the
// callee owns; the tail-call move must retain the moved value (the borrowed-tail-arg
// retain) BEFORE the param cell's last-use `DecrefCellRegion`, whose cascade frees
// the cell's contents — the very value being moved. Emitting the cell release first
// frees the moved value and the retain then reads a freed page (the owned-params
// double-release). `lower_call` defers the tail arg's decrefs so the retain orders
// ahead of them. Runs under guardfree so a regression faults deterministically at
// that stale read; the harness runs the file under its vm/jit policies WITHOUT the
// oracle, where the freed page is stale-but-intact and the functional asserts pass.
// Canonical shape: tests/elle/region-mutable-reassign-param.lisp (overwrite-return,
// multi-reassign chain, aliased-arg clobber, id-recycling loop).
#[test]
fn region_mutable_reassign_param_uaf() {
    run_elle_script_with_args("region-mutable-reassign-param", &["--trace=guardfree"]);
}

// Guard — the BORROWED tail-arg retain must see THROUGH a branch/phi. A borrowed
// upvalue hidden behind an `(or borrowed fresh)` (or any `and`/`if`/`cond`/`match`)
// passed as a tail-call argument to an owned-param callee must still be recognized
// as borrowed, so the callee is handed a fresh owning reference instead of pure-
// moving the capture. `tail_arg_is_borrowed` (src/lir/lower/control.rs) sees a bare
// `Var`/`DerefCell(Var)` upvalue; a naive predicate returns false for an `Or` node,
// so the borrowed short-circuit operand is pure-moved and the owned-param callee's
// release drains the capture RC to a premature free (SIGSEGV under guardfree,
// `DecrefValueRegion of struct … context UpdateCapture`). The retain and the operand
// releases are value-gated, so a single retain balances BOTH arms; the fixture's
// subjects B/C guard that balance from below (no over-free on the borrow arm's
// mutable-store escape) and above (no over-incref leak when the FRESH arm is taken).
// Canonical shape: tests/elle/region-or-tail-move-borrow-uaf.lisp (the phi sibling of
// region-tail-move-borrow-uaf.lisp), plus the faithful `(protect (te (or state @{})))`
// form. Runs under guardfree so a regression faults deterministically.
#[test]
fn region_or_tail_move_borrow_uaf() {
    run_elle_script_with_args(
        "region-or-tail-move-borrow-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// The CONST sibling of region_tail_move_borrow_uaf: a compile-time-constant HEAP
// value — a stdlib-export closure (`+`/`inc`/`map`), a primitive's closure value,
// a `begin-for-syntax` value — reads as `LoadConst` from `immutable_values`
// (never captured; hir/analyze/scopes.rs skips the capture for a known-constant
// binding), so the frame owns NO reference to it. Tail-moving it into an
// owned-param callee lets the callee's release drain the stdlib env's region rc
// by one per call to a premature free: user-reachable as `(defn f [xs] (map inc
// xs))` — a handful of calls frees `inc`'s region under the live stdlib env
// (SIGSEGV under guardfree, tag/object-mismatch panic without). GREEN since
// `arg_leaf_is_borrowed` treats a constant HEAP value as borrowed (one fresh
// owning reference, consumed by the callee's release); the fixture's witness (c)
// guards the balance from above (no over-incref leak). Canonical shape:
// tests/elle/region-const-tail-move-borrow-uaf.lisp.
#[test]
fn region_const_tail_move_borrow_uaf() {
    run_elle_script_with_args(
        "region-const-tail-move-borrow-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a mutable @set del of a HEAP member must release the STORED member's
// region, not the caller's lookup value. `(del s x)` removes the element
// value-EQUAL to `x`; for a heap member the stored element and `x` are two
// distinct allocations in distinct regions, and the add half recorded the
// outgoing edge / incref against the stored member's region. A set remove that
// resolves the un-record + decref from `x` (a bare `BTreeSet::remove` yields no
// element) un-records an edge that was never recorded — outgoing-edge accounting
// drift the debug equivalence oracle detonates on — and over-frees the caller's
// live region under guardfree, while the stored member leaks. `set_del_with_decref`
// resolves both from the member `take` hands back, mirroring the @struct/@array
// removes. Quarantined as a subprocess because a regression ABORTS (oracle panic /
// guardfree fault) and would take the shared smoke harness down. Full repro +
// invariant in the fixture.
#[test]
fn region_set_del_heap_member_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-set-del-heap-member-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// Guard — the F1b container compensation must release ONLY the store wrapper's
// stranded owned-param reference, never a live container. A polymorphic
// `push`/`put`/`add` reached as a value runs its `(match (type-of coll) …)` body,
// whose mutable arm tail-calls a `-mut` funnel returning the container arg0
// pass-through; the wrapper leaks its owned-param reference to that return-escaping
// container (1/op). The close balances it with a per-arm release in the wrapper
// body (`regions::compensate`, `funnel_container_sites`) plus suppressing the
// redundant tail ReturnValue retain (`lir::lower::control::call`). Because the
// funnel's `pass_through_retain` already handed the caller one owning reference,
// releasing the owned-param reference can never drop the live container to zero —
// but an over-aggressive release would free a container the caller still holds. The
// fixture builds ESCAPING array/set accumulators and a nested pass-through wrapper,
// reading every stored element back across an id-recycling loop, so such an
// over-free faults under guardfree. Quarantined as a subprocess because a
// regression ABORTS (guardfree fault / oracle panic) and would take the shared
// smoke harness down. Full repro + invariant in the fixture.
#[test]
fn region_mut_container_compensation_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-mut-container-compensation-uaf.lisp",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a struct storing a HEAP-valued key (a list/bytes/set/struct used as a
// key, held as `TableKey::Heap(Value)`) records the outgoing content edge
// `region(struct) → region(key)` and increfs the key's region, exactly as it
// does for a struct VALUE. The key value is built in the caller's region and
// pointed at from the struct's region — a cross-region reference the alloc-time
// scan (`find_object_cross_refs`) must enumerate so the free-time cascade
// balances it. Enumerating only the values (the old struct arms) left the key's
// region reclaimed at its constructor's decref_point while the struct still
// pointed into it: a stale key comparison on the next `get`/`put` (binary search)
// derefs the freed page, and — because the drifted region gets reused — reads
// live-but-wrong data, silently collapsing distinct compound keys onto one slot.
// Quarantined as a subprocess because a regression ABORTS (guardfree fault /
// oracle panic) and would take the shared smoke harness down. Full repro +
// invariant in the fixture.
#[test]
fn region_struct_heap_key_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-struct-heap-key-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// Guard — the MUTABLE-@struct twin of the above: an in-place `put` that ADDS a
// heap-valued key records the `region(struct) → region(key)` edge and increfs the
// key, and a `del` un-records + decrefs it (`struct_put_with_rebind` /
// `struct_remove_with_decref`). The alloc-scan handles keys present at
// construction, but an in-place put adds a key AFTER allocation, so the store
// funnel must record it — enumerating only the value left the free-time content
// scan (which walks keys) disagreeing with the recorded edge table, a missed
// store-funnel edge the equivalence oracle detonates on. Quarantined as a
// subprocess because a regression ABORTS. Full repro + invariant in the fixture.
#[test]
fn region_struct_mut_put_heap_key_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-struct-mut-put-heap-key-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// Guard — a leaf helper called many times from a driver. The callee closure lives
// in a letrec forward-reference cell the driver captures BY INDIRECTION (an
// uncounted cell store the ownership scan cannot see). The forest must treat that
// capture as a borrow (`needs_capture` in `capture_containment_edges`), never
// folding the callee's region into the driver's Owned subtree nor claiming it Owned:
// the closure region reclaims on the per-region-RC baseline (kept live by the
// runtime auto-incref over the driver's env). Adopting it — the acyclic
// forward-reference the closure-cycle MERGE does not cover — defers the cell's free
// past the closure's own, so a later region's free-time cross-ref scan reads the
// reclaimed closure page. Runs under guardfree so a regression faults
// deterministically at that stale read; the harness runs the file under its vm/jit
// policies WITHOUT the oracle, where the read is stale-but-intact and the
// bounded-growth asserts pass. Canonical shape:
// tests/elle/region-repeated-call-adopt-uaf.lisp.
#[test]
fn region_repeated_call_adopt_uaf() {
    run_elle_script_with_args(
        "region-repeated-call-adopt-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — an os/spawn worker pre-allocates a capture cell for each of the
// spawned closure's captured LOCALS so the body's UpdateCapture/DecrefCellRegion
// find them in the env. Each such cell must live in its OWN region, never the
// worker's `recv_region`: the body owns the cells and frees them with
// `DecrefCellRegion` (value-resolved to the cell's region) at scope exit, so a
// cell in recv_region would drive recv_region's RC to 0 mid-body and the worker's
// cleanup `decref_region(recv_region)` then double-frees a phantom region. This
// subprocess runs the JIT tier under the guardfree oracle, where a regression
// faults deterministically on the worker thread. (`src/primitives/
// concurrency.rs`, the captured-local cell loop.)
#[test]
fn region_spawn_capture_mutate_guardfree() {
    run_elle_script_with_args(
        "region-spawn-capture-mutate",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Self-recursion correctness across control-flow boundaries, armed under the UAF
// oracle. A self-recursive local function must keep recursing as itself — same
// body, same captured environment — across a yield/resume, a tail-call frame
// replacement, or a value handoff. The corpus files assert the *values* (a stale
// self-reference returns a wrong-but-well-typed result the harness's vm/jit
// policies catch); these subprocess runs add the complementary guarantee that
// carrying the executing closure across each boundary reads no freed page — a
// botched self-identity that freed the live closure/env would fault here under
// guardfree rather than read recycled memory. `--jit=adaptive` exercises the
// hot-compiled path while the recursion is still in flight.
#[test]
fn recur_after_yield_guardfree() {
    run_elle_script_with_args(
        "recur-after-yield",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_after_tail_call_guardfree() {
    run_elle_script_with_args(
        "recur-after-tail-call",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_as_value_guardfree() {
    run_elle_script_with_args(
        "recur-as-value",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_entry_guardfree() {
    run_elle_script_with_args(
        "recur-entry",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// In-lambda MUTUAL recursion under the UAF oracle: the closure-cycle merge puts
// the ev/od pair and their forward cells in ONE arena, released either by the
// letrec binding-scope drop (non-tail body) or by the tail-call deferred release at the
// recursion's normal completion (tail body). A mis-accounted release — the arena
// freed while a rotation is still in flight, or freed twice across the two
// channels — reads a freed page here and faults deterministically.
#[test]
fn recur_mutual_guardfree() {
    run_elle_script_with_args(
        "recur-mutual",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// The adaptive-JIT build of the same entry-boundary coverage: the adaptive
// tier compiles a hot caller while its self-recursive callee is still
// interpreted — the compile-window shape the stdlib-HOF probe in the file
// exercises. The harness runs the file on the default (VM) tier; this
// subprocess covers the JIT half.
#[test]
fn recur_entry_jit() {
    run_elle_script_with_args("recur-entry", &["--jit=adaptive", "--mlir=off"]);
}

// =============================================================================
// Backend toggles paired with the JIT
// =============================================================================

// Guard — a JIT-compiled fiber that suspends mid-I/O must not over-release the
// yielded io-request region. `--mlir=off` pins the pure-JIT path (the invariant
// must not depend on the MLIR backend being present); the harness's vm/jit
// policies don't isolate this combination on an MLIR-enabled build.
#[test]
fn region_jit_io_suspend_uaf() {
    run_elle_script_with_args("region-jit-io-suspend-uaf", &["--mlir=off"]);
}

// Guard — an io completion struct shares the reaping call's region, so the
// scheduler pump's release of the `io/wait` array cascades to the payload the
// backend built and handed the resumed fiber. That fiber's own reference is
// what must carry the payload past the cascade; under the UAF oracle a missing
// one faults at the read instead of returning a recycled page.
#[test]
fn region_io_completion_leak_guardfree() {
    run_elle_script_with_args(
        "region-io-completion-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// =============================================================================
// I/O backend selection (`--no-uring`)
// =============================================================================

// Same script the harness already runs (`posix.lisp`), but forced onto the
// threadpool I/O backend on Linux via `--no-uring` — a process-global choice the
// harness cannot make per file. The threadpool path uses the same
// `SignalReceiver` / `kq_sig_read_blocking` / `sigfd_read_blocking` machinery as
// macOS, so this gates the threadpool signal flow (the signalfd EAGAIN-poll path
// on Linux and the EVFILT_SIGNAL worker-unblock + no-op sigaction path on
// macOS). Without it we'd only exercise the io_uring path on the Linux runner.
#[test]
fn posix_threadpool() {
    run_elle_script_with_args("posix", &["--no-uring"]);
}

// The full-write invariant on the OTHER backend. `port-shortwrite.lisp` proves
// that `port/write` transfers every byte of a payload far larger than one
// write(2) can move; the harness runs it on io_uring (the Linux default), where
// `drain_cqes` resubmits the unwritten tail. `--no-uring` is a process-global
// choice, so this pin is the only way to cover the thread-pool worker's own
// write loop (`PoolOp::Write`) on the Linux runner — the path macOS always
// takes. See src/io/AGENTS.md § Full-Write Invariant.
#[test]
fn port_shortwrite_threadpool() {
    run_elle_script_with_args("port-shortwrite", &["--no-uring"]);
}

// `:timeout` on a write that outgrows one syscall, on the OTHER backend. The
// two backends bound a blocked operation by different means — io_uring links a
// timeout SQE, the thread-pool worker relies on the fd's own send timeout — so
// each needs its own coverage of the re-armed deadline. Measured before the
// fix, both ignored `:timeout` on the resubmitted tail identically: the call
// blocked until the peer closed the socket and then reported ECONNRESET.
// See src/io/AGENTS.md § Full-Write Invariant.
#[test]
fn port_write_timeout_threadpool() {
    run_elle_script_with_args("port-write-timeout", &["--no-uring"]);
}

// `:timeout` on the looping reads, on the OTHER backend. io_uring re-arms a
// linked timeout on each resubmission; the thread-pool worker takes the fd
// non-blocking and waits in `poll(2)`. The pool half needs its own coverage
// twice over: it is the sole mechanism on macOS, and it was the weaker of the
// two before — measured on this file, io_uring already bounded a single
// `port/read` while the pool backend bounded no read at all.
// See src/io/AGENTS.md § Operation timeouts.
#[test]
fn port_read_timeout_threadpool() {
    run_elle_script_with_args("port-read-timeout", &["--no-uring"]);
}

// Grapheme-counted `read-exact` framing, on the OTHER backend. The two backends
// assemble a text read's answer from different places — io_uring from the
// fiber's buffer, the pool worker from the bytes it hands back — so each needs
// its own coverage of a cluster too wide for that buffer. The pool half is the
// sole mechanism on macOS. See docs/io.md § "A read that overshoots keeps the
// rest for the same port".
#[test]
fn port_text_framing_threadpool() {
    run_elle_script_with_args("port-text-framing", &["--no-uring"]);
}

// A line longer than the buffer `read-line` reserves, on the OTHER backend. The
// two backends outgrow that buffer in different places — the pool worker reads
// to the newline and hands back every byte at once, io_uring fills the buffer
// and resubmits — so each needs its own coverage. The pool half is the sole
// mechanism on macOS. See docs/io.md § "A read that overshoots keeps the rest
// for the same port".
#[test]
fn port_longline_threadpool() {
    run_elle_script_with_args("port-longline", &["--no-uring"]);
}

// Two timed operations on one descriptor, on the OTHER backend. The bound the
// pool worker uses is descriptor state the operations share, so the file only
// measures anything where that mechanism runs: io_uring gives each operation
// its own linked timeout and shares nothing. See the fixture header and
// src/io/AGENTS.md § Operation timeouts.
#[test]
fn port_timeout_shared_fd_threadpool() {
    run_elle_script_with_args("port-timeout-shared-fd", &["--no-uring"]);
}

// `:timeout` on the calls that wait for a peer, on the OTHER backend. io_uring
// links a timeout SQE to the accept and the receive; the pool worker has to
// bound them itself, waiting in `poll(2)` for the listener or the socket to be
// readable rather than parking in `accept(2)`/`recvfrom(2)` where no deadline
// can reach it. That mechanism is the only one on macOS, and it is the half
// this file measures — on io_uring the same script passes either way.
#[test]
fn net_wait_timeout_threadpool() {
    run_elle_script_with_args("net-wait-timeout", &["--no-uring"]);
}

// What a cancelled operation gives back, on the OTHER backend. `:workers`
// counts thread-pool operations submitted and not yet reaped, and io_uring runs
// most of these in the kernel — so it is zero there whatever the pool does. The
// worker half of the promise is only measurable here.
// See src/io/AGENTS.md § "I/O Cancellation".
#[test]
fn io_cancel_releases_threadpool() {
    run_elle_script_with_args("io-cancel-releases", &["--no-uring"]);
}

// An operation whose asking fiber is gone must end itself, on the OTHER
// backend. The pool ends one through its stop pipe and io_uring through
// `IORING_OP_ASYNC_CANCEL`, so neither half says anything about the other; the
// pool's is what macOS always runs. `:workers` measures the second claim the
// file makes — the thread comes back — and is zero on io_uring whatever the
// pool does. See src/io/AGENTS.md § "Ending an operation whose operands are
// gone".
#[test]
fn io_stale_operation_ends_threadpool() {
    run_elle_script_with_args("io-stale-operation-ends", &["--no-uring"]);
}

// (Hygiene for syntax-case bindings is carried structurally — synthetic-ness
// lives on PatternBinding (src/syntax/expand/syntaxcase.rs) rather than being
// inferred from a name's string prefix. The regression for it lives in
// tests/elle/macros.lisp as a plain-run test, where the harness owns it.)

// Deep fiber/resume nesting must not consume the host call stack. The
// bytecode-VM path routes nested resumes through the SIG_SWITCH trampoline
// in `do_fiber_resume` (src/vm/fiber.rs), so 20000-deep nesting completes;
// pinned under the process-global `--jit=off` so the VM path is what runs.
// See the fixture header.
#[test]
fn fiber_deep_nesting_vm() {
    run_elle_file_with_args(
        "tests/integration/fixtures/fiber-depth.lisp",
        &["--jit=off"],
    );
}

// The same file under `--jit=eager`. The fixture's `-jit` driver shapes are
// JIT-admissible (their `fiber/new` lives in a helper, so the recursive
// resume caller itself compiles), so this pin drives a compiled
// `fiber/resume` caller 20000 deep — the depth a per-level Rust frame
// residue would turn into a stack-overflow abort. See the fixture header.
#[test]
fn fiber_deep_nesting_jit() {
    run_elle_file_with_args(
        "tests/integration/fixtures/fiber-depth.lisp",
        &["--jit=eager"],
    );
}

// A parked activation's region-map snapshot named a region it no longer owned,
// so on resume the debug-only uncounted-borrow guard (src/vm/core/resume.rs →
// `first_stale_borrow`, docs/impl/region/generations.md § "Uncounted-borrow
// check") aborted: "stale suspended-frame region borrow on resume".
//
// Root cause: the activation region map records `static slot → physical region`
// for every ALLOC-slot allocation and is cleared only by the slot-based
// `DecrefRegion`. A region freed any OTHER way — a value-based `DecrefValueRegion`/
// `DecrefCellRegion` (capture cells), a cross-region cascade, a subtree drop —
// leaves its entry behind, and the physical id it names is recycled to an
// unrelated region. `record_region_borrows` stamped each parked entry with the
// id's CURRENT generation, so such a leftover was snapshotted as a live borrow of
// an incarnation the activation never owned; when that unrelated incarnation was
// later freed, the resume check tripped. signals.lisp's cumulative squelch/
// silence/yield churn recycles ids fast enough to hit it (state-sensitive — it
// does not minimize to a small standalone form, hence the coupling to the file).
//
// Fixed by carrying the establish-generation in the map (`MappedRegion`): the
// snapshot records the generation the slot was valid at and skips entries whose
// region has since moved on (dead leftovers), while a genuine borrow freed *while
// parked* still trips the check. The abort was DEBUG-ONLY (release compiles the
// guard out and the leftover's dead `DecrefRegion` never reads it, so signals.lisp
// passed in CI's release corpus); this runs the file under the debug cargo-test
// profile where the guard is live.
#[test]
fn signals_no_stale_suspended_frame_region_borrow() {
    run_elle_script_with_args("signals", &["--jit=off"]);
}

// A spawned fiber outlives the parameterize scope it inherited from, so its
// baseline snapshot must COUNT what it holds (docs/impl/region/owner.md § "A
// child's inherited parameter baseline is a counted holder"). This binary runs
// with debug assertions, where a missing seeding retain panics deterministically
// at the resume boundary (the generation-stamped borrow check,
// docs/impl/region/generations.md § "Uncounted-borrow check") — the reason the
// pin lives here rather than only in the release-built corpus, where the same
// defect surfaces as timing-dependent stale reads.
#[test]
fn region_param_fiber_inherit_uaf() {
    run_elle_script_with_args("param-fiber-inherit", &[]);
}
