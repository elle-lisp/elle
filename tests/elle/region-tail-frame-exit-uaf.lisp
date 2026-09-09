(elle/epoch 12)
# audited: 2026-09-08
# Soundness complement of region-tail-frame-exit.lisp: hoisting a release from
# the dead post-`TailCall` block to just before the `TailCall` must not free
# anything the callee can still reach.
#
# The hoist moves a release EARLIER — the one place in the region system it does
# (docs/impl/region/mechanism.md § "A release past a frame-replacing tail call is
# not a release"). On the closure path it fires where none fired before, so it
# needs BOTH gates, and each witness below drives one of them. The **exemption**
# covers what the call names: an argument's release must not fire before the
# callee it was moved to reads it, and the callee closure's own region must not
# be dropped before the frame it replaces runs. The **admission** — escape
# proving the frame holds the region alone — covers the path the exemption
# cannot see: a tail callee also reaches its CAPTURED environment, which no
# argument names — but the env's hold is the funnel's counted edge, so that path
# is admitted rather than refused, and (e3) is the row that proves the count
# stands. A value the callee hands BACK rides that same edge one step further —
# the callee's `Return` mints the caller's reference after the relocated release
# has run, and the env edge is what holds the region off zero in between, which
# (e2), (e5) and (e6) drive. (e15), (e16) and (e16b) drive the other end of the
# same enumeration: a point neither route reaches cannot mint against the region
# at all, so the relocated release is the last one.
# (e7) and (e8) drive the same admission for an ENV
# CELL's `DecrefCellRegion`, whose holder is REASSIGNED: that refusal is about a
# release routed through the holder's slot, and this one names the cell box no
# `assign` repoints. (e10) and (e11) drive that same box release where it is the
# SIBLING arm's head compensation instead of the relocated copy — the arm names the
# cell's binding nowhere, so the head route covers it, and what must survive is the
# capturer's counted edge and the cell's own content. (e12), (e13) and (e14) drive
# the sibling arm that READS the cell's binding, where the box release is the tail
# compensation instead: it must post-date the arm's read, leave the capturer's
# counted edge standing, and post-date the READER of an uncounted opcode-read
# borrow out of the cell. A closure the frame RETURNS carries its capture on the
# same counted edge, so it is admitted too and the edge is what must stand (e4, and
# e9 for the cell). What the admission still refuses is a holder escape marks by a
# facet no counted edge covers: a store into a longer-lived container (f), and a
# fiber crossing (h).
#
# Every witness reads the subject's HEAP contents on the far side of the tail
# call, through a chain long enough that an over-early free faults rather than
# reading stale but still-mapped bytes. A fresh subject per iteration keeps
# region ids churning so a freed region is recycled under the reader.


# The witnesses are spliced at compile time, so this is still one program
# driving one loop — the split is a reading budget (DOCUMENTATION.md).
(include-file "tailexit/move.lisp")
(include-file "tailexit/replica.lisp")

# ── controls: the same reads with a NATIVE tail call — correct now ────────────
(defn c-plain (i)
  (let [x (list (string "p" i) i)]
    (length (first x))))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
