(elle/epoch 12)
## tests/elle/recur-mutual.lisp
##
## In-lambda MUTUAL recursion — a `letrec` pair nested in a function body, each
## closure capturing the other through its forward cell. The closure-cycle merge
## collapses the pair and its cells onto one arena, freed at the letrec binding
## scope or — when the letrec body tail-calls a member — by the tail-call adopt
## at the recursion's normal completion (docs/impl/region-model.md § The letrec
## closure-cycle merge). The leak side is pinned by oracle.lisp
## (recur-local-mutual); these assert VALUES, which neither the leak gauge nor
## guardfree can see (docs/testing.md "Correctness the leak and UAF oracles
## cannot see"): a mis-freed arena or a stale sibling reference returns a
## wrong-but-well-typed result, caught only here.

## 1. Tail-call letrec body (the canonical shape): the binding-scope drop is
## dead past the frame-replacing TailCall; the adopt supplies the release.
(defn parity [n]
  (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))]
    (ev n)))
(assert (= (parity 0) :even)
        "in-lambda mutual recursion: base case enters and returns")
(assert (= (parity 1) :odd) "in-lambda mutual recursion: one rotation")
(assert (= (parity 7) :odd) "in-lambda mutual recursion: odd depth")
(assert (= (parity 10) :even) "in-lambda mutual recursion: even depth")

## 2. Non-tail letrec body: the binding-scope drop fires live, after the call.
(defn parity-nontail [n]
  (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))]
    (let [r (ev n)]
      r)))
(assert (= (parity-nontail 6) :even) "non-tail mutual letrec body: even depth")
(assert (= (parity-nontail 3) :odd) "non-tail mutual letrec body: odd depth")

## 3. Mixed tail exits: the member-callee path adopts; the value path falls
## through to the live binding-scope drop. Exactly one release per path.
(defn parity-mixed [n]
  (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))]
    (if (%lt n 0) :neg (ev n))))
(assert (= (parity-mixed -1) :neg) "mixed-exit mutual letrec: value path")
(assert (= (parity-mixed 5) :odd) "mixed-exit mutual letrec: member tail path")
(assert (= (parity-mixed 0) :even)
        "mixed-exit mutual letrec: member tail path, base case")

## 4. Heap values built through the rotation survive the recursion — the merged
## arena must not be freed while the accumulator still flows through it.
(defn labels [n]
  (letrec [ev (fn [m acc]
                (if (%lt m 1)
                  acc
                  (od (%sub m 1) (pair (string "e" m) acc))))
           od (fn [m acc]
                (if (%lt m 1)
                  acc
                  (ev (%sub m 1) (pair (string "o" m) acc))))]
    (ev n (list))))
(assert (= (first (labels 4)) "o1") "mutual rotation: innermost label survives")
(assert (= (length (labels 4)) 4) "mutual rotation: every label survives")

## 5. Loop-driven churn: one merged arena per call, reclaimed per call.
(def @i 0)
(while (%lt i 300)
  (parity 5)
  (parity-nontail 4)
  (assign i (%add i 1)))
(assert (= i 300) "300 iterations of in-lambda mutual recursion completed")

(println "recur-mutual: ok")
