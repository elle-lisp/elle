(elle/epoch 15)
# audited: 2026-11-17
# Discarded call results and break escapes, the branch-arm release window in each of its faces, and the frame-exit rows.
#
# docs/impl/region/diagnostics.md
# ── Discarded call-result + break-escape ──────────────────────────────
# Direct while-statement run-blocks, with no thunk wrapper. The discarded value
# is a CALL-RESULT (Rule 2's discarded-result release). A thunk's return
# convention would reclaim the break-escape on its own, and hide what the break
# probes below measure: the block, not the enclosing call, must anchor a release
# the break's jump passes over.
#
# Each struct these rows read comes from `t17-h` or `t17-h2`, which build it
# from the op's index (probe/factory.lisp). The allocation rows hold each probe
# to building it.
(println "── folded suite: call-result + break ──")
(r:drive "branch-call"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             # %rem (not the mod wrapper): j is a proven local int
             # and the wrapper's result would be untyped
             (if (%lt (%rem j 2) 1) (t17-h j) (t17-h2 j))
             (assign j (%add j 1)))) :on [r:objects r:allocs])
# The raw `%array-push`/`%put` into a fresh container, discarded: the CONTROL for
# the stdlib `put`/`push` type-dispatch WRAPPER. The raw intrinsic reclaims the
# container in BOTH intrinsics modes (rate 0), so a rate `put-churn` shows below
# rides the wrapper, not the store funnel. Direct while-statements (a thunk
# wrapper's return convention would inflate the rate by 1).
(r:drive "push-slot-source"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (let [items @[]]
               (%array-push items (%pair 1 2)))
             (assign j (%add j 1)))))
(r:drive "put-slot-source"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (let [s @{}]
               (%put s :k (%pair 1 2)))
             (assign j (%add j 1)))))
# The raw `%add-set-mut` into a fresh @set, discarded — the set-family CONTROL,
# the peer of push-slot-source/put-slot-source. The raw silent intrinsic reclaims
# the container (rate 0), so a rate `set-add` (probe/store.lisp) shows rides the
# stdlib `add` type-dispatch WRAPPER, not the set-add funnel (`set_add_with_incref`).
(r:drive "set-add-slot-source"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (let [s @||]
               (%add-set-mut s (%pair 1 2)))
             (assign j (%add j 1)))))
# put-churn mints a FRESH @struct container per op and hands it through the stdlib
# `put`; its `:@struct` arm's `%put-struct-mut` returns the container pass-through,
# and the wrapper's per-arm container release (`container_release_sites`,
# src/hir/region/infer/compensate.rs) frees the stranded owned-param reference,
# cascading the stored struct — rate 0 in both intrinsics modes, every tier. A
# CLOSED control beside `put-slot-source`; it opens if the container compensation
# regresses.
(r:drive "put-churn"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (let [s @{}]
               (put s :k {:v j}))
             (assign j (%add j 1)))))
# Per-arm compensation over a `Match`, both faces. `match-dead-arm` is a CLOSED
# control: the taken arm has no use of the pre-allocated local, so the head release
# frees it (docs/impl/region/compensate.md — the premises are stated over arms, so
# the branch's arity and kind are not read). `match-used-arm` is the USED face,
# also CLOSED. The taken arm uses the local but does not hold its `decref_point`,
# and no retain on its last-use node funds a per-arm release. So the region's
# single release is anchored where every arm reaches it
# (docs/impl/region/window.md).
#
# Widening `tail` to every arm-last-use node over-frees, because an arm that
# used the region may hold an uncounted borrow the solver does not name. So the
# window is a placement argument, not a count one.
(r:drive "match-dead-arm"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t21-dead-arm :skip)
             (assign j (%add j 1)))))
(r:drive "match-used-arm"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t21-used-arm :a)
             (assign j (%add j 1)))))
