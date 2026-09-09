(elle/epoch 12)
# audited: 2026-09-08
# The frame-exit relocation's shapes: what a frame-replacing tail call strands, and the exemptions and channels that reclaim it.
#
# docs/impl/region/diagnostics.md
# The frame-exit release (docs/impl/region/mechanism.md § "A release past a
# frame-replacing tail call is not a release"). `t23-unused`'s parameter is used
# nowhere, so its release is the unused-parameter fallback the lowerer emits at the
# end of the body — the block a CLOSURE callee never reaches — and escape clears it
# as frame-held. `t23-moved` is the exemption: its parameter IS the tail call's
# argument, so its release is the ownership move and must stay put.
(defn t23-sink []
  0)
(defn t23-unused [x]
  (t23-sink))
(defn t23-take [a]
  (length a))
(defn t23-moved [x]
  (t23-take x))
# `t23-arms` puts the same stranded release past a MERGE: both arms leave through
# a frame-replacing tail call, so the block the release lands in is reached on
# neither path. A branch merge inherits its arms' relocation points, so the
# release is replicated ahead of each arm's `TailCall` as well as emitted at the
# merge — sound because a value-routed release nil-stamps the slot it read, so
# the copy a path reaches second no-ops.
(defn t23-sink2 []
  1)
(defn t23-arms [x t]
  (if t (t23-sink) (t23-sink2)))
# `t23-or-arm` and `t23-and-arm` put the same strand past a SHORT-CIRCUIT merge.
# `and` and `or` carry no branch in the source and the lowerer gives them one:
# each operand stores its value into the result slot and every operand but the
# last branches on it, so the done block is a merge every operand's block reaches
# (docs/impl/region/replicate.md § "A short-circuit operand is an arm"). Only the
# LAST operand inherits tail position, so it is the only arm that can carry a
# frame-replacing tail call, and that one arm strands the whole release set the
# enclosing scope emits past the merge. Each is driven through its own arm: `or`
# reaches the last operand on a FALSE first operand, `and` on a true one.
(defn t23-or-arm [x t]
  (or t (t23-sink)))
(defn t23-and-arm [x t]
  (and t (t23-sink)))
# `t23-captured`'s parameter is reached by the tail callee through its CAPTURED
# environment — the path no argument names and no callee region describes. It is
# admitted all the same: the funnel counted the closure's hold when the env was
# built, so the relocated release drops the frame's own reference and leaves the
# callee's standing (docs/impl/region/mechanism.md § "Lexical capture is not a
# second holder to fear").
(defn t23-captured [x]
  (let [g (fn [] (length x))]
    (g)))
# `t23-handback` is the same capture carrying one step further: the tail callee
# RETURNS the parameter, so the caller's owning reference is minted inside the
# callee — after the relocated release has run. The env's counted edge is what
# holds the region off zero in between, and it falls away only with the closure
# region, at the callee's completion (docs/impl/region/mechanism.md § "The callee's
# return mint, and why the point owes it nothing"). This is the stdlib `push-all`
# shape, and the strand it carried is what held the `concat`/`append` family.
(defn t23-handback [dst src]
  (let [n (length src)]
    (letrec [go (fn [i]
                  (if (%lt i n)
                    (begin
                      (push dst (get src i))
                      (go (%add i 1)))
                    dst))]
      (go 0))))
(defn t23-drive-handback [src]
  (let [acc (@array)]
    (t23-handback acc src)
    (length acc)))
# `t23-fwd-cell`'s `go` reaches its sibling `helper` through a prebound FORWARD
# CELL, and `helper` does not call back — a one-way sibling capture, so there is no
# SCC and the closure-cycle merge never sees the cell. Its binding-scope
# `DecrefRegion` lands in the dead block like any other of the frame's releases, and
# the count argument reaches it through its BINDING's verdict: a binding names the
# closure region its cell points at, never the cell's own, so an admission read over
# holder bindings alone cannot see the cell at all. Stranding the cell strands the
# sibling with it, the cell's reference being what holds that closure off zero
# (docs/impl/region/mechanism.md § "A compiled capture cell is frame-held exactly as
# its binding is"). `t23-fwd-cell-ret` is the RETURN face of the same projection:
# the capturer is handed back, so what keeps the cell alive across the relocated
# release is the counted `closure ⊇ cell` edge, and the cell's own region has to
# carry its binding's verdict for the projection to name it at all.
(defn t23-fwd-cell [n]
  (letrec [helper (fn [x] (%sub x 1))
           go (fn [m] (helper m))]
    (go n)))
