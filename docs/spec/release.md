# Release in Datalog

<!-- audited: 2026-11-23 -->

The rules that place each release at a value's last use, as Datalog that runs on proof.

[Liveness](liveness.md) holds `live` and each wait that keeps a value live,
and [Datalog](datalog.md) holds the facts and the rules of who owns each value.
[The ownership rules](ownership.md) state each rule in words.
[Removal](removal.md) adds the waits a removal needs.

| Derived | Holds when |
|---------|------------|
| `kept(v, p)` | the fiber still holds `v` at the end of step `p` |
| `refused(v, p)` | `v` is live at step `p`, where the paths leave it in two states or a step makes it again |
| `released(v, p, q)` | `v` is released on the edge from step `p` to step `q` |

## The rules

| Rule | Datalog |
|------|---------|
| Join | `kept(v, p) :- owned(v, :fiber, p), not moved(v, :fiber, p).` |
| Join | `kept(v, p) :- alloc(v, p).` and `kept(v, p) :- remove(v, c, p).` |
| Unheld value | `released(v, p, q) :- next(p, q), was(v, p), kept(v, p), not live(v, q).` where `was` is `live`, `alloc` or `remove` |
| [Unused parameter](unused.md) | `released(x, p, q) :- pass(y, x, p), owned(y, :fiber, p), next(p, q), not live(x, q).` where `x` is not `[:sink i p]`, `[:owner x p]` or `[:cross p]` |
| Join | `refused(v, p) :- owned(v, :fiber, p), owned(v, c, p), live(v, p).` where `c` is a container |
| Made again while live | `refused(v, p) :- made(v, p), live(v, p).` where `made` is `alloc`, `read` or `remove` |

A release sits on the edge from the last step that needs a value to the first
step that does not. So a branch that never uses a value releases it on entry,
and a value the fiber stored no longer has a release of its own.

A release reads the state at the end of step `p`, on the edge. At a join,
`owned` merges the paths, so a value that one path moves and another keeps is
released on the path that keeps it, at its last use there.

`releases` takes the waits as `liveness` does, and passes them on.

In `g`, `a` is released after its use at step 3 and on entry to the `else`
branch at step 4, and `b` the other way round. In `j`, the `then` arm stores
`b` into `a`, and the `else` arm keeps it. `b` is released on the edge out of
the `else` arm alone.

A value live where one path leaves the fiber holding it and another leaves a
container holding it is refused. The refusal is a target
([the judgment](judgment.md)). A lowering that copies the code after the join
once per state would accept it, and so would a split of each step by state in
these rules. `j2` uses `b` after the join, and the loop in `l` uses on its next
pass a value it stored on the last.

