(elle/epoch 12)
# Search loop fusion — value preservation, early exit, realization
# (docs/impl/dissolution.md § "Search — the terminal that stops early").
#
# `any?`, `all?`, `find` and `find-index` each answer a question about the FIRST
# element their predicate decides and stop there. Each wears a `filter`'s
# two-argument shape and produces a scalar, so each is a terminal exactly as
# `fold` and `count` are: the predicate becomes the pipeline's guard stage, the
# accumulator is a scalar seeded with the answer for "no element decided it", and —
# where the search is lone — the loop leaves through a `more` sentinel its condition
# reads. `all?` is the one decided by a REJECTING element, so its guard carries the
# pipeline on the else side. Over a `map`/`filter` prefix the early exit stops the
# search's own stage rather than the walk, because the staged form runs every
# prefix stage on every element.
#
# This file is the behavioral gauge: the fused form must compute EXACTLY
# what the stdlib op computes, must read no element past the decision when it is
# lone, and must run its prefix on every element when it is not. The codegen gauge
# (the dispatch gone, the predicate inline, a scalar accumulator, where the
# sentinel is read) lives in `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference applies each op through a named function with a
# `match` body — a binding-introducing form a fragment cannot close over —
# so those calls stay plain stdlib dispatches and mint what the fused form does
# not. Same value.

(defn gt2 [x]
  (match x
    _ (> x 2)))
(defn pos? [x]
  (match x
    _ (> x 0)))
(defn nump [x]
  (match x
    _ (number? x)))
(defn evp [x]
  (match x
    _ (even? x)))
(defn oddp [x]
  (match x
    _ (odd? x)))
(defn t3 [x]
  (match x
    _ (* x 3)))
(defn inc1 [x]
  (match x
    _ (+ x 1)))

# ── any? — decided by the first admitted element ───────────────────────
(assert (= (any? (fn [x] (> x 2)) [1 2 3 4]) true)
        "any? finds an admitted element")
(assert (= (any? (fn [x] (> x 9)) [1 2 3 4]) false)
        "any? with no admitted element is false, not nil")
(assert (= (any? (fn [x] (> x 9)) []) false) "any? over an empty array is false")
(assert (= (any? (fn [x] (> x 2)) [1 2 3 4]) (any? gt2 [1 2 3 4]))
        "fused any? agrees with the un-fused named-fn form")
# Only `nil` and `false` are falsy, so a predicate answering 0 still admits.
(assert (= (any? (fn [x] 0) [1 2]) true)
        "0 is truthy — the guard follows Elle truthiness")
(assert (= (any? (fn [x] nil) [1 2]) false) "nil is falsy")

# ── all? — decided by the first rejected element ───────────────────────
(assert (= (all? (fn [x] (> x 0)) [1 2 3 4]) true)
        "all? holds when none is rejected")
(assert (= (all? (fn [x] (> x 2)) [1 2 3 4]) false)
        "all? fails at the first rejection")
(assert (= (all? (fn [x] (> x 9)) []) true) "all? over an empty array is true")
(assert (= (all? (fn [x] (> x 0)) [1 2 3 4]) (all? pos? [1 2 3 4]))
        "fused all? agrees with the un-fused named-fn form")
(assert (= (all? (fn [x] 0) [1 2]) true)
        "0 is truthy — every element is admitted")
(assert (= (all? (fn [x] nil) [1 2]) false)
        "nil is falsy — the first element rejects")

# ── find — the admitted element itself ─────────────────────────────────
(assert (= (find (fn [x] (> x 2)) [1 2 3 4]) 3)
        "find returns the first admitted element")
(assert (= (find (fn [x] (> x 9)) [1 2 3 4]) nil)
        "find with no admitted element is nil")
(assert (= (find (fn [x] (> x 9)) []) nil) "find over an empty array is nil")
(assert (= (find (fn [x] (> x 2)) [1 2 3 4]) (find gt2 [1 2 3 4]))
        "fused find agrees with the un-fused named-fn form")
