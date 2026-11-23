# Made again while live, in Datalog

<!-- audited: 2026-11-23 -->

How a step that makes a value again while the value it made before is still live is refused, as Datalog that runs on proof.

[Liveness](liveness.md) holds `live`, and [the ownership rules](ownership.md)
state the rule in words. [Made anew](anew.md) ends each hold on the value made
before.

| Derived | Holds when |
|---------|------------|
| `refused(v, p)` | step `p` makes `v` while the value it made before is live |

## The rule

| Rule | Datalog |
|------|---------|
| Made again while live | `refused(v, p) :- made(v, p), live(v, p).` where `made` is `alloc`, `read` or `remove` |

The refusal is a target ([the judgment](judgment.md)). A jump makes no value
here: the new value it hands over may be read out of the old one, which then
rightly stays live, as in a walk down a list. In `lb`, each pass makes `m`,
reads `h` out of it and hands `y`, which borrows `h`, back to `x`. The next
pass uses `x` after it makes `m` again, so the old `m` is live where the new
one is made. `ga` builds a list whose new pair holds the old one and borrows
from this pass's `parts`.

The two loops differ in what a lowering could do. In `lb`, the value handed
back borrows only from the `m` of its own pass. So the old `m` could travel the
back edge as one extra owner, and be released in the next pass after the last
use of `x`. In `ga`, the handed pair holds the old `acc`, which borrows from
the `parts` before it, so the final `acc` reaches every `parts` made so far.
After k passes, k owners must stay alive, and a block parameter names one
value. Holding them would take a count of owners at run time, or a merge of
each `parts` into one region:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence} ((import "../spec.md")))
  (def {:ownership ownership} ((import "./datalog.md")))
  (def {:liveness liveness :operand operand} ((import "./liveness.md")))

  (defn made-again [facts live-r]
    "refused(v, p) as [v p], under the Made again while live rule."
    (def made (proof:relation (concat (get facts :alloc []) (map (fn [[v c p]] [v p]) (get facts :read []))
                                      (map (fn [[v c p]] [v p]) (get facts :remove [])))))
    # refused(v, p) :- made(v, p), live(v, p).
    (proof:elements (proof:join (proof:map made (fn [vp] [vp true])) (proof:map live-r (fn [vp] [vp true]))
                                (fn [vp _ _] vp))))

  (defn again [facts] (made-again facts (liveness facts (ownership facts) operand)))

  (def lb (fence 'lb :doc "m and h are made again while the old ones are live."
    :source "(defn lb [c]
  (def i (pair 0 ()))         # 0  i is made
  (def @x i)                  # 1  i jumps to x
  (while true                 # 2  the loop head
    (let [m @[@[1]]           # 3  m is made
          h (get m 0)         # 4  h is read out of m
          y (pair h ())]      # 5  y is made, with h as an operand
      (length (first x))      # 6  x is used, and the test
      (if c (assign x y)      # 7  y jumps to x
          (break))))          # 8  the exit path
  nil)                        # 9  the exit"
    :facts {:next      [[0 1] [1 2] [2 3] [3 4] [4 5] [5 6] [6 7] [7 2] [6 8] [8 9]]
            :alloc     [[:i 0] [:m 3] [:y 5]]
            :read      [[:h :m 4]]
            :operand   [[:h :y 5]]
            :immutable [:i :y]
            :pass      [[:i :x 1] [:y :x 7]]
            :use       [[:m 4] [:h 5] [:x 6]]}
    :check (fn [lb] (= [[:m 3] [:h 4]] (again lb)))))

  (def ga (fence 'ga :doc "Under a growing list, parts and h are made again while live."
    :source "(defn ga [c]
  (def e (pair 0 ()))         # 0  e is made
  (def @acc e)                # 1  e jumps to acc
  (while true                 # 2  the loop head
    (let [parts @[@[1]]       # 3  parts is made
          h (get parts 0)     # 4  h is read out of parts
          n (pair h acc)]     # 5  n is made, with h and acc as operands
      (if c (assign acc n)    # 6  the test   7  n jumps to acc
          (break))))
  (length acc))               # 8  acc is used   9  the exit"
    :facts {:next      [[0 1] [1 2] [2 3] [3 4] [4 5] [5 6] [6 7] [7 2] [6 8] [8 9]]
            :alloc     [[:e 0] [:parts 3] [:n 5]]
            :read      [[:h :parts 4]]
            :operand   [[:h :n 5] [:acc :n 5]]
            :immutable [:e :n]
            :pass      [[:e :acc 1] [:n :acc 7]]
            :use       [[:parts 4] [:h 5] [:acc 5] [:acc 8]]}
    :check (fn [ga] (= [[:parts 3] [:h 4]] (again ga)))))

  (def walk (fence 'walk :doc "A value a jump hands over is not made again."
    :source "(defn walk []
  (def @xs (list 1 2 3))      # 0  xs is made
  (while (pair? xs)           # 1  the loop head   2  xs is used
    (assign xs (rest xs)))    # 3  ys is read out of xs   4  ys jumps to xs
  nil)                        # 5  the exit path   6  the exit"
    :facts {:next [[0 1] [1 2] [2 3] [3 4] [4 1] [2 5] [5 6]] :alloc [[:xs 0]]
            :read [[:ys :xs 3]] :pass [[:ys :xs 4]] :use [[:xs 2] [:xs 3]]}
    :check (fn [walk] (empty? (again walk)))))

  {:made-again made-again :again again :fences [lb ga walk]})
```