# The owned-parameter face of the same branch structure, and its `If` twin. Both
# are CLOSED controls for the branch-arm release window. The argument's whole
# region (3 cons cells) strands on every arm but the one naming it last, unless
# the single release is anchored where every arm reaches it. Their
# counterfactual and the window boundaries are
# `tests/impl/region-branch-arm-window.lisp` and
# `tests/impl/region-branch-arm-tailcall.lisp`; the soundness complement is
# `region-branch-arm-window-uaf.lisp`.
#
# `branch-arm-tailcall-sibling` is the third: the same window over a branch whose
# OTHER arm leaves through a frame-replacing closure tail call. Declining such a
# branch whole strands the argument on the arm driven here, once per call of the
# `concat`/`append` family. `arm-alias-inside` is the fourth: one arm binds an
# ALIAS of the argument. The live-in premise is about the allocation, so a
# binding the arm introduces is not a birth in the arm. Reading the two kinds of
# anchor as one strands the argument on every other arm.
#
# `arm-seq-read` is the fifth: one arm READS the argument through a sequence
# read. Those reads declare `Opaque`, so they seed nothing on escape's store
# facet and the window admits the argument. A `Mixed` declaration seeds one and
# holds the whole branch to the baseline.
#
# `arm-loop-read` is the sixth: one arm LOOPS over the argument. Its release is
# anchored at the loop node, which the lowerer emits after the loop. So the
# boundary that declines a loop is the loop's BODY, and this class is admitted.
# Reading the boundary as the closed subtree interval strands the argument on
# every arm but the looping one.
(r:drive "param-used-arm"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t22-param-arm (list 1 2 3) :a)
             (assign j (%add j 1)))))
(r:drive "param-used-arm-if"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t22-param-if (list 1 2 3) true)
             (assign j (%add j 1)))))
(r:drive "arm-alias-inside"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t22-arm-alias-inside (list 1 2 3) :a)
             (assign j (%add j 1)))))
(r:drive "arm-seq-read"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t22-arm-seq-read (list 1 2 3) :a)
             (assign j (%add j 1)))))
(r:drive "arm-loop-read"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t22-arm-loop-read (list 1 2 3) :a)
             (assign j (%add j 1)))))
(r:drive "branch-arm-tailcall-sibling"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t22-tailcall-sibling (list 1 2 3) :a)
             (assign j (%add j 1)))))
(r:drive "branch-arm-return-captured"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t22-returned-captured (@string) "xy")
             (assign j (%add j 1)))))
# The scope-map face of the same `Match`. The arm READS a name its pattern bound,
# a borrowing read of the SCRUTINEE (rules.md Rule 4), and that read places a
# whole fresh struct's release. A pattern whose scope goes
# unrecorded reads as bound outside this loop, hoisting that release past the loop
# and stranding every iteration's scrutinee but the last
# (docs/impl/region/anchors.md). A control at 0. The
# per-shape rows and the arm-not-taken / guard / nested-loop faces are
# `tests/impl/region-match-bind-loop.lisp`; the soundness complement is
# `region-match-bind-loop-uaf.lisp`.
(r:drive "struct-match"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (match {:type :a :v j}
               {:type :a :v v} v
               _ 0)
             (assign j (%add j 1)))))
