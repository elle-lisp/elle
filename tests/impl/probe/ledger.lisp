(elle/epoch 12)
# audited: 2026-09-29
# The ledger every probe is classified against: the roots, the by-design set, the dual-read table, and the driver each row table runs with.
#
# docs/impl/region/diagnostics.md
# docs/impl/memory.md
# ── The defect / by-design split — the instrument owns the burndown headline ──
# Every leak class has a ROOT, defined in docs/impl/memory.md (F1a, F1b,
# F2–F6), and the roots with open probes are declared below. A small fixed set
# of probes read open BY DESIGN — one live-growth discriminator per gauge (object
# count, physical id, region count, bytes), and the sub-integer estimator
# self-test — and are NOT counted as defects. The classifier below prints the
# open/closed split and the defect-vs-by-design breakdown, AND refuses to be
# silently wrong: every probe that MEASURES :open must be declared here (a
# root, or by-design), or the completeness gate at the end fails. A by-design
# open probe DISPLAYS :growth so `grep -c '^  open'` counts defects alone; the measured :verdict is untouched, so
# the gauge-live and B-invariance gates (which read it) are unchanged.
# `push-outer` and `recur-local-foreign-mint` are NOT here: their apparent growth
# was the F1b push-container over-keep of a BLOCK-LOCAL accumulator (freed at the
# block's return once the wrapper stops stranding its owned-param reference), not
# genuine unbounded retention — the real gauge-live discriminator (`probe-disc`)
# uses a MODULE-LEVEL sink and is unaffected. They are CLOSED controls
# (undeclared, like `rest-array-copy`), so a regression to open trips the
# completeness gate as an F1b defect rather than being absorbed as growth.
# `push-accum` is NOT here either, for the reason those two are not: its rate was
# the per-op `map` scratch its accumulator retained, and the accumulator itself is
# block-local, so it frees at the block's return. A CLOSED control, since the
# scratch is reclaimed — undeclared, so a regression to open trips the completeness
# gate as an F1a defect rather than being absorbed as growth.
(each l in ["discriminator (live-growth)" "id discriminator (live-growth)"
            "region discriminator (live-growth)"
            "bytes discriminator (live-growth)" "sub-integer (1-in-3 retain)"]
  (put by-design l true))
# `rest-array-copy` is a CLOSED control (the native fresh-result invariant, not F1a
# stdlib-body scratch) — undeclared like `slice`/`to-array`, so a regression to open
# trips the completeness gate loudly rather than being silently absorbed as F1a.
# `map-while`/`filter-while` are undeclared for the same reason: a fusable kernel
# dissolves, so they are CLOSED dissolution controls and a regression to open must
# trip the gate loudly instead of being absorbed as F1a scratch that is not
# there. The un-fused op's scratch keeps its F1a declaration through `wrap-map`.
# The `concat`/`append`/`fold` shapes — `concat`, `concat-while`, `stdlib-concat`,
# `stdlib-fold`, `yield-concat`, `string-outer`, `append-outer` — are CLOSED controls,
# undeclared for the same reason `rest-array-copy` is. Two readings of one
# window close them: `push-all`'s bulk arm returns the accumulator its sibling arm's
# walker captures, which the branch-arm window anchors (docs/impl/region/window.md),
# and the index-walk fold driver returns its accumulator from the base arm while
# the recursive arm hands the callee the COMBINER's result — a point that cannot
# reach the accumulator and so owes it no funding edge
# (docs/impl/region/relocate.md). A
# regression to open must trip the completeness gate loudly rather than be absorbed
# back into F1a.
# `group-by`, `frequencies`, `merge` and `each-list` are CLOSED controls too, under
# a different rule of the same window: one binding owns a region's release ROUTE, so
# a cursor an arm walks the input with refuses nothing (docs/impl/region/window.md).
# `zip-tower` is a CLOSED control too, for the same reason: the tower's `letrec`
# helpers are released at a merge every dispatch arm leaves through, and the
# frame-exit relocation replicates that release into each arm through the closure's
# value route (docs/impl/region/replicate.md).
(declare-root :f1a ["reduce" "fold"])
(declare-root :f1b ["mut-array-push" "mut-string" "struct-put" "push-churn"
                    "put-churn" "store-wrapper" "native-tail-put-struct"
                    "native-tail-put-array" "native-tail-del-ctl" "pop-wrapper"
                    "del-wrapper" "set-del-wrapper" "set-add"])
(declare-root :f2 ["fiber-nested" "multi-resume" "yield-discard"
                   "yield-multimut" "protect-while" "denied-discard"
                   "denied-discard@regions" "cancel-discard"])
