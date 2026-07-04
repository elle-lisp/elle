(elle/epoch 12)
# Counterfactual: a squelch/attune-built closure run as a FIBER BODY leaves a
# closure region under-counted; at teardown that region frees while another
# region still references it, and the next free's cascade/scan reads the freed
# page — use-after-free (SIGSEGV under `--trace=guardfree`, the robust oracle;
# without guardfree the read is usually stale-but-intact and silent).
#
# The minimal shape needs all three ingredients, established by reduction:
#   - a `squelch`/`attune` WRAPPER (it shares the wrapped closure's template
#     and env backing — the counted cross-region references at stake);
#   - `fiber/new` on the wrapper;
#   - `fiber/resume` of that fiber (to completion or just past a yield).
# Each ingredient alone is clean: calling the wrapper directly, holding the
# un-resumed fiber, or resuming a fiber built from a PLAIN closure all pass
# guardfree. A squelch ABORT (signal-violation -> discard_suspended_frames) is
# NOT involved — the abort-discard shape is green, pinned by
# region-squelch-discard.lisp.
#
# Defect class: a borrowed reference consumed as if owned — the family
# region-tail-move-borrow-uaf.lisp pins (green since the borrowed tail-arg
# retain landed). The fiber-resume path of a squelch-built closure still
# drains one reference to the wrapped/inner closure's region without a
# matching incref, so the region's count underruns its live referents; the
# free surfaces at program teardown, where the guardfree fault names a
# closure region freed by a `DecrefValueRegion` with "1 later frees" — the
# next region's free-time scan is the read.
#
# RED now (the teardown scan faults); GREEN once the fiber-resume path hands
# the squelch-shared closure region an owning reference balanced against the
# release that currently underruns it. docs/impl/region-rules.md Rules 5/8.

# ── minimal: squelch + fiber/new + fiber/resume ──
(def sq (squelch (fn [] 42) :yield))
(def coro (fiber/new sq |:yield|))
(fiber/resume coro nil)
(assert (= (fiber/value coro) 42) "squelch-wrapped fiber body completes")
(println "squelch-fiber-minimal: ok")

# ── the attune + yield variant of the same family ──
# The wrapper's mask ADMITS the yield, so the fiber parks at it and is read —
# no violation, no discard; the under-count rides the same resume path.
(def yielder (fn [] (yield 42)))
(def attuned (attune |:yield :error| yielder))
(def coro2 (fiber/new attuned |:yield|))
(fiber/resume coro2 nil)
(assert (= (fiber/value coro2) 42) "attune-wrapped fiber body yields")
(println "squelch-fiber-yield: ok")

(println "region-squelch-fiber-uaf: ok")