# The recorded value is the element, so a heap element comes back whole.
(assert (= (find (fn [s] (= (length s) 3)) ["ab" "xyz" "q"]) "xyz")
        "find hands a heap element out of the loop")
# A nil element that the predicate admits is indistinguishable from "not found",
# exactly as it is for the stdlib op — the answer is nil either way.
(assert (= (find (fn [x] (nil? x)) [nil 2]) nil)
        "an admitted nil element answers nil")

# ── find-index — the position of the admitted element ──────────────────
(assert (= (find-index (fn [x] (> x 2)) [1 2 3 4]) 2)
        "find-index returns the position")
(assert (= (find-index (fn [x] (> x 0)) [1 2 3 4]) 0) "a decision at position 0")
(assert (= (find-index (fn [x] (> x 9)) [1 2 3 4]) nil)
        "find-index with no admitted element is nil")
(assert (= (find-index (fn [x] (> x 9)) []) nil)
        "find-index over an empty array is nil")
(assert (= (find-index (fn [x] (> x 2)) [1 2 3 4]) (find-index gt2 [1 2 3 4]))
        "fused find-index agrees with the un-fused named-fn form")

# ── the early exit: no element past the decision is read ───────────────
# The fused loop leaves through the `more` sentinel it clears at the deciding
# element. A predicate that ERRORS on every later element is the sharp gauge: the
# division by zero is unreachable if — and only if — the walk truly stops. It gauges
# the element being READ, which is stronger than any tally of the predicate's own
# calls: the error fires from inside the body the loop would have run.
(assert (= (any? (fn [x] (> (/ 6 x) 1)) [3 0]) true)
        "any? stops at the deciding element — the later one is never read")
(assert (= (all? (fn [x] (> (/ 6 x) 3)) [3 0]) false)
        "all? stops at the rejecting element — the later one is never read")
(assert (= (find (fn [x] (> (/ 6 x) 1)) [3 0]) 3)
        "find stops at the deciding element — the later one is never read")
(assert (= (find-index (fn [x] (> (/ 6 x) 1)) [3 0]) 0)
        "find-index stops at the deciding element — the later one is never read")

# The decision may be the LAST element, so the walk reads everything up to it and
# nothing after: the answer names position 3 of a 4-element input.
(assert (= (find-index (fn [x] (> x 30)) [10 20 30 40]) 3)
        "a decision at the last element is reached")
# An undecided walk reads every element and answers the seed.
(assert (= (any? (fn [x] (> x 99)) [10 20 30 40]) false)
        "an undecided walk covers the whole input and answers the seed")

# ── the prefix fuses, and the early exit stops the SEARCH alone ────────
# A staged `(any? p (map f xs))` runs `f` over the WHOLE input and `p` over the
# elements up to the decision, so the fused loop makes exactly those calls: the
# walk stays exhaustive and the `more` sentinel gates the search's own stage.
(assert (= (any? (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4])) true)
        "any? over a map prefix computes the staged value")
(assert (= (any? (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))
           (any? evp (map t3 [1 2 3 4])))
        "the fused composition agrees with the fully un-fused form")
(assert (= (all? (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))
           (all? evp (map t3 [1 2 3 4])))
        "all? over a map prefix agrees with the un-fused form")
# `find` over a map prefix answers the TRANSFORMED value, not the base element.
(assert (= (find (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4])) 6)
        "find over a map prefix answers the mapped value")
(assert (= (find (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))
           (find evp (map t3 [1 2 3 4])))
        "the fused find composition agrees with the un-fused form")

# A prefix stage runs on every element, decision or not. A transform that fails on
# an element PAST the decision must still raise, exactly as the staged form does:
# `(/ 6 0)` on the second element is reached only if the walk does not stop at the
# first, which decides the answer.
(assert (= :division-by-zero (try
                               (any? (fn [y] (even? y))
                                     (map (fn [x] (/ 6 x)) [3 0]))
                               (catch e e:error)))
        "a prefix runs past the decision — its error still surfaces")

