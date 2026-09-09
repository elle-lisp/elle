(elle/epoch 12)
# audited: 2026-09-08
# Discarded call results and break escapes, the branch-arm release window in each of its faces, and the frame-exit rows.
#
# docs/impl/region/diagnostics.md
# ── Discarded call-result + break-escape ──────────────────────────────
# Direct while-statement run-blocks (no thunk wrapper): the discarded value is a
# CALL-RESULT (Rule 2's discarded-result release), and a thunk's return
# convention would reclaim the break-escape on its own, hiding what the break
# probes below are here to measure — the block, not the enclosing call, is what
# must anchor a release the break's jump passes over.
(println "── folded suite: call-result + break ──")
(pin (measure-core "branch-call"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       # %rem (not the mod wrapper): j is a proven local int
                       # and the wrapper's result would be untyped
                       (if (%lt (%rem j 2) 1) (t17-h) (t17-h2))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The raw `%array-push`/`%put` into a fresh container, discarded: the CONTROL for
# F1b — the dispatch-wrapper passthrough leak. The raw intrinsic
# reclaims the container in BOTH intrinsics modes (rate 0), so the over-keep
# `put-churn` shows below (2/op) rides the stdlib `put`/`push` type-dispatch WRAPPER,
# not the store funnel. Direct while-statements (a thunk wrapper's return convention
# would inflate the rate by 1).
(pin (measure-core "push-slot-source"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [items @[]]
                         (%array-push items (%pair 1 2)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "put-slot-source"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [s @{}]
                         (%put s :k (%pair 1 2)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The raw `%add-set-mut` into a fresh @set, discarded — the set-family CONTROL
# for F1b, the peer of push-slot-source/put-slot-source. The raw silent intrinsic
# reclaims the container (rate 0), so the `set-add` over-keep (3/op) rides the
# stdlib `add` type-dispatch WRAPPER, not the set-add funnel (`set_add_with_incref`).
(pin (measure-core "set-add-slot-source"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [s @||]
                         (%add-set-mut s (%pair 1 2)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# put-churn mints a FRESH @struct container per op and hands it through the stdlib
# `put`; its `:@struct` arm's `%put-struct-mut` returns the container pass-through,
# and the wrapper's per-arm container release (`regions::compensate`,
# `funnel_container_sites`) frees the stranded owned-param reference, cascading the
# stored struct — rate 0 in both intrinsics modes, every tier. A CLOSED control
# beside `put-slot-source`; RED if the container compensation regresses.
(pin (measure-core "put-churn"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [s @{}]
                         (put s :k {:v j}))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# Per-arm compensation over a `Match`, both faces. `match-dead-arm` is a CLOSED
# control: the taken arm has no use of the pre-allocated local, so the head release
# frees it (docs/impl/region/mechanism.md § "The return frontier is per-path" — the
# premises are stated over arms, so the branch's arity and kind are not read).
# `match-used-arm` is the USED face, also CLOSED: the taken arm uses the local but
# does not hold its `decref_point`, and no retain on its last-use node funds a
# per-arm release — so instead of adding one, the region's single release is
# anchored where every arm reaches it (§ "A release inside one arm is not a
# release on the other arms"). Widening `tail` to every arm-last-use node is a
# measured over-free and is NOT what closed this: an arm that used the region may
# hold an uncounted borrow the solver does not name, which is exactly why the
# close is a placement argument and not a count one.
(pin (measure-core "match-dead-arm"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t21-dead-arm :skip)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "match-used-arm"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t21-used-arm :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The owned-parameter face of the same branch structure, and its `If` twin. Both
# are CLOSED controls for the branch-arm release window: the argument's whole
# region (3 cons cells) strands on every arm that is not the one naming it last,
# unless the single release is anchored where every arm reaches it. Undeclared,
# like `rest-array-copy`, so a regression trips the completeness gate loudly
# rather than being absorbed as an F5 strand. Their counterfactual and the two
# window boundaries are `tests/elle/region-branch-arm-window.lisp`; the soundness
# complement is `region-branch-arm-window-uaf.lisp`.
# `branch-arm-tailcall-sibling` is the third: the same window over a branch whose
# OTHER arm leaves through a frame-replacing closure tail call. Declining such a
# branch whole strands the argument on the arm driven here, which is what the
# `concat`/`append` family paid per call.
# `arm-alias-inside` is the fourth: the same window over a branch one of whose
# arms binds an ALIAS of the argument. The live-in premise is about the
# allocation, so a binding the arm introduces is not a birth in the arm; reading
# the two kinds of anchor as one strands the argument on every other arm.
# `arm-seq-read` is the fifth: the same window over a branch one of whose arms
# READS the argument through a sequence read. Those reads declare `Opaque`, so
# they seed nothing on escape's store facet and the window admits the argument;
# a `Mixed` declaration seeds one and holds the whole branch to the baseline.
# `arm-loop-read` is the sixth: the same window over a branch one of whose arms
# LOOPS over the argument. Its release is anchored at the loop node, which the
# lowerer emits after the loop, so the boundary that declines a loop is the loop's
# BODY and this class is admitted; reading the boundary as the closed subtree
# interval strands the argument on every arm but the looping one.
(pin (measure-core "param-used-arm"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-param-arm (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "param-used-arm-if"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-param-if (list 1 2 3) true)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "arm-alias-inside"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-arm-alias-inside (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "arm-seq-read"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-arm-seq-read (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "arm-loop-read"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-arm-loop-read (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "branch-arm-tailcall-sibling"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-tailcall-sibling (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "branch-arm-return-captured"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-returned-captured (@string) "xy")
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The scope-map face of the same `Match`: the arm READS a name its pattern bound,
# and that read is a borrowing read of the SCRUTINEE (rules.md Rule 4), so it is
# what places a whole fresh struct's release. A pattern whose scope goes
# unrecorded reads as bound outside this loop, hoisting that release past the loop
# and stranding every iteration's scrutinee but the last
# (docs/impl/region/mechanism.md § "Every binder records its scope"). CLOSED
# control — undeclared, like `param-used-arm`, so a regression trips the
# completeness gate loudly rather than being absorbed as an F5 strand. The
# per-shape rows and the arm-not-taken / guard / nested-loop faces are
# `tests/elle/region-match-bind-loop.lisp`; the soundness complement is
# `region-match-bind-loop-uaf.lisp`.
(pin (measure-core "struct-match"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (match {:type :a :v j}
                         {:type :a :v v} v
                         _ 0)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The frame-exit release, eleven CLOSED controls. A frame-replacing tail call means
# everything the lowerer emits after it runs only on the NATIVE fall-through, so a
# release landing there is emitted where control may never arrive; the close moves
# that one release ahead of the `TailCall` — admitted where escape proves the frame
# holds the region alone, since on the closure path it fires where none fired
# before (docs/impl/region/mechanism.md § "A release past a frame-replacing tail
# call is not a release"). `tail-frame-exit-unused` is the unused-parameter
# fallback through that dead block; `tail-frame-exit-arms` is the same strand one
# block further out, where the tail calls sit in the arms of a branch and the
# release lands past the merge; `tail-frame-exit-captured` is the holder the tail
# callee reaches through its CAPTURED environment, admitted because the funnel
# counted that hold; `tail-frame-exit-handback` is that same edge carrying one step
# further — the callee RETURNS the parameter, so the caller's reference is minted
# inside it, after the relocated release has run, and the env edge is what holds
# the region off zero in between; `tail-frame-exit-moved` is the exemption face,
# already 0, which reads GROWTH if the hoist ever releases an argument the callee
# now owns. The two `tail-frame-exit-fwd-cell*` probes are the region no holder
# binding NAMES — a prebound forward cell, which carries its binding's verdict one
# indirection out, for a frame-local capturer and for a returned one in turn;
# `tail-frame-exit-fwd-cell-sib` is the inversion where the sibling captures the
# member, so one tail call strands a merged arena AND its callee, on two channels
# that neither substitute for one another nor name the same region.
# `tail-frame-exit-operand-value` is the reading of what the call itself names: a
# region the tail call reaches through no operand's VALUE, only through an argument's
# own nested call, is not exempt. `tail-frame-exit-callee-member` is the other side
# of the exemption: what it keeps in the dead block, a channel must still run, so a
# tail callee whose release the enclosing letrec places at its SCOPE END rides the
# deferral exactly as one demising at the call node does.
# `tail-frame-exit-fold-driver` is the other end of the hand-back's enumeration: a
# returned accumulator the tail callee reaches through NEITHER route, so its
# `Return` mints nothing against the region and the relocated release is the last.
# Undeclared, like `param-used-arm`, so a regression trips the
# completeness gate loudly rather than being absorbed as F1a scratch. The
# counterfactual and the boundary rows live in
# `tests/elle/region-tail-frame-exit.lisp`; the soundness complement is
# `region-tail-frame-exit-uaf.lisp`.
(pin (measure-core "tail-frame-exit-unused"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-unused (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-arms"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-arms (list 1 2 3) true)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-captured"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-captured (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-handback"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-drive-handback (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-moved"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-moved (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fwd-cell"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fwd-cell 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fwd-cell-ret"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fwd-cell-ret 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fwd-cell-sib"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fwd-cell-sib 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-operand-value"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-operand-value 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-callee-member"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-callee-member 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fold-driver"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fold-drive 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The three `break-value*` probes are CLOSED controls for the break TRANSFER
# (docs/impl/region/mechanism.md § "`break` transfers its value"): the value a
# `break` carries out is the BLOCK's value, so its release is anchored where the
# block's value is consumed — for a discarded block that is the block node
# itself, emitted after the exit label and reached on both paths — instead of
# inside the body the break jumps out of. Discarded, consumed, and heap-literal
# placements all reclaim; RED if the transfer regresses.
(pin (measure-core "break-value"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x (t17-h)]
                                (break x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "break-value-used"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [r (block (let [x (t17-h)]
                                        (break x)))]
                         (get r :a))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "break-value-lit"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x {:a j}]
                                (break x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The OTHER face of the break window, also CLOSED: a region whose value is NOT
# the one broken out, but whose `decref_point` sits between the break site and
# the block's exit label. The transfer does not reach it — the release is simply
# jumped over — so it is re-anchored to the block by the same pin
# (docs/impl/region/mechanism.md § "A release the break jumps over is not a
# release"). Its control `break-skipped-nobreak` runs the same body with the
# break unreachable, isolating the skip from the shape; RED if the window pin
# regresses. Both boundaries the window stops at — a loop or a lambda nested
# inside it — are gauged by tests/elle/region-break-skip.lisp, not here.
(pin (measure-core "break-skipped"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x (t17-h)]
                                (when (%lt -1 j) (break 1))
                                (%struct? x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "break-skipped-nobreak"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x (t17-h)]
                                (when (%lt j -1) (break 1))
                                (%struct? x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
