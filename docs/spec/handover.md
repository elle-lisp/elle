# Hand-over at last use, in Datalog

<!-- audited: 2026-11-20 -->

Where a jump hands its value to a block parameter, and where the parameter is read out of the value, as Datalog that runs on proof.

[Recur](recur.md) holds the jumps a loop, a block and an arm's value make, and
[liveness](liveness.md) holds `live`. [Capture](capture.md) applies the rule
once the late captures are stores. The fences here include
[removal](removal.md), so they run its rules, and those of release, liveness
and Datalog too. [The ownership rules](ownership.md) state the rule in words.

Where `y` is used after a jump to the block parameter `x`, a hand-over would
leave two names holding one value, each with a release of its own. So `x` is
read out of `y` instead, and `y` waits for `x` under the Container waits rule,
as `kept-on` does for a call in [call](call.md). The jumps are facts of the
first pass, and `handed-over` returns the facts the other rules read.

| Derived | Holds when |
|---------|------------|
| `kept-on(x, y, p)` | in the first pass, `y` is live after the jump at `p` that hands it to `x` |

| Rule | Datalog |
|------|---------|
| Hand-over at last use | `kept-on(x, y, p) :- pass(y, x, p), next(p, q), live(y, q).` in the first pass |
| Hand-over at last use | `read(x, y, p) :- kept-on(x, y, p).`, which replaces the pass |

In `kw` below, both arms jump to `r` with `v`, and `v` is used after the join.
So `r` is read out of `v` on both paths, and `v` alone is released, after its
use at step 5. In `due`, each arm jumps with a value used after the join, so
`r` is read out of `later` on one path and out of `now` on the other. Both are
containers, so `r` has one state, and both wait for its use:

```lisp
(include-file "removal.md")

(defn one-state? [facts]
  "Whether no value is live in two states anywhere in FACTS."
  (def owned (ownership facts))
  (empty? (two-states facts owned (liveness facts owned))))

(defn handed-over [facts & waits]
  "FACTS with each jump whose value is live after it as a read, from a first pass with WAITS."
  (def live1 (proof:map (liveness facts (ownership facts) ;waits) (fn [vp] [vp true])))
  (def jumps (proof:relation (map (fn [[y x p]] [p [y x]]) (get facts :pass []))))
  # kept-on(x, y, p) :- pass(y, x, p), next(p, q), live(y, q).
  (def kept-on (proof:join (proof:join jumps (proof:relation (get facts :next))
                                       (fn [p [y x] q] [[y q] [y x p]]))
                           live1 (fn [_ yxp _] yxp)))
  (merge facts
         {:pass (proof:elements (proof:antijoin (proof:relation (map (fn [t] [t true])
                                                                     (get facts :pass [])))
                                                kept-on (fn [t _] t)))
          # read(x, y, p) :- kept-on(x, y, p).
          :read (concat (get facts :read []) (map (fn [[y x p]] [x y p]) (proof:elements kept-on)))}))

# (defn kw [c]
#   (def v (array 1))           # 0  v is made
#   (def r (if c v v))          # 1  the test   2  v jumps to r   3  v jumps to r
#   (length r)                  # 4  r is used
#   (length v))                 # 5  v is used
#                               # 6  the exit
(def kw (handed-over {:next  [[0 1] [1 2] [1 3] [2 4] [3 4] [4 5] [5 6]]
                      :alloc [[:v 0]]
                      :pass  [[:v :r 2] [:v :r 3]]
                      :use   [[:r 4] [:v 5]]}))
(assert (= [[:r :v 2] [:r :v 3]] (kw :read)) "a value used after the join is read, not handed")
(assert (= [[:v 5 6]] (releases kw (ownership kw))) "and released once, after its last use")

# (defn due [c]
#   (def now @{})               # 0  now is made
#   (def later @{})             # 1  later is made
#   (def r (if c later now))    # 2  the test   3  later jumps to r   4  now jumps to r
#   (put r :k 1)                # 5  r is used
#   (+ (length now) (length later)))   # 6  now and later are used
#                               # 7  the exit
(def due (handed-over {:next  [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5] [5 6] [6 7]]
                       :alloc [[:now 0] [:later 1]]
                       :pass  [[:later :r 3] [:now :r 4]]
                       :use   [[:r 5] [:now 6] [:later 6]]}))
(assert (one-state? due) "r is read out of a container on each path")
(assert (= [[:now 6 7] [:later 6 7]] (releases due (ownership due)))
        "each container is released after its own last use")
```

Where one arm hands over a fresh value and the other reads one, as in `kv`
below, the parameter has two states at the join, and the Join rule's target
refuses it. So does a loop that starts from a value used after the loop, as in
`lv`, since each later pass hands over a fresh value:

```lisp
# (defn kv [c]
#   (def v (array 1))           # 0  v is made
#   (def w (array 2))           # 1  w is made
#   (def r (if c v w))          # 2  the test   3  v jumps to r   4  w jumps to r
#   (length r)                  # 5  r is used
#   (length v))                 # 6  v is used
#                               # 7  the exit
(def kv (handed-over {:next  [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5] [5 6] [6 7]]
                      :alloc [[:v 0] [:w 1]]
                      :pass  [[:v :r 3] [:w :r 4]]
                      :use   [[:r 5] [:v 6]]}))
(assert (= [[:r 5]] (two-states kv (ownership kv) (liveness kv (ownership kv))))
        "a read on one path and a hand-over on the other leave r in two states")

# (defn lv [c]
#   (def v (array 1))           # 0  v is made
#   (loop [x v]                 # 1  v jumps to x   2  the loop head
#     (length x)                # 3  x is used, and the test
#     (if c (recur (array 2))   # 4  b is made   5  b jumps to x
#         nil))                 # 6  the exit path
#   (length v))                 # 7  v is used   8  the exit
(def lv (handed-over {:next  [[0 1] [1 2] [2 3] [3 4] [4 5] [5 2] [3 6] [6 7] [7 8]]
                      :alloc [[:v 0] [:b 4]]
                      :pass  [[:v :x 1] [:b :x 5]]
                      :use   [[:x 3] [:v 7]]}))
(assert (= [[:x 2] [:x 3]] (two-states lv (ownership lv) (liveness lv (ownership lv))))
        "a loop that starts from a value used after it is refused at its head")
```
