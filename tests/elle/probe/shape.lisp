(elle/epoch 12)
# audited: 2026-09-08
# The shapes the direct-loop rows drive: values, error payloads, parked primitives, propagate depth, env and module cells, branch arms.
#
# docs/impl/region/diagnostics.md
# ── The folded leak suite ─────────────────────────────────────────────
# One dashboard covering every leak class (each declared a root below), on the estimator. The
# shapes need different DRIVERS — one run-block per shape, all feeding the one
# measure-core:
#   - direct-loop (the table below): a per-op thunk run b times;
#   - tail-call rotation: the recursive call itself is the run-block;
#   - fiber-internal yield: a fiber that runs b iterations then completes, drained
#     (a drained loop reclaims at scope-exit; a forever-generator never exits its
#     loop, so its per-iteration values would falsely read as leaks);
#   - persistent containers: the container is def'd fn-local in the run-block;
#   - discarded call-result / break-escape / match-scrutinee: a DIRECT
#     while-statement run-block (a thunk's return convention would reclaim the
#     over-keep the discarded-statement shape leaks);
#   - byte-gauge: the same drivers under arena/bytes;
#   - value-survival: plain asserts (correctness, not a rate).
#
# Each pin is the TRUE CURRENT rate the estimator measures, exact (or a
# [lo hi] range) and shrink-only: a fix LOWERS it, never raises it.

(defn make-struct [i]
  # `i` reaches the value position (:iter i), which disables call-site param
  # joins, so the %add operand is proven by a local coerce-guard instead
  # (docs/intrinsics.md § The contract). The coerce rebinds i to an int without a
  # branch-compensation retain, so the success path stays at 0/op.
  (let [i (if (%int? i) i 0)]
    {:iter i :val (%add i 1)}))
(defn make-label [i]
  (string "item-" i))
(defn t19-store [c v]
  (put c :x v))
# The `error-payload-helper` / `error-payload-param` raisers: each allocates or
# receives the payload in a frame the error exit WALKS (the raising body's own
# frame is parked instead — that face is `error-payload`).
(defn ep-raiser [j]
  (error (string "x" j)))
(defn ep-raise-param [v]
  (error v))
# The `primitive-resume-*` bodies park at a suspending PRIMITIVE call, whose
# resume value stands in for a result no `Return` mint ever funded, so the
# delivery mints it instead (docs/impl/region/owner.md § "A delivery into a
# replayed frame carries one owning reference"). What the rate gauges is the
# mint's ARITY: one reference per delivery, consumed by the continuation's own
# result release, so a second mint no release answers strands the resume value
# per park. `ora-dyn-sig` is what makes the park a primitive one — a non-literal
# first argument falls through to the runtime primitive, where the literal form
# compiles to the `Emit` terminator whose resume block mints in bytecode. That
# literal form is the control the three witnesses are read against: the same
# program, the same delivery, one path funded by the compiler instead.
(def ora-dyn-sig :yield)
(defn pr-bind [j]
  (let [f (fiber/new (fn []
                       (let [r (emit ora-dyn-sig 7)]
                         [:resumed r])) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "b" j))))
(defn pr-tail [j]
  (let [f (fiber/new (fn [] (emit ora-dyn-sig 7)) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "t" j))))
(defn pr-keep [j]
  (let [f (fiber/new (fn []
                       (let [r (emit ora-dyn-sig 0)]
                         (emit ora-dyn-sig (length r))
                         (first r))) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "k" j))
    (fiber/resume f)))
(defn pr-literal [j]
  (let [f (fiber/new (fn []
                       (let [r (emit :yield 7)]
                         [:resumed r])) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "l" j))))
# The `propagate-*` raisers. `fiber/propagate` installs the child's parked
# payload as the propagating fiber's own `signal`, which is a FRESH park and owes
# its own delivery reference — the one the propagating fiber's resumer consumes
# when it releases its resume result (docs/impl/region/owner.md § "Park/unpark
# symmetry"). `defer` is that propagate in production form: it resumes a body
# fiber, runs cleanup, then propagates when the body did not complete. So the
# rate is read across propagate DEPTH: a mint no release answers strands one
# region per park, which makes growth scale with the number of `defer`s the raise
# passes through, and `propagate-none` is the same raise with no propagate in it
# at all.
(defn pg-raise [n]
  (error {:reason :bang :tag (string "z" n)}))