# The other half: the PREDICATE stops at the decision even though the walk does
# not. `(/ 6 0)` in the predicate is reached only if the sentinel gate fails to
# hold it off the second element, which the first element already decided.
(assert (= (find (fn [y] (even? (/ 6 y))) (map (fn [x] (* x 1)) [3 0])) 3)
        "the sentinel gate keeps the predicate off elements past the decision")

# A filter prefix renumbers, so `find-index` answers a position in the FILTERED
# input, which the loop carries as the surviving element's own count. The base
# below puts the admitted element at base index 3 and filtered index 2, so the two
# readings are distinguishable.
(assert (= (find-index (fn [y] (even? y))
                       (filter (fn [w] (number? w)) [1 "a" 3 4])) 2)
        "find-index over a filter prefix answers a position in the FILTERED input")
(assert (= (find-index (fn [y] (even? y))
                       (filter (fn [w] (number? w)) [1 "a" 3 4]))
           (find-index evp (filter nump [1 "a" 3 4])))
        "the fused find-index composition agrees with the un-fused form")
(assert (= (find-index (fn [y] (even? y))
                       (filter (fn [w] (number? w)) [1 "a" 3])) nil)
        "no survivor is admitted — the answer is the seed")
(assert (= (find (fn [y] (even? y)) (filter (fn [w] (number? w)) [1 "a" 3 4])) 4)
        "find over a filter prefix answers the surviving element")

# A three-op prefix: the filter renumbers and the map does not, so the answer is
# the survivor's position — 2 among the three survivors, where the base index is 3.
(assert (= (find-index (fn [y] (odd? y))
                       (map (fn [x] (+ x 1))
                            (filter (fn [w] (number? w)) [1 "a" 3 4]))) 2)
        "a mixed prefix renumbers by its filter alone")
(assert (= (find-index (fn [y] (odd? y))
                       (map (fn [x] (+ x 1))
                            (filter (fn [w] (number? w)) [1 "a" 3 4])))
           (find-index oddp (map inc1 (filter nump [1 "a" 3 4]))))
        "the three-op composition agrees with the fully un-fused form")

# A NON-reorder-safe body declines the whole composition (`>` routes through
# `apply`), and the pre-order recursion still fuses the inner `map`. Same value.
(assert (= (any? (fn [y] (> y 9)) (map (fn [x] (* x 3)) [1 2 3 4]))
           (any? gt2 (map t3 [1 2 3 4])))
        "the declined composition agrees with the fully un-fused form")

# ── the mutable-array base declines ────────────────────────────────────
# Each search's array arm re-reads `(length coll)` every iteration where the fused
# loop captures `len` once, so a mutable base stays a plain call.
(def @mut @[1 2 3 4])
(assert (= (any? (fn [x] (> x 3)) mut) true)
        "a mutable @array base searches un-fused")
(assert (= (find (fn [x] (> x 3)) mut) 4) "find over a mutable @array base")
(assert (= (length mut) 4) "searching does not disturb the mutable base")

# ── named predicates and Var-bound bases ───────────────────────────────
(defn big? [x]
  (> x 2))
(assert (= (any? big? [1 2 3]) true) "a named same-unit predicate inlines")
(assert (= (all? inc [1 2 3]) true) "a cross-unit stdlib predicate inlines")

(def base [1 2 3 4 5 6])
(assert (= (find (fn [x] (> x 4)) base) 5) "a Var-bound base searches")
(assert (= (get base 0) 1) "the base Var survives the fused search")

# ── Realization: the walker closure and its forward cell ───────────────
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). Each search's array arm walks with a
# `letrec`-bound self-recursive closure, so the un-fused call mints that closure
# and its forward cell per call, plus the predicate closure wherever the argument
# is a lambda literal. The fused loop mints none of the three.
#
# The walks below are UNDECIDED — the predicate admits nothing (or rejects
# nothing), so both forms visit every element and the comparison is over the same
# work. That matters: a walk that decides early would compare a fused loop that
# runs one full iteration for the deciding element against a recursive walker that
# answers without its final recursive step, and the per-element cost of that odd
# step would swamp the two objects fusion removes. The early exit is gauged above,
# in predicate calls.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def odds [1 3 5 7 9 11 13 15 17 19])