# The frame-exit release, thirteen CLOSED controls. Everything the lowerer emits
# after a frame-replacing tail call runs only on the NATIVE fall-through, so a
# release landing there may never run. The relocation moves that one release
# ahead of the `TailCall`. It is admitted where escape proves the frame holds
# the region alone, since on the closure path it adds a release the dead block
# never ran (docs/impl/region/relocate.md).
#
# `tail-frame-exit-unused` is the unused-parameter fallback through that dead
# block. `tail-frame-exit-arms` is the same strand one block further out: the
# tail calls sit in the arms of a branch, and the release lands past the merge.
# `tail-frame-exit-captured` is the holder the tail callee reaches through its
# CAPTURED environment, admitted because the funnel counted that hold.
# `tail-frame-exit-handback` carries that edge one step further. The callee
# RETURNS the parameter, so the caller's reference is minted inside it after the
# relocated release ran, and the env edge holds the region off zero in between.
#
# `tail-frame-exit-moved` is the exemption face, already 0. It reads GROWTH if
# the relocation ever releases an argument the callee now owns. The two
# `tail-frame-exit-fwd-cell*` probes are the region no holder binding NAMES: a
# prebound forward cell, which carries its binding's verdict one indirection
# out, for a frame-local capturer and for a returned one in turn.
# `tail-frame-exit-fwd-cell-sib` is the inversion where the sibling captures the
# member. One tail call then strands a merged arena AND its callee, on two
# channels that neither stand in for each other nor name the same region.
#
# `tail-frame-exit-operand-value` reads what the call itself names. A region the
# tail call reaches through no operand's VALUE, only through an argument's own
# nested call, is not exempt. `tail-frame-exit-callee-member` is the other side
# of the exemption: what it keeps in the dead block, a channel must still run.
# So a tail callee whose release the enclosing letrec places at its SCOPE END
# rides the deferral as one demising at the call node does.
# `tail-frame-exit-fold-driver` is the other end of the hand-back's enumeration.
# The tail callee reaches its returned accumulator through NEITHER route, so its
# `Return` mints nothing against the region and the relocated release is the
# last.
#
# `tail-frame-exit-or-arm` and `tail-frame-exit-and-arm` are the SHORT-CIRCUIT
# face of the merge, a strand of their own. An `and`/`or` done block is a merge
# the lowerer builds out of operands the source spells as one expression. So the
# arms are found by a different route, and only the last operand can carry the
# tail call (docs/impl/region/replicate.md). `tail-frame-exit-arms` drives an
# `if`, so it reads 0 whether or not the short-circuit face strands.
#
# The pair must stay together. `and`'s branch is `or`'s mirror: the first
# operand settles on FALSE rather than on true. So the two reach the same arm
# through opposite conditions, and a reading that recovered one polarity alone
# would close one of them. The counterfactual and the boundary rows live in
# `tests/impl/region-tail-frame-exit.lisp` and its `-capture` and `-letrec`
# siblings; the soundness complement is `region-tail-frame-exit-uaf.lisp`.
(r:drive "tail-frame-exit-unused"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-unused (list 1 2 3))
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-arms"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-arms (list 1 2 3) true)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-captured"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-captured (list 1 2 3))
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-handback"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-drive-handback (list 1 2 3))
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-moved"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-moved (list 1 2 3))
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-fwd-cell"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-fwd-cell 3)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-fwd-cell-ret"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-fwd-cell-ret 3)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-fwd-cell-sib"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-fwd-cell-sib 3)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-operand-value"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-operand-value 3)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-callee-member"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-callee-member 3)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-fold-driver"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-fold-drive 3)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-or-arm"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-or-arm (list 1 2 3) false)
             (assign j (%add j 1)))))
(r:drive "tail-frame-exit-and-arm"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (t23-and-arm (list 1 2 3) true)
             (assign j (%add j 1)))))
# The three `break-value*` probes are CLOSED controls for the break TRANSFER
# (docs/impl/region/anchors.md). The value a `break` carries out is the BLOCK's
# value, so its release is anchored where the block's value is consumed, not
# inside the body the break jumps out of. For a discarded block that is the
# block node itself, emitted after the exit label and reached on both paths. Discarded, consumed, and heap-literal
# placements all reclaim; they open if the transfer regresses.
(r:drive "break-value"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (block (let [x (t17-h j)]
                      (break x)))
             (assign j (%add j 1)))) :on [r:objects r:allocs])
(r:drive "break-value-used"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (let [r (block (let [x (t17-h j)]
                              (break x)))]
               (get r :a))
             (assign j (%add j 1)))) :on [r:objects r:allocs])
(r:drive "break-value-lit"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (block (let [x {:a j}]
                      (break x)))
             (assign j (%add j 1)))))
# The OTHER face of the break window, also CLOSED: a region whose value is NOT
# the one broken out, but whose `decref_point` sits between the break site and
# the block's exit label. The transfer does not reach it — the release is simply
# jumped over — so it is re-anchored to the block by the same pin
# (docs/impl/region/anchors.md). Its control `break-skipped-nobreak` runs the same
# body with the break unreachable, isolating the skip from the shape; it opens if
# the window pin regresses. The three boundaries the window stops at — a loop or
# a lambda nested inside it, and a frame-replacing tail call on the block's
# fall-through — are gauged by tests/impl/region-break-skip.lisp, not here.
(r:drive "break-skipped"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (block (let [x (t17-h j)]
                      (when (%lt -1 j) (break 1))
                      (%struct? x)))
             (assign j (%add j 1)))) :on [r:objects r:allocs])
(r:drive "break-skipped-nobreak"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (block (let [x (t17-h j)]
                      (when (%lt j -1) (break 1))
                      (%struct? x)))
             (assign j (%add j 1)))) :on [r:objects r:allocs])