(defn pg-none [j]
  (let [[ok? err] (protect (pg-raise j))]
    (length err:tag)))
(defn pg-one [j]
  (let [[ok? err] (protect (defer
                             nil
                             (pg-raise j)))]
    (length err:tag)))
(defn pg-three [j]
  (let [[ok? err] (protect (defer
                             nil
                             (defer
                               nil
                               (defer
                                 nil
                                 (pg-raise j)))))]
    (length err:tag)))
# The `env-cell-def-capture` / `env-cell-let-twin` pair drives a captured `def`
# inside a lambda, whose init is a CALL (a constant folds and allocates nothing to
# strand). Such a binding is env-celled, so its init's release is routed through
# the env index rather than the stack slot of the same number, and the pair splits
# on whether a cell exists at all: `def` is always mutable, hence always celled,
# while the `let` twin's immutable local is captured by value and never gets one.
# So the gap between them isolates the env route rather than the shape, and a
# release that never reaches the env strands the init on every execution.
(defn ec-increment [x]
  (+ x 1))
(defn ec-def-capture []
  (let [root "/x"]
    ((fn []
       (def joined (path/join root "a"))
       (let [reader (fn [] (list (string? joined) (ec-increment 1)))]
         (reader))))))
(defn ec-let-twin []
  (let [root "/x"]
    ((fn []
       (let [joined (path/join root "a")]
         (let [reader (fn [] (list (string? joined) (ec-increment 1)))]
           (reader)))))))
# `module-cell-read-window` is the closure-as-module: a lambda whose captured
# `def`s the returned struct's accessors read, and whose last form is that struct
# literal — a NATIVE tail call, so the block after the `TailCall` is reached and
# everything the lowerer put there runs. The captured binding's value and its env
# cell are two REGIONS and the frame-exit relocation answers per region, so the
# pair can be split: an `Immediate` native's result is named by no binding, so the
# frame-held admission refuses it for want of a holder and its release stays
# behind the call, while the cell is admitted on its binding's verdict and moves
# ahead. A move that crosses a read through the cell it frees is declined
# (docs/impl/region/mechanism.md § "A move that crosses a read through the cell it
# frees is declined"), and declining leaves the box release on the closure path —
# the bounded fallback whose cost this gauges. `ptr/from-int` is the `Immediate`
# init that splits the pair; `module-cell-heap-init` is the same module with a
# HEAP init, where the value region has a holder and both releases are admitted
# together, so the gap between them isolates the decline from the module shape.
(defn mod-cell-immediate []
  (def a (ptr/from-int 7))
  (def p (fn [] a))
  {:p p})
(defn mod-cell-heap []
  (def a (string "cap"))
  (def p (fn [] a))
  {:p p})
# The `fresh-env-cell` / `shared-env-cell` pair drives one env cell each, split on
# where the cell is minted. `c` is a captured, REASSIGNED local, so `populate_env`
# mints its cell box once per activation — a fresh region per call — and the frame
# ends in a closure tail call, which puts the box's `DecrefCellRegion` in the dead
# post-`TailCall` block. Relocating it there is the frame-held admission's
# business, and the holder's mutation does not refuse it: the release names the
# BOX, which no `assign` repoints (docs/impl/region/mechanism.md § "A mutated
# holder poisons its value route, not its cell box"). `shared-env-cell`'s cell is
# module-level, minted once for the file, so it measures the same closure call with
# no per-op box at all.
(defn t20-make-cell []
  (def @c 0)
  (let [f (fn []
            (assign c (%add c 1))
            c)]
    (f)))
# `env-cell-read-arm` drives the same box where a branch SIBLING arm reads the
# cell's binding. The capture-use of `c` resolves through `f`'s last use, so the
# box's `decref_point` follows the call into the arm that makes it, and the reading
# arm takes compensation's TAIL release — after its own read, where the head release
# would free the box under that read (docs/impl/region/mechanism.md § "A
# compensating release of an env cell names the box, not the holder's slot"). Driven
# through the reading arm, the only one whose release is new.
(defn t20-read-arm [t]
  (def @c 0)
  (let [f (fn [] c)]
    (if t c (f))))
