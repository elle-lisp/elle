// audited: 2026-09-28
// Guardfree pins for the error exits: unwinds, emitted and injected raises, and restarts.
//
// docs/analysis/testing.md

use super::*;

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
// TAIL POSITION (docs/impl/region/park.md § "What yields is the emit OPERATION, not
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

// Guard — the abort-delivery retain (docs/impl/region/park.md,
// the delivery rule). A replayed frame's pending release consumes
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

// Guard — a restart delivers into an error park and owes what the raise site
// left unfunded (docs/impl/region/park.md § "A restart delivers into an error
// park"). A primitive, an instruction, a tail call, a callee, a replayed frame,
// a refusal over a denial park, and a parent a child's error stopped at its
// `fiber/resume` each produced no result, so the delivery mints the reference
// the continuation's release consumes. The fiber keeps the restart value in its
// result and the resumer reads it after its own release ran, so a missing mint
// faults there — SIGSEGV under guardfree. The leak face is
// `region-fiber-restart.lisp`.
#[test]
fn region_fiber_restart_uaf() {
    run_elle_script_with_args(
        "region-fiber-restart-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}
