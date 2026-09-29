(elle/epoch 12)
# audited: 2026-09-29
# A capturing lambda literal fuses at every op and computes the stdlib op's value,
# and a chain of two or more capturing ops keeps the staged call order.
#
# docs/impl/dissolution.md
#
# The codegen pins — the dispatch gone, the body inline, where the capture reaches
# from — are in src/hir/typeinfer/fuse/tests/capture.rs. This file checks values,
# call order and allocation counts. The order check below is the one instrument
# that sees a capture: a shared log tells the staged order (all of the inner op,
# then all of the outer) from the interleaved one a fused loop would run.
#
# The un-fused oracles here are named functions with a `match` body — a
# binding-introducing form a fragment cannot close over — so each stays a real
# stdlib call.

(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(defn r-mul3 [x]
  (match x
    _ (* x 3)))

(def nums [0 1 2 3 4 5 6 7 8 9])

# ── every op fuses a capture, and computes the stdlib value ────────────
(assert (= (let [m 3]
             (map (fn [x] (* x m)) [1 2 3])) [3 6 9])
        "a capturing map transform")
(assert (= (let [k 2]
             (filter (fn [x] (> x k)) [1 2 3 4])) [3 4])
        "a capturing filter predicate")
(assert (= (let [k 10]
             (fold (fn [a x] (+ a (* x k))) 0 [1 2 3])) 60)
        "a capturing fold combinator")
(assert (= (let [k 2]
             (count (fn [x] (> x k)) [1 2 3 4])) 2)
        "a capturing count predicate")
(assert (= (let [k 2]
             (any? (fn [x] (> x k)) [1 2 3])) true) "a capturing any? predicate")
(assert (= (let [k 9]
             (all? (fn [x] (< x k)) [1 2 3])) true) "a capturing all? predicate")
(assert (= (let [k 2]
             (find (fn [x] (> x k)) [1 2 3 4])) 3) "a capturing find predicate")
(assert (= (let [k 2]
             (find-index (fn [x] (> x k)) [1 2 3 4])) 2)
        "a capturing find-index predicate")
(assert (= (->list (let [k 3]
                     (take-while (fn [x] (< x k)) [1 2 3 4]))) (list 1 2))
        "a capturing take-while predicate")
(assert (= (->list (let [k 3]
                     (drop-while (fn [x] (< x k)) [1 2 3 4]))) (list 3 4))
        "a capturing drop-while predicate")
(assert (= (->list (let [k 10]
                     (map-indexed (fn [i x] (+ (* i k) x)) [7 8]))) (list 7 18))
        "a capturing map-indexed function")
(assert (= (->list (let [k 5]
                     (mapcat (fn [x] [x k]) [1 2]))) (list 1 5 2 5))
        "a capturing mapcat function")

# A capture reaching two function levels out fuses: the inner lambda's capture
# propagates to the enclosing function's own capture list, so the spliced read
# resolves from there.
(assert (= (let [k 4]
             ((fn [] (map (fn [x] (+ x k)) [1 2])))) [5 6])
        "a capture from two function levels out")

# A capture reaching a lambda PARAMETER of the enclosing function fuses the same way
# — a parameter is a slot that function holds, exactly as a local is.
(defn scale-all [factor]
  (map (fn [x] (* x factor)) [1 2 3]))
(assert (= (scale-all 3) [3 6 9])
        "a capture of the enclosing function's parameter")

# ── a MUTABLE capture is read live, per element ────────────────────────
# The capture cells the binding, and the spliced read unwraps that cell exactly as
# every other read of it does — so a value the body itself assigns is visible to the
# next element, as it was through the closure.
(assert (= (let [@run 0]
             (map (fn [x]
                    (assign run (+ run x))
                    run) [1 2 3])) [1 3 6])
        "a mutable capture accumulates across the fused walk")

# ── a capture of the MUTABLE base itself ───────────────────────────────
# A lone `map` over a mutable `@array` fuses, and its stdlib arm captures `len` once
# and reads `coll` live. The fused loop does exactly that, so a body that mutates the
# base through a capture of its own binding computes the same value: three elements
# visited, each read before the push that follows it.
(assert (= (->list (let [xs @[1 2 3]]
                     (map (fn [x]
                            (push xs 9)
                            x) xs))) (list 1 2 3))
        "a capture of the mutable base reads it live against a `len` taken once")

# ── the composition gate: a capture keeps the staged order ─────────────
# The two lambdas share `log`, so the ORDER of their calls is observable. The staged
# form runs the whole inner op, then the whole outer one; a fused loop would
# interleave them. The chain must decline, and the recursion still fuses the inner
# run — which changes no order, the inner op's own calls staying left to right.
(assert (= (->list (let [log @[]]
                     (map (fn [y]
                            (push log :g)
                            y)
                          (map (fn [x]
                                 (push log :f)
                                 x) [1 2]))
                     log)) (list :f :f :g :g))
        "a capturing composition keeps the staged order (fused would interleave)")

# A LONE op has no sibling to interleave with, so it is admitted — and its own calls
# run left to right, exactly as the stdlib op's do.
(assert (= (->list (let [log @[]]
                     (map (fn [x]
                            (push log x)
                            x) [1 2 3])
                     log)) (list 1 2 3))
        "a lone capturing op visits the elements in walk order")

# The terminal counts as an op, so a capturing predicate over a prefix is a chain of
# two and declines the same way. The value is the stdlib op's either way.
(assert (= (let [k 3]
             (count (fn [y] (> y k)) (map (fn [x] (* x 2)) [1 2 3]))) 2)
        "a capturing terminal over a prefix declines and still counts")

# ── the self-reference declines ────────────────────────────────────────
# A lambda naming the binding its own call initializes resolves to the EXECUTING
# closure — which is the element function itself, and which fusion would remove. The
# chain declines, so every element past the first still re-enters that function.
(def selfref (map (fn [x] (if (< x 2) x (selfref 1))) [1 2 3]))
(assert (= selfref [1 1 1]) "a self-referencing element function declines")

# ── Realization: the capture's own closure is gone ─────────────────────
# A capturing lambda mints a closure per evaluation and is called once per element;
# fused, it mints neither. `arena/total-allocs` counts every object ever minted.
(def cap-fused
  (allocs (fn []
            (let [m 3]
              (map (fn [x] (* x m)) nums)))))
(def cap-unfused (allocs (fn [] (map r-mul3 nums))))
(assert (= (let [m 3]
             (map (fn [x] (* x m)) nums)) (map r-mul3 nums))
        "fused capturing map and the un-fused oracle compute the same value")
(assert (< cap-fused cap-unfused)
        (string "a fused capturing map must mint strictly fewer objects: "
                cap-fused " vs " cap-unfused))

# And it fuses to the SAME cost as the global-only form: the capture itself buys
# nothing back, which is the whole claim — the splice needs no machinery.
(def plain-fused (allocs (fn [] (map (fn [x] (* x 3)) nums))))
(assert (= cap-fused plain-fused)
        (string "a capturing map fuses identically to a global-only one: "
                cap-fused " vs " plain-fused))

(println "dissolution-capture-fuse: ok (capture saved "
         (- cap-unfused cap-fused) ")")