(def any-fused
  (allocs (fn [] (any? (fn [x] (even? x)) [1 3 5 7 9 11 13 15 17 19]))))
(def any-unfused (allocs (fn [] (any? evp [1 3 5 7 9 11 13 15 17 19]))))
(assert (= (any? (fn [x] (even? x)) odds) (any? evp odds))
        "fused and un-fused any? compute the same value")
(assert (< any-fused any-unfused)
        (string "a fused any? must mint fewer (no walker closure): " any-fused
                " vs " any-unfused))

(def all-fused
  (allocs (fn [] (all? (fn [x] (odd? x)) [1 3 5 7 9 11 13 15 17 19]))))
(def all-unfused (allocs (fn [] (all? oddp [1 3 5 7 9 11 13 15 17 19]))))
(assert (= (all? (fn [x] (odd? x)) odds) (all? oddp odds))
        "fused and un-fused all? compute the same value")
(assert (< all-fused all-unfused)
        (string "a fused all? must mint fewer: " all-fused " vs " all-unfused))

(def find-fused
  (allocs (fn [] (find (fn [x] (even? x)) [1 3 5 7 9 11 13 15 17 19]))))
(def find-unfused (allocs (fn [] (find evp [1 3 5 7 9 11 13 15 17 19]))))
(assert (= (find (fn [x] (even? x)) odds) (find evp odds))
        "fused and un-fused find compute the same value")
(assert (< find-fused find-unfused)
        (string "a fused find must mint fewer: " find-fused " vs " find-unfused))

(def idx-fused
  (allocs (fn [] (find-index (fn [x] (even? x)) [1 3 5 7 9 11 13 15 17 19]))))
(def idx-unfused (allocs (fn [] (find-index evp [1 3 5 7 9 11 13 15 17 19]))))
(assert (= (find-index (fn [x] (even? x)) odds) (find-index evp odds))
        "fused and un-fused find-index compute the same value")
(assert (< idx-fused idx-unfused)
        (string "a fused find-index must mint fewer: " idx-fused " vs "
                idx-unfused))

# Over a prefix the fused loop additionally removes the INTERMEDIATE array. The
# reference isolates exactly that: `my-any` is the same stdlib function under a
# user binding, which is not `is_primitive` and so is never recognized as a
# terminal, while the inner `map` — the identical lambda over the identical base —
# still fuses on the recursion. The two forms are otherwise the same program, so
# the difference is exactly what fusing the search over a prefix buys: the
# intermediate array between the two ops, the search's walker closure and its
# forward cell, and the predicate closure the splice dissolves.
(def my-any any?)
(def pre-fused
  (allocs (fn []
            (any? (fn [y] (even? y))
                  (map (fn [x] (* x 3)) [1 3 5 7 9 11 13 15 17 19])))))
(def pre-staged
  (allocs (fn []
            (my-any (fn [y] (even? y))
                    (map (fn [x] (* x 3)) [1 3 5 7 9 11 13 15 17 19])))))
(assert (= (any? (fn [y] (even? y)) (map (fn [x] (* x 3)) odds))
           (my-any (fn [y] (even? y)) (map t3 odds)))
        "the fused prefixed any? computes the staged value")
(assert (< pre-fused pre-staged)
        (string "a prefixed any? must mint fewer (no intermediate array): "
                pre-fused " vs " pre-staged))

(println "dissolution-search-fuse: ok (any? saved " (- any-unfused any-fused)
         ", all? saved " (- all-unfused all-fused) ", find saved "
         (- find-unfused find-fused) ", find-index saved "
         (- idx-unfused idx-fused) ", prefixed any? saved "
         (- pre-staged pre-fused) ")")
