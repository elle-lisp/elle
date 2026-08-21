(elle/epoch 12)
# Counterfactual: a function whose body feeds a `(get <owned-heap-param> idx)`
# result into a RETURNED combining expression (e.g. `(+ … …)`) LEAKS the owned
# heap params' regions — they are never freed. Pure interpreter bug, no FFI, no
# JIT (reproduces identically on every tier).
#
# Bisected behaviour (over a fixed iteration window, `arena/region-count` delta):
#   LEAKS ~2 regions/iter (BOTH arg regions):
#     (fn (a b) (+ (get a 0) (get b 0)))      # two gets, combined + returned
#     (fn (a b) (+ (get a 0) (length b)))     # ONE get is enough — leaks BOTH
#     (fn (a b) (+ (length a) (get b 0)))
#   BOUNDED (no leak):
#     (fn (a b) (get a 0))                     # single get result returned directly
#     (fn (a b) (do (get a 0) (get b 0) 0))    # get results discarded, return const
#     (fn (a b) (+ (length a) (length b)))     # length instead of get
# So the trigger is a `get` result flowing as an operand into a returned combining
# expression while ≥2 owned heap params are live; the leak then strands BOTH
# params' regions (not just the get'd one). Both immutable `[…]` and mutable
# `@[…]` aggregates trigger it.
#
# HYPOTHESIS for the fixer (verify with `--trace=rc`, do not assume): `get` does
# the Rule-5 native-result pass-through retain (IncrefValueRegion on the result's
# region). When the get result is returned directly or discarded, the matching
# DecrefValueRegion balances it. But when it is consumed as an operand of an
# arithmetic combiner (`+`) that treats it as an immediate and the activation
# returns the combination, the pass-through retain is left unbalanced and the
# owned-param value-based releases do not fire — stranding the regions. Likely in
# the owned-params / `call_result_regions` / `cell_release_regions` interaction
# (src/hir/regions.rs) or the `DecrefValueRegion` placement for an operand whose
# producer is a pass-through native.
#
# A LEAK, not a UAF — the witness is an `arena/region-count` delta, not a crash.
# RED now on every tier; GREEN once the pass-through retain is balanced (or the
# owned-param releases fire) so a returned get-combination is bounded.

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

# ── subjects ──────────────────────────────────────────────────────
(defn two-get (a b)
  (+ (get a 0) (get b 0)))
# WITNESS
(defn one-get (a b)
  (get a 0))
# control: returned directly
(defn discard-get (a b)
  (do
    (get a 0)
    (get b 0)
    0))
# control: discarded
(defn two-len (a b)
  (+ (length a) (length b)))
# control: length, not get

# ── controls: bounded NOW (these bisect the trigger) ───────────────
(def one-imm (measure (fn () (one-get [7 0] [9 0])) 100 2000))
(def discard-imm (measure (fn () (discard-get [7 0] [9 0])) 100 2000))
(def len-imm (measure (fn () (two-len [7 0] [9 0])) 100 2000))
(assert (%lt one-imm 100)
        (concat "control: single returned get leaks, delta="
                (number->string one-imm)))
(assert (%lt discard-imm 100)
        (concat "control: discarded gets leak, delta="
                (number->string discard-imm)))
(assert (%lt len-imm 100)
        (concat "control: length combine leaks, delta=" (number->string len-imm)))

# ── witnesses: a returned get-combination must not strand owned-arg regions ──
(def two-imm (measure (fn () (two-get [7 0] [9 0])) 100 2000))
(def two-mut (measure (fn () (two-get @[7 0] @[9 0])) 100 2000))
(println "region-get-owned-arg-leak deltas over 2000 iters:")
(println "  immutable [..]: " two-imm)
(println "  mutable  @[..]: " two-mut)
(assert (%lt two-imm 100)
        (concat "(+ (get a 0) (get b 0)) over immutable arrays leaks owned-arg "
                "regions, delta=" (number->string two-imm)))
(assert (%lt two-mut 100)
        (concat "(+ (get a 0) (get b 0)) over mutable arrays leaks owned-arg "
                "regions, delta=" (number->string two-mut)))

(println "region-get-owned-arg-leak: ok")