# The `adopt-park-*` family is CLOSED on both dimensions (undeclared, like
# `rest-array-copy`, so a regression to open trips the completeness gate loudly
# rather than being absorbed under F2): the park split keys a second adopt
# ahead of the park, so an abandoned park's discharge frees the SCC through the
# parked frame's owner node (docs/impl/region/owner.md; the `ap-*` defns in
# probe/park.lisp hold the attribution set).
# `abort-tail-result` and `abort-mask-caught-literal` are CLOSED controls
# (undeclared, like `rest-array-copy`, so a regression to open trips the
# completeness gate loudly rather than being absorbed back under F2). What they
# measured was a fiber carrier in TAIL position whose outcome this fiber's mask
# absorbs: the request is answered here, so the value is the call's result and the
# frame never left — it takes the fall-through into the post-`TailCall` block, which
# runs the compiler's owned-argument releases and the return mint, exactly as the
# Call position and the JIT tier do (docs/impl/region/signalexit.md).
# `abort-discard` is a CLOSED control (undeclared, like `rest-array-copy`, so a
# regression to open trips the completeness gate loudly rather than being absorbed
# back under F2): what it measured was the borrowed-argument
# retain a native tail call's SIGNAL exit strands. The post-`TailCall` block that
# consumes that retain runs on the native's normal completion alone, so the exit
# consumes it instead — stamping the stash local `nil` so a replayed continuation's
# copy of the same release no-ops (docs/impl/region/signalexit.md). Its first
# stranded reference was the
# aborted fiber's own value, which pinned the body closure and everything the parked
# frame held behind it.
# `denied-discard` is a CLOSED control, and it stays declared under F2 so its
# rate keeps reading against that root. What it measured was what is LEFT of a dead
# continuation once the frames' own owed releases run: a frame abandoned by an
# error — and a parked one the fiber can never re-enter — reaches none of its
# remaining instructions, so each release among them runs off the value-route slots
# the emitter recorded (docs/impl/region/unwind.md), gauged directly by
# `tests/impl/region-error-unwind.lisp`.
# What that walk could not NAME was a value with no binding of its own, and the one
# this shape had is the ARGS ARRAY the denied `(println …)` builds for its
# `(apply string args)`: no binding names it, so no emitted release could either.
# The call that consumes an args array reclaims it
# (docs/impl/region/mechanism.md), gauged directly by
# `tests/impl/region-splice-args.lisp`.
# `spawn-join` is a CLOSED control (undeclared, like `rest-array-copy`), so a
# regression to open trips the completeness gate loudly rather than being absorbed
# under F2 — which is not where it belonged: what it measured was the frame-held
# admission refusing the FIBER facet, not park residue. A crossing leaves a counted
# holder — the park's `EmitEscape` retain going out, the resume value's own mint
# coming back — so the admission rides it and only the containment facets refuse
# (docs/impl/region/window.md).
# The io round trip (`io-yield ev/sleep`) and the displaced io park are gauged
# by the io dashboard, tests/impl/plumb.lisp — every probe whose drive reaches
# the io backend lives there, under this same ledger discipline.
# F4 has NO declared probe: the returned closure cycle is closed for every body shape,
# including the one bound OUT of its frame's tail position, where the merge follows the
# handed-out member to the release point the last-use rule already computed for it. Its
# four body shapes are CLOSED controls at 0 — `recur-local-mutual-ret` for a member tail
# call, `recur-local-mutual-ret-foreign` for a non-member one,
# `recur-local-mutual-ret-value` for a bare member value, and
# `recur-local-mutual-ret-bound` for the letrec bound out of tail position. All are
# undeclared, like `rest-array-copy`, so a regression to open trips the completeness gate
# loudly instead of being absorbed under the root. The class's other named shape — the
# ambiguous-owner / unemittable-edge subtree, the `compute_adopt_edges` refusals — still
# has no probe.
# `recur-local-self-mint` is NOT a member of this class despite the resemblance: the
# returned self-recursive closure records no region cycle at all and is cell-free, so
# it belongs to the deferred-release mechanism instead and is a control below.
# The whole `break-*` family is CLOSED controls (undeclared, like
# `rest-array-copy`), so a regression to open trips the completeness gate loudly
# instead of being absorbed as F5: `break-value*` pin the break TRANSFER (the
# value the break carries dies where the block's value dies) and `break-skipped`
# pins the window the jump passes over (every OTHER release between the break
# site and the exit label is re-anchored to the block).
# `take`/`drop`/`zip` are CLOSED controls for the per-path return frontier
# (undeclared, like `rest-array-copy`): all three are `letrec` walks whose base case
# returns a heap value the recursive arm's `decref_point` was left to release, so a
# regression must trip the completeness gate rather than be absorbed as an F5 strand.
# `match-dead-arm` and `match-used-arm` are CLOSED controls for the two faces of a
# region live-in to a branch (undeclared, like `rest-array-copy`): the dead arm
# takes per-arm compensation, and the USED arm is covered by anchoring the
# region's one release where every arm reaches it — the branch-arm release window,
# whose owned-parameter and `If` faces are `param-used-arm`/`param-used-arm-if`.
# `struct-outer` and `yield-reassign` are CLOSED controls for the fn-local 1-slot
# container (undeclared, like `rest-array-copy`), and they must stay a PAIR: both
# drive the container's two release channels — drop-on-overwrite for each displaced
# prior, the content drop at the cell's demise for the final one, with the
# producer's separate claim released at the store — but only `yield-reassign` has a
# HEAP init, the shape that reaches the gate's sole-held check, where
# functionalization's split of the cell's source name into a pre-loop version and a
# loop parameter reads as two holders of one name. A regression of either must trip
# the completeness gate rather than hide behind its sibling.
# `struct-match` is a CLOSED control for the scope map's completeness (undeclared,
# like `rest-array-copy`): a `match` arm's pattern binding records its scope, so a
# read of it inside a loop is not read as a read of a loop-external binding
# and the scrutinee's release stays in the body that allocates it.
# `recur-local-self-mint` is a CLOSED control for the return-funded deferred release
# (undeclared, like `rest-array-copy`): a cell-free self-recursive closure the
# recursion RETURNS has the tail-call deferred release as its region's only channel,
# and keeps it — the callee's `Return` mints the caller's reference before the
# trampoline runs the deferred decref, so the deferral drops only the frame's own
# (docs/impl/selfrec.md).
# Its control `recur-local-foreign-mint` is not self-recursive, so nothing strands it
# in the first place and the gap isolates the strand rather than the retain.
# `loop-acc-return` and `recur-acc-return` are CLOSED controls (undeclared, like
# `rest-array-copy`) for the RETURNED 1-slot container: a local accumulated across
# a loop and handed back counts what it holds exactly as an unreturned cell does,
# so each displaced prior dies at the overwrite and the final content leaves with
# the caller (docs/impl/region/bindings.md). They must stay a PAIR: `recur-acc-return` computes the identical
# value by threading the accumulator as a parameter to a self-recursive binding,
# so the gap between them is exactly what a programmer would have to give up to be
# leak-free, and a regression of either trips the completeness gate.
# `walk-feeder-return` and `walk-feeder-discard` are CLOSED controls too
# (undeclared, like `rest-array-copy`) for the element a WALK stores: the name
# `each` binds the element to is the store's feeder, so the container model holds
# and each element's producer reference dies at its own store
# (docs/impl/region/bindings.md). They must stay a PAIR — only the returned one reaches the
# `Return`'s mint, and only the discarded one leaves the content drop with nothing
# but the cell's scope node to place it.
(declare-root :f5 ["raw-del" "raw-del-immediate" "fresh-env-cell"])
# F6 has NO declared probe: a cursor walk's rate was the ALIASED INIT, not the
# cursor. `list-cursor` is a CLOSED control (undeclared, like
# `rest-array-copy`) for the counted-init half of the 1-slot container: a cell
# whose init value carries a second name counts that value instead of donating
# it, so it keeps the model — and with it the store-site pin that holds each
# step's release inside the loop (docs/impl/region/bindings.md). `each-manual` is its
# array control: an index walk names nothing it moves off and reads 0 either way,
# so the gap between the two isolates the cell rather than the loop.
# `cell-alias-after` is its ORDERING control (undeclared, like `rest-array-copy`):
# the same walk with the alias taken after the cell, so a whole-value read of the
# container takes a counted reference of its own and the cell donates its init
# (docs/impl/region/reads.md). The two must stay a PAIR — each isolates one of the two
# routes the init's producer reference can take — so a regression of either trips
# the completeness gate rather than hiding behind its sibling.