# The two faces of per-arm compensation over a `Match`, driven through the arm the
# caller picks. `v` is allocated before the dispatch, so it is live-in on every arm
# and its lone `decref_point` lands in the arm that uses it last.
#   DEAD arm  — the taken arm has no use of `v` at all, so it creates no reference
#               and takes the head release (`regions::compensate`).
#   USED arm  — the taken arm uses `v` but is not the one holding the `decref_point`,
#               and no retain on its last-use node funds a per-arm release, so it
#               keeps the conservative baseline and strands `v` (F5).
(defn t21-dead-arm [t]
  (let [v (list 1 2 3)]
    (match t
      :use (length v)
      :skip 0
      _ -1)))
(defn t21-used-arm [t]
  (let [v (list 1 2 3)]
    (match t
      :a (length v)
      :b (length v)
      _ (length v))))
# The same arm structure over an OWNED PARAMETER rather than a fn-local — the
# polymorphic stdlib entry point's shape, whose caller moved the argument in. The
# region's one release is anchored where every arm reaches it, so the arm the
# caller happens to pick does not decide whether the argument is freed
# (docs/impl/region/mechanism.md § "A release inside one arm is not a release on
# the other arms"). `t22-param-if` is the `If` face of the identical premise:
# the window reads arm structure, never the branch's kind or arity.
(defn t22-param-arm [v t]
  (match t
    :a (length v)
    :b (length v)
    _ (length v)))
(defn t22-param-if [v c]
  (if c (length v) (%add 1 (length v))))
# The window's live-in premise is about the ALLOCATION: that is what "born in an
# arm" means, and what the release's route follows, since `region_to_slot` is
# keyed on a region's allocation site. A binding the arm introduces to ALIAS a
# live-in value records no slot of its own and so decides nothing. Driven through
# the arm that takes no alias, the one whose release is new.
(defn t22-arm-alias-inside [v t]
  (match t
    :a (length v)
    _ (let [w v]
        (length w))))
# The window's iterative boundary is the loop's BODY, not the loop's own node. A
# read of a loop-external binding is anchored at the loop NODE, and the lowerer
# emits a node's releases after it, so that release already runs once per execution
# of the loop — the count the merge label is reached with. Driven through the arm
# that does NOT loop, the one whose release is new.
# The sequence reads are read-only trait dispatchers and declare `Opaque`, so
# they seed nothing on escape's store facet (docs/impl/region/effects.md
# § `Opaque`). A `Mixed` declaration would, and every mechanism gated on
# `frame_held_regions` refuses a region escaping by a facet other than
# return — the branch-arm window among them, which is what this drives.
(defn t22-arm-seq-read [v t]
  (match t
    :a (first v)
    :b (length v)
    _ (length v)))
(defn t22-arm-loop-read [v t]
  (match t
    :a (length v)
    _
      (begin
        (var i 0)
        (while (%lt i 3)
          (get v i)
          (assign i (%add i 1)))
        (%add i 100))))
# The same window over a branch one of whose arms leaves through a frame-replacing
# CLOSURE tail call naming the same parameter — the `append`/`concat` dispatch
# shape. Anchoring is what covers the arm driven here; the frame-exiting arm is
# covered by the relocation's exemption, since its call took the argument over
# (docs/impl/region/mechanism.md § "An arm that leaves through a callee takes a
# replica, not the anchor").
(defn t22-tc-callee [v]
  (length v))
(defn t22-tailcall-sibling [v t]
  (match t
    :a (length v)
    :b (t22-tc-callee v)
    _ 0))
# The same window over a RETURNED parameter — `push-all`'s shape, and with it every
# `append`/`concat` over a byte-family source. The arm driven here hands `dst` back
# to the caller; the sibling arm leaves through a local walker that reaches `dst`
# only through its captured environment. The merge follows this arm's own mint, and
# the sibling's replica runs ahead of a callee whose captured edge holds the region
# off zero until its own mint, so the branch is admitted for the class
# (docs/impl/region/mechanism.md § "The return facet costs the merge nothing").
# Refusing the facet outright strands one whole accumulator per call on the arm
# driven here.
(defn t22-returned-captured [dst src]
  (if (%eq (type-of src) :string)
    (begin
      (push dst src)
      dst)
    (let [n (length src)]
      (letrec [go (fn (i)
                    (if (%lt i n)
                      (begin
                        (push dst (get src i))
                        (go (%add i 1)))
                      dst))]
        (go 0)))))
