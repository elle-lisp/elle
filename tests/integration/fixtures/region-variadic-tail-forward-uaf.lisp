(elle/epoch 12)
# tests/integration/fixtures/region-variadic-tail-forward-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression in either
# direction is loud: an OVER-FREE faults under --trace=guardfree (and the debug
# edge-table equivalence oracle / generation stamp panics at the drifted free),
# and `make smoke` globs tests/elle/*.lisp into one shared process where an abort
# takes the whole harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_variadic_tail_forward_uaf`).
#
# WHAT IT PINS — the variadic TAIL-FORWARD reference balance. When a function
# forwards a heap value into a `& rest` variadic through a TAIL call
# (`(defn g [& rest] …) (defn f [x] (g x))`), the callee env is built as a MOVE
# (`own_params = false`): the caller's owning reference to each arg transfers to the
# callee. A fixed param lands that reference in its env slot; a rest arg instead
# lives in the collected rest-list, which `args_to_list`'s `alloc_obj` gave its OWN
# incref. So the moved-in reference is SURPLUS and must be released — else it leaks
# one region per rest arg per call (the `store-wrapper` oracle probe: stdlib `put`'s
# `& rest` + a heap value forwarded through an indirect wrapper call). The release
# is applied ONLY to a value appearing exactly once across all arg positions: an
# aliased arg (`(g x x)`) shares one transferred reference a fixed slot / earlier
# cons already consumes, so a second release would OVER-FREE — leak-safe, never
# mis-free.
#
# TWO FAILURE MODES, one fixture:
#   - UNDER-release (the pre-fix leak) — region-count grows per op; the assert below
#     catches it (the store-wrapper leak face).
#   - OVER-release (a mis-fix decreffing an aliased or borrowed arg) — a stale-region
#     deref once the freed page recycles; the guardfree trace / generation stamp
#     faults. State-dependent, so the PRIMING loop churns ids first.
#
# A rewrite must not be "verified" on a short run: the over-free lands only once
# region ids recycle onto the freed one.

(def @sink @{:x 0})

# The minimal tail-forward: `f` forwards its heap param into `g`'s variadic.
(defn ignore-rest [& rest]
  5)
(defn forward-one [x]
  (ignore-rest x))

# The store-wrapper shape: an indirect wrapper over stdlib `put`, whose `& rest`
# reads the forwarded heap value and stores it into a persistent @struct.
(defn put-wrap [c v]
  (put c :x v))

# An aliased forward — the same value in a fixed slot AND the variadic. The move
# transfers one reference; over-releasing the rest occurrence would over-free.
(defn head-and-rest [a & rest]
  5)
(defn forward-aliased [x]
  (head-and-rest x x))

(defn churn [n]
  (def @i 0)
  (while (< i n)
    (forward-one (string "f" i))
    (put-wrap sink (string "v" i))
    (forward-aliased (string "a" i))
    (assign i (+ i 1))))

# Prime: churn region ids so any freed page below is recycled onto a live region.
(churn 1500)

# Measure steady-state region growth: the forwarded heap values must be reclaimed
# (the moved-in surplus released), so region-count is bounded across the window.
(def r0 (arena/region-count))
(churn 1500)
(def r1 (arena/region-count))
(def growth (- r1 r0))
(assert (< growth 50)
        (string "variadic tail-forward leaked " growth " regions over 1500 ops"))

(println "region-variadic-tail-forward-uaf: ok")