# ── The dual-read table — the authority for two-dimension coverage ────
# label → pinned region rate (shrink-only, like every pin). The runner reads a
# listed probe on the object count AND the region count in one drive
# (`measure-2`), pinning the region dimension under `label@regions`; the
# completeness gate fails the run when an entry recorded no region verdict, so
# membership cannot rot into silence (see the header's dual-read rule). The
# initial set is the park families — the machinery that moves whole region
# entries between fibers and frames — plus the adopt-park family, the one
# probe set whose activation MINTS an owner node. Region declarations ride
# `declare-root`/`@by-design` under the suffixed label, independent of the
# object dimension's.
(def @dual-read
  @{"fiber-nested" 0
    "multi-resume" 0
    "yield-discard" 0
    "denied-discard" 0
    "abort-discard" 0
    "cancel-discard" 0
    "adopt-park-drop" 0
    "adopt-park-abort" 0
    "adopt-park-cancel" 0
    "adopt-complete" 0
    "adopt-nopark" 0
    "plain-park-drop" 0
    "adopt-before-park-drop" 0
    "adopt-before-park-cancel" 0})
(def @dual-read-seen @{})


# The direct-loop driver, shared by every table of rows below. A row is
# `[label (fn [j] body) rate]`, and `j` varies the input so a body cannot
# constant-fold. A label the dual-read table names is read on the object count
# AND the region count in one drive, and its region verdict is recorded so the
# completeness gate can tell coverage from silence.
(defn run-direct-loop [classes]
  (each entry classes
    (let [label (get entry 0)
          probe (get entry 1)
          rpin (get dual-read label)]
      (if (nil? rpin)
        (pin (measure label probe 100 6 60 0.4 0.5) (get entry 2))
        (let [[r rr] (measure-2 label (fn [b] (run-thunk-block probe b))
                                count-gauge 0.4 0.5 "regions" region-gauge 0.4
                                0.5 100 6 60)]
          (put dual-read-seen label true)
          (pin r (get entry 2))
          (pin rr rpin))))))
