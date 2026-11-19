(elle/epoch 15)
# audited: 2026-11-18
# A compiled prologue builds the rest list and each capture cell, and hot JIT-to-JIT calls through it answer as the interpreter does.
# docs/impl/region/rules.md
# docs/impl/jit-closure.md
#
# A compiled function's prologue builds the env values the interpreter's
# `populate_env` builds: the rest list of a variadic, and a capture cell for a
# parameter or a local that a nested closure captures and mutates. Each gets a
# fresh region of its own (`elle_jit_collect_rest_list`,
# `elle_jit_make_capture_owned`). The Rust tests
# `prologue_capture_cell_gets_its_own_region_not_callers` and
# `prologue_rest_list_conses_get_own_regions_not_callers`
# (src/jit/data/tests.rs) pin the regions; no Elle-level count isolates them,
# because a rest list outlives its last use until its activation ends on both
# tiers alike.
#
# This file pins the answers. A compiled driver calls each subject in a hot
# loop, so the subjects' prologues run JIT-to-JIT, under the driver's
# activation. A prologue that built a rest list wrong, or gave a celled
# parameter or local a cell the closure does not share, changes a total here.
#
# The counter-factual for the celled subjects is a JIT that refuses a function
# that builds a closure: they run interpreted, every answer stays right, and
# only `jit?` tells.

# ── variadic subjects ──────────────────────────────────────────────────
(defn vsum (& xs)
  (var s 0)
  (each x xs
    (assign s (+ s x)))
  s)

# fixed params + rest: exercises the prologue's fixed-arg loads AND the rest
# cons-list together (the `non_rest_params` boundary in collect_rest_list).
(defn vtail (a b & xs)
  (%add (%add a b) (length xs)))

# ── celled subjects: a nested closure captures and mutates them ─────────
(defn cell-param [@x]
  (let [bump (fn [] (assign x (+ x 1)))]
    (bump)
    (bump)
    x))

(defn cell-local [n]
  (var acc n)
  (let [bump (fn [k] (assign acc (+ acc k)))]
    (bump 1)
    (bump 2)
    acc))

# ── driver: when hot, calls every subject JIT->JIT ────────────────────
(defn driver (k)
  (var s 0)
  (var j 0)
  (while (%lt j k)
    (assign s (+ s (vsum 1 2 3 4 5)))
    (assign s (+ s (vtail 10 20 1 2 3)))
    (assign s (+ s (cell-param 10)))
    (assign s (+ s (cell-local 20)))
    (assign j (%add j 1)))
  s)

# Warmup: drive `driver` (and the subjects it calls) hot so adaptive JIT
# compiles them and later calls go JIT->JIT.
(defn warm (n)
  (var w 0)
  (while (%lt w n)
    (driver 5)
    (assign w (%add w 1))))
(warm 6000)
(jit/rejections)

# ── (a) correctness under JIT ─────────────────────────────────────────
# vsum(1..5)=15, vtail(10,20,1,2,3)=10+20+3=33, cell-param(10)=12,
# cell-local(20)=23; one driver iter adds 83.
(assert (= (driver 1) 83) "a compiled prologue produced a wrong result")
(assert (= (vsum 1 2 3 4 5) 15) "JIT vsum wrong")
(assert (= (vtail 10 20 1 2 3) 33) "JIT vtail wrong")
(assert (= (vsum) 0) "JIT vsum of no args must be 0 (empty rest list)")
(assert (= (vtail 4 5) 9) "JIT vtail with empty rest list wrong")
(assert (= (cell-param 10) 12) "the closure and the body share the param's cell")
(assert (= (cell-local 20) 23) "the closure and the body share the local's cell")

# ── (b) sustained correctness under a hot JIT->JIT driver ─────────────
(assert (= (driver 1000) 83000)
        "a compiled prologue corrupted results under a hot JIT->JIT driver")

# Only the celled subjects are read as values: a value use of `vtail` or
# `driver` turns off the call-site join that proves their `%` operands.
(when (vm/config :jit)
  (assert (and (jit? cell-param) (jit? cell-local))
          "the celled subjects compile, so their prologues build the cells"))

(println "region-jit-env: ok")
