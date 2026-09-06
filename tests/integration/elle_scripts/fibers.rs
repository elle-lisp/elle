// audited: 2026-09-05
// Guardfree pins for the fiber frontier: parks, resumes, error unwinds, squelch boundaries and the compiled tier.
//
// docs/analysis/testing.md

use super::*;

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

// Guard — the same raised delivery where the raise leaves the emit PRIMITIVE OFF
// TAIL POSITION (docs/impl/region/owner.md § "What yields is the emit OPERATION, not
// the `Emit` node"). There the site takes the retain, so the exit mints the delivery
// and leaves that retain to the continuation past the call. An `:error` fiber is
// resumable, so a RESTART replays that continuation: without the mint the replay
// releases the very reference the catcher already consumed, and every holder that
// outlives the fiber reads freed memory. Nine witnesses drive one raise each past a
// restart — a module-level binding, a captured local, a captured parameter, a
// body-allocated payload, a `fiber/value` read, a container, an uncaught propagation,
// a second restart, and one region named through both arguments — and read the
// payload afterwards, so an over-free faults under guardfree. Six controls remove one
// ingredient each and must stay clean, the sharpest being the same body resumed ONCE:
// with no replay the site's retain reaches only the catcher, which is why the shape
// reads correct until a restart claims it twice. A growth gauge refuses the trade in
// the other direction — a mint whose retain no route reaches strands one region per
// raise.
#[test]
fn region_dynamic_emit_statement_uaf() {
    run_elle_script_with_args(
        "region-dynamic-emit-statement-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the resume of a mediated capability denial releases the one reference
// the park has no body to release, and that decref answers for the payload's own
// left-over reference, never for a holder's (docs/impl/region/owner.md § "A
// payload the RUNTIME built is released by the install that displaces it"). The
// witness binds the payload in the mediating parent, resumes the fiber past the
// denial, churns the heap, then reads three payload fields; taking the holder's
// reference instead frees the struct under those reads — SIGSEGV under guardfree.
// This file's faces are the denial POSITIONS (call and tail); the per-install
// faces are `region_denial_park_uaf` below. The leak face is the region-count
// bound in the same file, which the object gauge in `tests/elle/oracle.lisp`
// cannot see.
#[test]
fn region_capability_denial_resume_uaf() {
    run_elle_script_with_args(
        "region-capability-denial-resume-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a park payload the RUNTIME built is released by the install that
// displaces it (docs/impl/region/owner.md § "Park/unpark symmetry"). A capability
// denial's payload is built by the denial path, so the body never names it and no
// continuation releases it; `fiber/resume`, `fiber/refuse` and `fiber/abort` each
// replace it in the slot and each owe that release. Every one of those releases
// fires on a path where none fired before, and the mediator is precisely the
// reader that may still hold the payload — `fiber/value` is pass-through, so a
// binding of it carries a counted reference the release must not consume. Each
// witness therefore reads a HEAP field (`:primitive`, `:args`) AFTER the install,
// including the shape whose resume value is read OUT of the payload's own region;
// a bare status check passes over a freed payload. Two controls drive a
// body-allocated `emit` payload through the same installs, where a release added
// would free it under these reads. The leak face is
// `tests/elle/region-denial-park.lisp`.
#[test]
fn region_denial_park_uaf() {
    run_elle_script_with_args(
        "region-denial-park-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the other park a payload the RUNTIME built (docs/impl/region/owner.md
// § "Park/unpark symmetry"): a yielding io op's `IoRequest`, which the native
// built and the body never named, so no continuation releases it. Every install
// that displaces the park owes that release, and `fiber/abort` / `fiber/refuse`
// each run one where none ran before. The mediator reads the request out of the
// park before it ends it — `fiber/value` is pass-through, so a binding carries a
// counted reference of its own — and every witness DEREFERENCES the request after
// the install; a bare status check passes over a freed one. The `:io` denial
// witnesses are the bits collision: a fiber denied `:io` parks under `SIG_IO`, so
// the ledger record and the io bit both answer for one park and exactly one
// reference is owed. Running both frees the payload under the mediator's read —
// SIGSEGV under guardfree. The leak face is `tests/elle/region-io-park.lisp`.
#[test]
fn region_io_park_uaf() {
    run_elle_script_with_args(
        "region-io-park-uaf",
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

// Guard — a squelch/attune boundary abandons its frames the same way an error
// does, so the discard runs the releases each of them still owed
// (docs/impl/region/mechanism.md § "A squelch boundary abandons frames the same
// way, so it runs the same walk"). The fiber SURVIVES the boundary, so what must
// be whole afterwards is everything the surviving side still reads: the CATCHING
// activation's own values, an OUTER non-discarded frame's, a value the abandoned
// frame STORED into a longer-lived container, a closure released by the SLOT
// route, and the scheduler machinery behind 160 boundaries. Every read happens
// after the discard ran, so an over-release faults there — SIGSEGV under
// guardfree. The leak face is `region-squelch-unwind.lisp`.
#[test]
fn region_squelch_unwind_uaf() {
    run_elle_script_with_args(
        "region-squelch-unwind-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a squelch/attune boundary ends a park with no reader and no install,
// so it releases both of the park's references (docs/impl/region/owner.md § "A
// boundary ends a park with no reader and no install"). Each is a decref that
// never ran before, and the fiber SURVIVES, so what must be whole afterwards is
// everything that still names the payload: a body-allocated one the emitting
// frame's own binding holds, one a longer-lived CONTAINER took a counted
// reference to, a BORROWED module-level binding the compiler minted the body's
// reference for, the io machinery behind 80 boundaries, and a park the install
// path still owns — resumed after all of them. Every read happens after the
// boundary ran, so an over-release faults there — SIGSEGV under guardfree. The
// leak face is `region-boundary-park.lisp`.
#[test]
fn region_boundary_park_uaf() {
    run_elle_script_with_args(
        "region-boundary-park-uaf",
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

// Guard — a `Fresh` io op builds its completion buffer in the request's own
// region, and the install that ends the park releases the suspend retain there
// like any other. The buffer the resume hands back must survive that release:
// under the UAF oracle a release that took one reference too many faults at the
// read of a held chunk instead of returning it.
#[test]
fn region_io_read_strand_guardfree() {
    run_elle_script_with_args(
        "region-io-read-strand",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
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
