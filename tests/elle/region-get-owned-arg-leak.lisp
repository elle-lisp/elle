(elle/epoch 12)
# audited: 2026-09-23
# A get result combined into a returned expression releases the regions of the owned heap parameters it read.
# docs/impl/region/rules.md
#
# `get` takes the Rule 5 pass-through retain on its result. When that result is
# an operand of an arithmetic combiner (`+`) whose sum the function returns, the
# retain still needs its matching release, and the owned parameters still need
# theirs. Counterfactual: a lowerer that leaves the retain unbalanced there
# strands BOTH owned parameters' regions, about two regions per call, on every
# tier. One `get` in the combination is enough, and immutable `[…]` and mutable
# `@[…]` aggregates both show it.
#
# The controls separate the trigger: a `get` result returned directly, `get`
# results discarded, and `length` in place of `get` stay bounded whether or not
# the combination leaks.
#
# A LEAK, not a UAF — the witness is an `arena/region-count` delta, not a crash.

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
# WITNESS
(defn two-get (a b)
  (+ (get a 0) (get b 0)))

# control: returned directly
(defn one-get (a b)
  (get a 0))

# control: discarded
(defn discard-get (a b)
  (do
    (get a 0)
    (get b 0)
    0))

# control: length, not get
(defn two-len (a b)
  (+ (length a) (length b)))

# ── controls ───────────────────────────────────────────────────────
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
