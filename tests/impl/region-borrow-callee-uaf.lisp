(elle/epoch 15)
# audited: 2026-10-21
# A closure read out of a pair stays live while a tail call enters it through the name it is bound to.
#
# docs/impl/region/borrow.md
# docs/impl/region/relocate.md
#
# The sidecar arms guardfree, keeps the JIT off and turns the graft off, so a
# stale read faults at its deref.
#
# `outer` moves a fresh pair into `pick`, so `pick`'s frame holds the pair's
# only reference. `g` is the pair's car, read with no count, and `pick`
# tail-calls it. The pair's count is all that keeps the closure alive, so no
# release of the pair may run ahead of the call. That rules out the solver's,
# the relocation's carried ahead of the `TailCall`, and a compensating release
# at the head of the arm that makes the call.
#
# The counter-factual: with `g` naming no region, each of those releases would
# free the pair, and its cascade would free the closure the call then enters.

(defn pick [fns c]
  (let [g (begin
            nil
            (%first fns))]
    (if c (g 1) (length fns))))

# A `def` encloses no branch, so the pair's release lands in the arm that reads
# `fns` itself. The arm that calls `g` reads the pair only through `g`, and must
# not take a release at its head as an arm that never reads the pair.
(defn pick-def [fns c]
  (def g (%first fns))
  (if c (g 1) (length fns)))

(defn outer [i c]
  (pick (%pair (fn [x] (string "v" x i)) ()) c))

(defn outer-def [i c]
  (pick-def (%pair (fn [x] (string "v" x i)) ()) c))

(defn churn [n]
  (def @i 0)
  (while (< i n)
    (assert (= (outer i true) (string "v" 1 i))
            "the call enters the borrowed closure")
    (assert (= (outer i false) 1) "the other arm reads the pair")
    (assert (= (outer-def i true) (string "v" 1 i))
            "the call enters the def-bound closure")
    (assert (= (outer-def i false) 1) "the other arm reads the pair")
    (assign i (+ i 1))))

# Churn region ids so a freed page is recycled onto a live region.
(churn 1000)

(println "region-borrow-callee-uaf: ok")
