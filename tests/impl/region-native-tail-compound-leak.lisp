(elle/epoch 12)
# The return mint is emitted exactly once per returned value
# (docs/impl/region/mechanism.md § "The return mint is emitted exactly once").
#
# A function hands its caller ONE owning reference to its result, and the
# caller's single `DecrefValueRegion` consumes it. Two lowering sites can supply
# that mint, chosen by whether the result is NAMED:
#
#   * the `Return` mint (`lower_return`, marked by `hir/return_incref.rs`) — ANF
#     bound the tail value to a synthetic slot, so the mint raises RC and the
#     binding's `decref_point` drops the frame's own reference: net one;
#   * the `TailCall` fall-through retain (`lower_call`'s tail arm) — a NATIVE
#     tail call pushes no bytecode frame, so the post-`TailCall` block runs on
#     normal completion. In a propagating tail position (a `let`/lambda body,
#     which ANF leaves unnamed) there is no binding and no `decref_point`, so
#     this retain IS the mint.
#
# They cover the same value whenever ANF names a tail call's result — the wrap
# `(let [_t (/*tail*/ f …)] (return _t))` ANF builds for a tail call nested in a
# `begin`/`if`/`cond`/`match` arm (visible under `--dump=fhir`). Both firing
# retains the result twice against one release, so the fall-through retain
# stands down when a `Return` mint covers the result.
#
# This file pins the per-shape disposition as a LEAK gauge (`arena/region-count`
# delta over a fixed window, not a UAF): every shape below must be BOUNDED, over
# the placements (bare / `let`-body / `begin`-nested / `if`-nested), the callee
# kinds (opcode intrinsic, Fresh native, Funnel native, closure), and both
# funnel flavours (a FRESH immutable result and a `-mut` container
# pass-through). The pass-through rows are the sharp ones: their result is the
# caller's own container, so a surplus retain pins a region the caller can never
# drop. The soundness complement — the ANONYMOUS path must KEEP its retain — is
# `region-native-tail-return-uaf.lisp` / `region-hof-tail-return-uaf.lisp`.

(def window 2000)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-count) before))

(def s {:a 1})

# subjects ─────────────────────────────────────────────────────────────────────
# Placement × callee kind, over an IMMUTABLE struct (each funnel call returns a
# FRESH result, so the frame owns a value distinct from its argument).
(defn bare-pair [a b]
  (%pair a b))
# opcode, bare
(defn bare-put [c k v]
  (%put c k v))
# funnel, bare
(defn bare-del [c k]
  (%del c k))
# funnel, bare
(defn let-pair [a b]
  (let [x 5]
    (%pair a b)))
(defn mk [a b]
  (%pair a b))
(defn clo-if [a b]
  (if (%gt a 0) (mk a b) 0))
# closure tail in if
(defn begin-pair [a b]
  (begin
    0
    (%pair a b)))
(defn if-pair [a b]
  (if (%gt a 0) (%pair a b) 0))
(defn if-put [c k v]
  (if true (%put c k v) 0))
(defn if-del [c k]
  (if true (%del c k) 0))

# The `-mut` PASS-THROUGH rows: the funnel returns arg0 — the container the
# frame itself minted — so a surplus retain strands that container's region and
# everything it holds. Each is a `begin`-nested tail call (the ANF-named shape):
# the earlier statement is what pushes the tail call out of return position.
(defn begin-put-mut []
  (let [m @{}]
    (%put m :k 7)
    (%put m :k2 8)))
(defn begin-del-mut []
  (let [m @{}]
    (%put m :k (%pair 1 2))
    (%del m :k)))
(defn begin-push-mut []
  (let [a @[]]
    (%array-push a 7)
    (%array-push a 8)))
# A Fresh native (not a funnel) in the same ANF-named shape: the leak is keyed
# on the placement, not on the callee's `RegionEffect`.
(defn begin-fresh []
  (begin
    (list 1 2)
    (list 3 4)))

(def bare-pair-d (measure (fn () (bare-pair 1 2)) 200 window))
(def bare-put-d (measure (fn () (bare-put s :b 2)) 200 window))
(def bare-del-d (measure (fn () (bare-del s :a)) 200 window))
(def let-pair-d (measure (fn () (let-pair 1 2)) 200 window))
(def clo-if-d (measure (fn () (clo-if 1 2)) 200 window))
(def begin-pair-d (measure (fn () (begin-pair 1 2)) 200 window))
(def if-pair-d (measure (fn () (if-pair 1 2)) 200 window))
(def if-put-d (measure (fn () (if-put s :b 2)) 200 window))
(def if-del-d (measure (fn () (if-del s :a)) 200 window))
(def begin-put-mut-d (measure (fn () (begin-put-mut)) 200 window))
(def begin-del-mut-d (measure (fn () (begin-del-mut)) 200 window))
(def begin-push-mut-d (measure (fn () (begin-push-mut)) 200 window))
(def begin-fresh-d (measure (fn () (begin-fresh)) 200 window))

(println "region-native-tail-compound-leak deltas over " window " iters:")
(println "  bare %pair  : " bare-pair-d "   bare %put : " bare-put-d
         "   bare %del : " bare-del-d)
(println "  let  %pair  : " let-pair-d "   clo-in-if : " clo-if-d)
(println "  begin %pair : " begin-pair-d "   if %pair : " if-pair-d
         "   if %put : " if-put-d "   if %del : " if-del-d)
(println "  begin -mut  : put " begin-put-mut-d "  del " begin-del-mut-d
         "  push " begin-push-mut-d "   begin fresh-native : " begin-fresh-d)

# The window is 2000 iterations and every leak in this class is a whole region
# per call, so a surviving double-mint reads ~2000 (or ~4000 where the stranded
# container also strands a member). 100 is generous slack for the measurement's
# one-time intercept.
(defn bounded? [d label]
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

# Placement is irrelevant: bare, `let`-body, `begin`-nested and `if`-nested all
# mint exactly once, for an opcode intrinsic and for a closure tail call alike.
(bounded? bare-pair-d "bare %pair")
(bounded? let-pair-d "let-body %pair")
(bounded? clo-if-d "closure tail in if")
(bounded? begin-pair-d "begin-nested %pair")
(bounded? if-pair-d "if-nested %pair")

# Callee kind is irrelevant: a Funnel native's FRESH result is bounded bare and
# nested, and so is a plain Fresh native's.
(bounded? bare-put-d "bare %put")
(bounded? bare-del-d "bare %del")
(bounded? if-put-d "if-nested %put")
(bounded? if-del-d "if-nested %del")
(bounded? begin-fresh-d "begin-nested Fresh native")

# The `-mut` pass-through rows — the shape where the surplus retain pinned the
# caller's own container. `%del` strands two regions per call when it leaks (the
# container plus the heap member it removed), which is why its slack is the same
# absolute 100 rather than a fraction of the window.
(bounded? begin-put-mut-d "begin-nested %put -mut pass-through")
(bounded? begin-del-mut-d "begin-nested %del -mut pass-through")
(bounded? begin-push-mut-d "begin-nested %array-push -mut pass-through")

(println "region-native-tail-compound-leak: ok")