(defn t23-fwd-cell-ret [n]
  (letrec [helper (fn [x]
                    (when (%not (%int? x)) (error :x))
                    (%sub x 1))
           go (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) go (go (helper m))))]
    (go n)))
# `t23-fwd-cell-sib` inverts `t23-fwd-cell`: the SIBLING captures the self-recursive
# member, and the body tail-calls the sibling. That one `TailCall` strands two
# regions on two independent channels — the merged arena's `deferred_release_slot`
# (`go`'s closure, its env, and the forward cell the single-closure self-edge
# admission collapsed into it) and the sibling's own `defer_callee_release`. RED if
# the runtime reads them as alternatives again, which reclaims neither: the
# sibling's counted `closure ⊇ cell` edge holds the arena off zero until the
# sibling's own region goes (docs/impl/region/letrec.md § "The arena channel and the
# callee channel are independent").
(defn t23-fwd-cell-sib [n]
  (letrec [go (fn [m] (if (%lt m 1) :done (go (%sub m 1))))
           outer (fn [m] (go m))]
    (outer n)))
# `t23-operand-value`'s tail call names `go` NOWHERE — its ARGUMENT calls `go`, so
# what the callee is handed is that call's result and `go`'s own closure region was
# read and finished with before the call was made. The exemption reads an operand's
# VALUE rather than its syntax, so the region an argument's own nested call merely
# used is not exempt and its scope-end release relocates like any other
# (docs/impl/region/mechanism.md § "What an operand names is its VALUE, not its
# syntax"). RED if the exemption widens back to the syntax walk, which strands one
# closure per call for every stdlib helper whose tail call consumes a local walker's
# result.
(defn t23-operand-value [n]
  (letrec [helper (fn [x] (%sub x 1))
           go (fn [m] (if (%lt m 1) 0 (go (%sub m 1))))]
    (helper (go n))))
# `t23-callee-member` swaps `t23-operand-value`'s capturer for a plain one, so the
# body tail-calls the CAPTURED sibling: `helper` is allocated per call and its uses
# span the letrec, which puts its demise at the letrec's scope end rather than at
# the call node. Its own region is exempt from the relocation because the new
# activation takes the release over, so the deferral has to reach a release placed
# at that scope end and not only one demising at the call node
# (docs/impl/region/mechanism.md § "What the exemption keeps, a channel must still
# run"). RED if the deferral narrows back to the call node, which strands one
# closure plus its environment per call for every helper pair whose body dispatches
# the captured member.
(defn t23-callee-member [n]
  (letrec [helper (fn [x] (%sub x 1))
           go (fn [m] (helper m))]
    (helper (go n))))
# `t23-fold-drive` is the index-walk fold driver every stdlib `fold`/`reduce`/
# `concat` walks with. Its base arm returns the accumulator, so the region is on the
# return frontier; its recursive arm hands the tail callee the COMBINER's result
# rather than the accumulator itself, so no route reaches the accumulator at that
# point. A callee reaches a value this frame owns as an operand or through its
# captured environment and by no other route, so one it reaches by neither cannot
# mint against the region and the relocated release is the last
# (docs/impl/region/mechanism.md § "The callee's return mint, and why the point owes
# it nothing"). RED if the admission narrows back to a per-point funding edge, which
# strands one displaced accumulator per fold step.
(defn t23-fold-step [f n i acc]
  (if (%lt i n) (t23-fold-step f n (%add i 1) (f acc i)) acc))
(defn t23-fold-drive [n]
  (length (t23-fold-step (fn [a b] (@array)) n 0 (@array))))