A step that makes a value again while the value it made before is still live
is refused, and the refusal is a target too. A jump makes no value here: the
new value it hands over may be read out of the old one, which then rightly
stays live, as in a walk down a list. In `lb`, each pass makes `m`, reads `h`
out of it and hands `y`, which borrows `h`, back to `x`. The next pass uses `x`
after it makes `m` again, so the old `m` is live where the new one is made.
`ga` builds a list whose new pair holds the old one and borrows from this
pass's `parts`.

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
  (def {:operands operands :fiber-held fiber-held :ownership ownership :steps steps}
    ((import "./datalog.md")))
  (def {:liveness liveness :operand operand} ((import "./liveness.md")))

  (defn released-from [facts owned live-r]
    "released(v, p, q) as [v p q], from the relation LIVE-R that liveness returns."
    (def [next alloc remove] (map (fn [k] (get facts k [])) [:next :alloc :remove]))
    (def made (proof:relation (concat alloc (map (fn [[v c p]] [v p]) remove))))
    (def [share stores] (operands facts))
    (def moved (proof:relation (concat (map (fn [[v c p]] [v p]) (concat stores share))
                                       (mapcat (fn [[y x p]] [[y p] [x p]]) (get facts :pass [])))))
    # kept(v, p) :- owned(v, :fiber, p), not moved(v, :fiber, p).  kept(v, p) :- made(v, p).
    (def fiber-at (proof:map (fiber-held owned) (fn [vp] [vp true])))
    (def kept (proof:merge made (proof:antijoin fiber-at moved (fn [vp _] vp))))
    # released(v, p, q) :- next(p, q), was(v, p), kept(v, p), not live(v, q).
    (def was-kept (proof:join (proof:map (proof:merge live-r made) (fn [vp] [vp true]))
                              (proof:map kept (fn [vp] [vp true])) (fn [[v p] _ _] [p v])))
    # released(x, p, q) :- pass(y, x, p), owned(y, :fiber, p), next(p, q), not live(x, q).
    (def jumps (filter (fn [[y x p]] (not (and (array? x) (has? |:sink :owner :cross| (get x 0)))))
                       (get facts :pass [])))
    (def handed (proof:join (proof:relation (map (fn [[y x p]] [[y p] x]) jumps)) fiber-at
                            (fn [[y p] x _] [p x])))
    (def candidates (proof:merge (proof:join was-kept (proof:relation next) (fn [p v q] [[v q] p]))
                                 (proof:join handed (proof:relation next) (fn [p x q] [[x q] p]))))
    (proof:elements (proof:antijoin candidates live-r (fn [[v q] p] [v p q]))))

  (defn releases [facts owned & waits]
    "released(v, p, q) as [v p q], under the rules liveness states and each of WAITS."
    (released-from facts owned (liveness facts owned ;waits)))

  (defn two-states [facts owned live-r]
    "refused(v, p) as [v p], under the Join rule."
    (def fiber (proof:map (fiber-held owned) (fn [vp] [vp true])))
    (def held (proof:relation (map (fn [[p [v o]]] [[v p] o])
                                   (filter (fn [[p [v o]]] (not= o :fiber)) owned))))
    # refused(v, p) :- owned(v, :fiber, p), owned(v, c, p), live(v, p).
    (def both (proof:join fiber held (fn [vp _ c] [vp true])))
    (proof:elements (proof:join both (proof:map live-r (fn [vp] [vp true])) (fn [vp _ _] vp))))

  (defn made-again [facts live-r]
    "refused(v, p) as [v p], under the Made again while live rule."
    (def made (proof:relation (concat (get facts :alloc []) (map (fn [[v c p]] [v p]) (get facts :read []))
                                      (map (fn [[v c p]] [v p]) (get facts :remove [])))))
    # refused(v, p) :- made(v, p), live(v, p).
    (proof:elements (proof:join (proof:map made (fn [vp] [vp true])) (proof:map live-r (fn [vp] [vp true]))
                                (fn [vp _ _] vp))))

  (defn again [facts] (made-again facts (liveness facts (ownership facts) operand)))

  (def g (fence 'g :doc "Each value is released on every edge past its last use."
    :source "(defn g [n]
  (def a (array 1 2))   # 0  a is made
  (def b (array 3))     # 1  b is made
  (if (> n 0)           # 2  the test
    (length a)          # 3  a is used
    (length b)))        # 4  b is used
                        # 5  the exit"
    :facts {:next  [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5]]
            :alloc [[:a 0] [:b 1]]
            :use   [[:a 3] [:b 4]]}
    :check (fn [g] (= [[:a 2 4] [:a 3 5] [:b 2 3] [:b 4 5]] (releases g (ownership g))))))

  (def j (fence 'j :doc "A value one path stores is released on the path that keeps it."
    :source "(defn j [n]
  (def a @[])          # 0  a is made
  (def b (array 1))    # 1  b is made
  (if (> n 0)          # 2  the test
    (push a b)         # 3  b is stored into a
    (length b))        # 4  b is used
  (length a))          # 5  a is used, where the paths join
                       # 6  the exit"
    :facts {:next  [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5] [5 6]]
            :alloc [[:a 0] [:b 1]]
            :store [[:b :a 3]]
            :use   [[:a 3] [:b 3] [:b 4] [:a 5]]}
    :check (fn [j] (= [[:a 5 6] [:b 4 5]] (releases j (ownership j))))))

  (def j2 (fence 'j2
    :doc "A value live after a join in two states is refused, and the value dead after it is not."
    :facts (put j:facts :use [[:a 3] [:b 3] [:b 4] [:a 5] [:b 5]])   # (length b) at 5
    :check (fn [j2] (and (= [[:b 5]] (two-states j2 (ownership j2) (liveness j2 (ownership j2))))
                         (empty? (two-states j:facts (ownership j:facts)
                                             (liveness j:facts (ownership j:facts))))))))

  (def l (fence 'l :doc "A loop that stores a value and uses it again is refused at its head."
    :source "(defn l [xs]
  (def acc @[])        # 0  acc is made
  (def v (array 1))    # 1  v is made
  (each x in xs        # 2  the loop head
    (push acc v)       # 3  v is stored into acc
    (length v))        # 4  v is used, and the loop goes back to 2
  (length acc))        # 5  acc is used"
    :facts {:next  [[0 1] [1 2] [2 3] [3 4] [4 2] [2 5]]
            :alloc [[:acc 0] [:v 1]]
            :store [[:v :acc 3]]
            :use   [[:acc 3] [:v 3] [:v 4] [:acc 5]]}
    :check (fn [l] (= [[:v 2] [:v 3]] (two-states l (ownership l) (liveness l (ownership l)))))))

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

  {:released-from released-from :releases releases :two-states two-states
   :made-again made-again :again again :fences [g j j2 l lb ga walk]})
```
