# Recur, Break and Eval in Datalog

<!-- audited: 2026-11-20 -->

How a loop hands each new value to its parameter, a block its value to its consumer, and an eval its result, as Datalog that runs on proof.

[Datalog](datalog.md) holds the `pass` fact and the Recur clauses for
`owned`, and [liveness](liveness.md) holds the one for `live`. A recur, a break,
and an arm's value at a join each hand a value to a block parameter, so Break
needs no clause of its own. Eval needs none either. [The ownership
rules](ownership.md) state each rule in words.

A loop parameter names one value on each pass. The step that recurs hands the
new value over, so the new value has no release of its own, and the old value's
hold ends there. The old value is released at its last use in its pass, or
later where a new value lies inside it. Each loop below has one state at its
head.

`l1` walks a list it made. Each new `xs` is read out of the old one, so it
keeps the old one's owner, and the old one waits for it. `xs` is released once,
on the edge out of the loop:

```lisp
(include-file "release.md")

(defn one-state? [facts]
  "Whether no value is live in two states anywhere in FACTS."
  (def owned (ownership facts))
  (empty? (two-states facts owned (liveness facts owned))))

(def loop-next [[0 1] [1 2] [2 3] [3 4] [4 1] [2 5] [5 6]])

# (defn l1 []
#   (def xs (list 1 2 3))       # 0  xs is made
#   (loop [xs xs]               # 1  the loop head
#     (if (pair? xs)            # 2  xs is used
#       (recur (rest xs))       # 3  ys is read out of xs   4  ys is handed to xs
#       nil)))                  # 5  the exit path          6  the exit
(def l1 {:next loop-next
         :alloc [[:xs 0]]
         :read  [[:ys :xs 3]]
         :pass  [[:ys :xs 4]]
         :use   [[:xs 2] [:xs 3]]})
(assert (one-state? l1) "the new xs keeps the old one's owner")
(assert (= [[:xs 2 5]] (releases l1 (ownership l1))) "xs is released once, after the loop")
```

`l2` makes a new value on each pass, so each pass releases the old `a` after its
use at step 2. `l3` builds an accumulator: the Allocation operand rule moves the
old `acc` into the new pair, so nothing is released inside the loop:

```lisp
# (defn l2 [c]
#   (loop [a (array 1)]         # 0  a is made   1  the loop head
#     (length a)                # 2  a is used
#     (if c                     #    the test is part of step 2
#       (recur (array 2))       # 3  b is made   4  b is handed to a
#       nil)))                  # 5  the exit path   6  the exit
(def l2 {:next loop-next
         :alloc [[:a 0] [:b 3]]
         :pass  [[:b :a 4]]
         :use   [[:a 2]]})
(assert (one-state? l2) "a has one owner at the loop head")
(assert (= [[:a 2 3] [:a 2 5]] (releases l2 (ownership l2)))
        "each pass releases its a after the last use")

# (defn l3 [c]
#   (loop [acc (array 0)]       # 0  acc is made   1  the loop head
#     (if c                     # 2  the test
#       (recur (pair 1 acc))    # 3  v is made, with acc as an operand   4  v is handed to acc
#       (length acc))))         # 5  acc is used   6  the exit
(def l3 {:next      loop-next
         :alloc     [[:acc 0] [:v 3]]
         :operand   [[:acc :v 3]]
         :immutable [:v]
         :pass      [[:v :acc 4]]
         :use       [[:acc 3] [:acc 5]]})
(assert (one-state? l3) "acc has one owner at the loop head")
(assert (= [[:acc 5 6]] (releases l3 (ownership l3))) "acc is released after its use on exit")
```

`l4` stores `xs` into `acc` before it recurs on `(rest xs)`, so from the second
pass on, `acc` holds each `xs`. The last Recur clause brings that owner back to
the head, where the fiber holds the first `xs`. So the loop has two states at
its head, and the Join rule's target refuses it. Without the clause, the exit
path would release an `xs` that `acc` holds:

```lisp
# (defn l4 []
#   (def acc @[])               # 0  acc is made
#   (loop [xs (list 1 2)]       # 1  xs is made   2  the loop head
#     (if (pair? xs)            # 3  xs is used
#       (begin (push acc xs)    # 4  xs is stored into acc
#              (recur (rest xs)))   # 5  ys is read out of xs   6  ys is handed to xs
#       (length xs))))          # 7  xs is used   8  the exit
(def l4 {:next  [[0 1] [1 2] [2 3] [3 4] [4 5] [5 6] [6 2] [3 7] [7 8]]
         :alloc [[:acc 0] [:xs 1]]
         :store [[:xs :acc 4]]
         :read  [[:ys :xs 5]]
         :pass  [[:ys :xs 6]]
         :use   [[:xs 3] [:acc 4] [:xs 4] [:xs 5] [:xs 7]]})
(assert (not (one-state? l4)) "xs is in two states at the loop head")
```

In `bk` below, the break hands `a` to `r`, and the block's tail hands `b`. On
the path that does not break, `a` is released at once. `r` has one state, and
is released after its use. Where one path hands `r` a value borrowed from a
parameter and the other a fresh value, as `(or (get m k) default)` does, `r`
has two states, and the Join rule's target refuses it:

```lisp
# (defn bk [c]
#   (def r (block :b
#     (def a (array 1))        # 0  a is made
#     (when c (break :b a))    # 1  the test   2  a is handed to r
#     (array 2)))              # 3  b is made  4  b is handed to r
#   (length r))                # 5  r is used  6  the exit
(def bk {:next  [[0 1] [1 2] [1 3] [2 5] [3 4] [4 5] [5 6]]
         :alloc [[:a 0] [:b 3]]
         :pass  [[:a :r 2] [:b :r 4]]
         :use   [[:r 5]]})
(assert (one-state? bk) "r has one owner where the paths join")
(assert (= [[:a 1 3] [:r 5 6]] (releases bk (ownership bk))) "a is released where it is not handed")

(def bk2 (merge bk {:alloc [[:b 3]] :read [[:a :coll 0]]}))   # a read out of a parameter
(assert (not (one-state? bk2)) "a borrowed value and a fresh one leave r in two states")
```

An eval borrows its datum for the call and makes its result, so it gives
`use(d, p)` for its datum `d` and `alloc(v, p)` for its result `v`. In `ev`
below, `d` is released on the edge after the eval, and `v` after its use. An
eval that passes an environment is not decided
([the ownership rules](ownership.md)):

```lisp
# (defn ev []
#   (def d (list 'array 1 2))   # 0  d is made
#   (def v (eval d))            # 1  d is used, and v is made
#   (length v))                 # 2  v is used
#                               # 3  the exit
(def ev {:next  (steps 3)
         :alloc [[:d 0] [:v 1]]
         :use   [[:d 1] [:v 2]]})
(assert (= [[:d 1 2] [:v 2 3]] (releases ev (ownership ev)))
        "the datum is released after the eval, and the result after its use")
```

## Hand-over at last use

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