# The iteration count is a PAGE budget, not a confidence knob: `--trace=guardfree`
# leaks every freed page `mprotect(PROT_NONE)`'d, so witnesses × iterations is
# bounded by the process's map count and an added witness has to be paid for out of
# the loop bound. A freed region is recycled within a handful of iterations, which is
# what the pin actually rests on — so the bound is set for headroom, not for reach.
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var e2 0)
(var e3 0)
(var e5 0)
(var e6 0)
(var e7 0)
(var e8 0)
(var e9 0)
(var e10 0)
(var e11 0)
(var e12 0)
(var e13 0)
(var e14 0)
(var e15 0)
(var e16 0)
(var e16b 0)
(var e17 0)
(var e18 0)
(var e4 0)
(var f 0)
(var g 0)
(var h 0)
(var ai 0)
(var aj 0)
(var ak 0)
(var k 0)
(var l 0)
(var m2 0)
(var n2 0)
(var s1 0)
(var t1 0)
(var o1 0)
(var p1 0)
(var q1 0)
(var r1 0)
(while (%lt i 1350)
  (assign a (a-moved i))
  (assign b (b-moved-beside i))
  (assign c (c-callee-local i))
  (assign d (d-captured i))
  (assign e (e-walker i))
  (assign e2 (e2-walker i))
  (assign e3 (e3-walker i))
  (assign e5 (e5-read i))
  (assign e6 (e6-read i))
  (assign e7 (e7-read i))
  (assign e8 (e8-read i))
  (assign e9 (e9-read i))
  (assign e10 (e10-read i))
  (assign e11 (e11-read i))
  (assign e12 (e12-read i))
  (assign e13 (e13-read i))
  (assign e14 (e14-read i))
  (assign e15 (e15-read i))
  (assign e16 (e16-read i))
  (assign e16b (e16b-read i))
  (assign e17 (e17-read i))
  (assign e18 (e18-read i))
  (assign e4 (e4-read i))
  (assign f (f-read i))
  (assign g (g-return i))
  (assign h (h-fiber i))
  (assign ai (i-arm i))
  (assign aj (j-arm i))
  (assign ak (k-arm i))
  (assign l (l-read i))
  (assign m2 (m2-read i))
  (assign n2 (n2-read i))
  (assign s1 (s-read i))
  (assign t1 (t-read i))
  (assign o1 (o-read i))
  (assign p1 (p-read i))
  (assign q1 (q-read i))
  (assign r1 (r-lambda-arg i))
  (assign k (c-plain i))
  # The sink is a module-level container by design (witness f stores into it);
  # drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt k 0) "control: plain native tail read mis-read (harness broken)")

(assert (%gt a 0) "moved argument freed under the callee that owns it")
(assert (%gt b 0) "moved argument freed beside a hoisted sibling")
(assert (%gt c 0) "per-call callee closure freed under the frame it replaces")
(assert (%gt d 0) "captured value freed under the tail callee's read")
(assert (%gt e 0) "mutable accumulator freed under the walker that fills it")
(assert (%gt e2 0) "accumulator freed before the captured callee handed it back")
(assert (%gt e3 0) "accumulator freed under the caller's read of what it holds")
(assert (%gt e5 0) "moved-in accumulator freed before the callee minted for it")
(assert (%gt e6 0) "arm hand-back freed before the arm's callee minted for it")
(assert (%gt e7 0) "env cell freed under the callee that rewrites it")
(assert (%gt e8 0) "env cell freed under the arm's callee that rewrites it")
(assert (> e9 0) "env cell freed under a closure that escaped holding it")
(assert (> e10 0)
        "env cell freed under the closure the compensated arm hands out")
(assert (%gt e11 0)
        "cell content freed by the box release on the compensated arm")
(assert (%gt e12 0)
        "cell content freed by the reading arm's box release before its return")
(assert (%gt e13 0)
        "env cell freed under a closure that escaped before the reading arm ran")
(assert (%gt e14 0)
        "cell content freed under the reading arm's own opcode-read borrow")
(assert (%gt e15 0) "hand-back freed at an arm whose callee cannot reach it")
(assert (%gt e16 0) "fold accumulator freed under the combiner handed it next")
(assert (%gt e16b 0)
        "fold accumulator freed before its own combiner minted for it")
(assert (%gt e17 0)
        "a letrec closure's replicated release freed more than the frame's own \
         reference under a branch body tail")
(assert (%gt e18 0)
        "a scrutinee's replicated release freed more than the frame's own \
         reference at a merge inherited from the branch's entry")
(assert (> e4 0) "value freed under a closure that escaped holding it")
(assert (%gt f 0) "stranded value freed after being stored into a container")
(assert (%gt g 0) "returned value freed under the caller's read")
(assert (> h 0) "argument freed under a parked frame's resume")

(assert (%gt ai 0) "argument freed under the arm's callee that owns it")
(assert (%gt aj 0) "captured value freed under the arm callee's read")
(assert (%gt ak 0)
        "value released twice where the arm falls through to the merge")

(assert (%gt l 0)
        "forward cell freed under the deref that dispatches its sibling")
(assert (%gt m2 0) "forward cell freed before the handed-back capturer drove it")
(assert (> n2 0) "sibling freed under the caller that received it from its cell")
(assert (%gt s1 0)
        "arena freed under the sibling callee stranded by the same tail call")
(assert (%gt t1 0) "letrec member freed under the tail call that entered it")

(assert (%gt o1 0)
        "argument freed under the callee its own nested call handed it to")
(assert (%gt p1 0) "container freed under the element read out of it")
(assert (%gt q1 0) "container freed under an opcode read's borrow")
(assert (%gt r1 0) "capture freed under the lambda argument that holds it")

(println "region-tail-frame-exit-uaf: ok")
