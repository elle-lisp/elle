(elle/epoch 12)
# tests/integration/fixtures/region-tail-repeated-arg-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression SIGSEGVs under
# the plain VM as well as under --trace=guardfree, and `make smoke` globs
# tests/elle/*.lisp into one shared process where a segfault would take the whole
# harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_tail_repeated_arg_uaf`).
#
# WHAT IT GUARDS — the tail-call move is ONE reference per OCCURRENCE, not one
# per call (docs/impl/region/rules.md Rule 5, the borrowed tail-call argument).
#
# A tail call pure-moves its arguments: the caller emits no incref and its own
# release lands in the dead post-`TailCall` block, and that skipped release IS
# the reference the callee's owned-param release consumes. The frame holds ONE
# reference to a region, though, while the callee releases once per PARAMETER. So
# an argument list that names the same region twice hands over one reference
# against two releases, and the second drops it to zero while the caller is still
# using the value. The stdlib shape is `concat`'s `(concat-seq a rest a false)`,
# where the mutable first argument is both the source and the accumulator.
#
# Only the FIRST owned occurrence is funded by the move; every later one is
# minted exactly as a borrowed argument is. Repetition is a question about the
# REGIONS the arguments may name, not about syntax — (b) below passes two
# distinct bindings that resolve to one region, and a check keyed on binding
# identity misses it.
#
# The witnesses read the subject's HEAP contents after the call returns, through
# a chain long enough that an over-early free faults rather than reading stale but
# still-mapped bytes; a fresh subject per iteration keeps region ids churning so a
# freed region is recycled under the reader.

# (a) the same BINDING in two argument positions of one tail call. `two-arg`
# releases both of its owned parameters, so the caller's single reference funds
# one of them and the other must be minted.
(defn two-arg [p q r]
  (%add (length p) (length q)))
(defn a-repeat [x]
  (two-arg x [9] x))
(defn a-read [i]
  (let [a (@array)]
    (push a (string "a" i))
    (a-repeat a)
    (length (first a))))

# (b) two DISTINCT bindings naming one region. Nothing syntactic relates `x` and
# `y` at the call, so the repetition is only visible through the regions their
# value-producing leaves may name.
(defn b-alias [x]
  (let [y x]
    (two-arg x [9] y)))
(defn b-read [i]
  (let [a (@array)]
    (push a (string "b" i))
    (b-alias a)
    (length (first a))))

# (c) the stdlib shape itself: `concat` on a MUTABLE first argument dispatches to
# `(concat-seq a rest a false)`, passing the accumulator in as both the source and
# the destination, and returns it extended in place.
(defn c-read [i]
  (let [a (@array)]
    (push a (string "c" i))
    (concat a @[1 2])
    (length (first a))))

# (d) the same through the value the call hands back, so the read goes through the
# result rather than through the caller's own binding.
(defn d-read [i]
  (let [a (@array)]
    (push a (string "d" i))
    (length (first (concat a @[1 2])))))

# ── controls: one occurrence each, which the move funds on its own ────────────
(defn e-single [x]
  (two-arg x [9] [8]))
(defn e-read [i]
  (let [a (@array)]
    (push a (string "e" i))
    (e-single a)
    (length (first a))))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(while (%lt i 2000)
  (assign a (a-read i))
  (assign b (b-read i))
  (assign c (c-read i))
  (assign d (d-read i))
  (assign e (e-read i))
  (assign i (%add i 1)))

(assert (%gt e 0)
        "control: single-occurrence tail move mis-read (harness broken)")
(assert (%gt a 0) "value freed under the caller after a repeated tail argument")
(assert (%gt b 0)
        "value freed under the caller after two aliased tail arguments")
(assert (%gt c 0)
        "concat's accumulator freed under the caller that still holds it")
(assert (%gt d 0) "concat's result freed under the caller's read of it")

# The mint the later occurrence takes must be CONSUMED, not stranded: a fix that
# increfs every occurrence and relies on nobody noticing would trade this
# over-free for a per-call leak. Each shape's steady-state region growth must be
# flat.
(defn measure [thunk]
  (var j 0)
  (while (%lt j 200)
    (thunk)
    (assign j (%add j 1)))
  (def before (arena/region-count))
  (var k 0)
  (while (%lt k 2000)
    (thunk)
    (assign k (%add k 1)))
  (%sub (arena/region-count) before))

(def a-d (measure (fn () (a-read 1))))
(def b-d (measure (fn () (b-read 2))))
(def e-d (measure (fn () (e-read 3))))
(println "region-tail-repeated-arg deltas: a " a-d "  b " b-d "  control " e-d)
(assert (%lt a-d 100) "repeated tail argument strands a region per call")
(assert (%lt b-d 100) "aliased tail arguments strand a region per call")
(assert (%lt e-d 100) "control: single occurrence strands a region per call")

(println "region-tail-repeated-arg-uaf: ok")
