# Release in Datalog

<!-- audited: 2026-11-19 -->

The rules that place each release at a value's last use, and the wait a container adds for what it holds, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts and the rules of who owns each value,
and [the ownership rules](ownership.md) state each rule in words.
[Removal](removal.md) adds the waits a removal needs. The fences here include
the first document, so they run its rules too.

| Derived | Holds when |
|---------|------------|
| `live(v, p)` | a step from `p` on uses `v`, or a value that `v` holds |
| `kept(v, p)` | the fiber still holds `v` at the end of step `p` |
| `refused(v, p)` | `v` is live at step `p`, where the paths leave it in two states |
| `released(v, p, q)` | `v` is released on the edge from step `p` to step `q` |

## The rules

| Rule | Datalog |
|------|---------|
| Unheld value | `live(v, p) :- use(v, p).` |
| Unheld value | `live(v, p) :- next(p, q), live(v, q), not defined(v, p).` where `defined` is `alloc`, `read`, `remove` or `pass` |
| Recur | `live(y, p) :- pass(y, x, p).` |
| Container waits | `live(c, p) :- live(v, p), owned(v, c, p).` where `c` is a container |
| Allocation operand | `live(x, p) :- share(x, v, a), live(v, p).` |
| Join | `kept(v, p) :- owned(v, :fiber, p), not moved(v, :fiber, p).` |
| Join | `kept(v, p) :- alloc(v, p).` and `kept(v, p) :- remove(v, c, p).` |
| Unheld value | `released(v, p, q) :- next(p, q), was(v, p), kept(v, p), not live(v, q).` where `was` is `live`, `alloc` or `remove` |
| Join | `refused(v, p) :- owned(v, :fiber, p), owned(v, c, p), live(v, p).` where `c` is a container |

A release sits on the edge from the last step that needs a value to the first
step that does not. So a branch that never uses a value releases it on entry,
and a value the fiber stored no longer has a release of its own.

A release reads the state at the end of step `p`, on the edge. At a join,
`owned` merges the paths, so a value that one path moves and another keeps is
released on the path that keeps it, at its last use there.

A container stays live while a value it holds is live, and the recursion of
`live` carries that up the chain of owners.

A rule that adds a wait adds a clause for `live`, so it must run inside the
same fixpoint. `liveness` and `releases` therefore take each such rule as a
function. The function adds its variables to the iteration, and returns what to
run in each round.

In the function below, `a` is released after its use at step 3 and on entry to
the `else` branch at step 4, and `b` the other way round:

```lisp
(include-file "datalog.md")

(defn liveness [facts owned & waits]
  "live(v, p) as a relation of [v p], under the Unheld value and Container waits rules,
   and the rules each of WAITS adds."
  (def [next alloc read remove use pass]
    (map (fn [k] (get facts k [])) [:next :alloc :read :remove :use :pass]))
  (def by-target (proof:relation (map (fn [[p q]] [q p]) next)))
  (def made (proof:relation (concat alloc (map (fn [[v c p]] [v p]) remove))))
  (def defined (proof:merge (proof:merge made (proof:relation (map (fn [[v c p]] [v p]) read)))
                            (proof:relation (map (fn [[y x p]] [x p]) pass))))
  (def held-by (proof:relation (map (fn [[p [v o]]] [[v p] o])
                                    (filter (fn [[p [v o]]] (not= o :fiber)) owned))))
  (def it (proof:iteration))
  (def live (proof:variable it "live"))                 # live(v, p) as [p v]
  (def live-at (proof:variable it "live-at"))           # [[v p] true]
  (def live-before (proof:variable it "live-before"))   # [[v p] true]
  (def live-by (proof:variable it "live-by"))           # [v p]
  (def rounds (map (fn [w] (w facts owned it live live-by live-at)) waits))
  # live(y, p) :- pass(y, x, p).
  (proof:extend live (map (fn [[v p]] [p v]) (concat use (map (fn [[y x p]] [y p]) pass))))
  (while (proof:changed? it)
    # live(v, p) :- next(p, q), live(v, q), not defined(v, p).
    (proof:from-join live-before live by-target (fn [q v p] [[v p] true]))
    (proof:from-antijoin live live-before defined (fn [[v p] _] [p v]))
    # live(c, p) :- live(v, p), owned(v, c, p).
    (proof:from-map live-at live (fn [[p v]] [[v p] true]))
    (proof:from-join live live-at held-by (fn [[v p] _ c] [p c]))
    (proof:from-map live-by live (fn [[p v]] [v p]))
    (each round in rounds (round)))
  (proof:relation (map (fn [[p v]] [v p]) (proof:elements (proof:complete live)))))

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
  (def candidates (proof:join was-kept (proof:relation next) (fn [p v q] [[v q] p])))
  (proof:elements (proof:antijoin candidates live-r (fn [[v q] p] [v p q]))))

(defn releases [facts owned & waits]
  "released(v, p, q) as [v p q], under the rules liveness states and each of WAITS."
  (released-from facts owned (liveness facts owned ;waits)))

# (defn g [n]
#   (def a (array 1 2))   # 0  a is made
#   (def b (array 3))     # 1  b is made
#   (if (> n 0)           # 2  the test
#     (length a)          # 3  a is used
#     (length b)))        # 4  b is used
#                         # 5  the exit
(def g {:next  [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5]]
        :alloc [[:a 0] [:b 1]]
        :use   [[:a 3] [:b 4]]})
(assert (= [[:a 2 4] [:a 3 5] [:b 2 3] [:b 4 5]] (releases g (ownership g)))
        "each value is released on every edge past its last use")
```

In the function below, step 5 is the last step that uses `a` itself. The value
`f` is read out of `e`, and `e` out of `a`, so `a` waits for the use of `f` at
step 7 and is released after it:

```lisp
# (defn h []
#   (def a @[])          # 0  a is made
#   (def b @[])          # 1  b is made
#   (def c (array 1))    # 2  c is made
#   (push b c)           # 3  c is stored into b
#   (push a b)           # 4  b is stored into a
#   (def e (get a 0))    # 5  e is read out of a
#   (def f (get e 0))    # 6  f is read out of e
#   (length f))          # 7  f is used
#                        # 8  the exit
(def h {:next  (map (fn [p] [p (+ p 1)]) (range 8))
        :alloc [[:a 0] [:b 1] [:c 2]]
        :read  [[:e :a 5] [:f :e 6]]
        :store [[:c :b 3] [:b :a 4]]
        :use   [[:b 3] [:c 3] [:a 4] [:b 4] [:a 5] [:e 6] [:f 7]]})
(assert (= [[:a 7 8]] (releases h (ownership h)))
        "a container is released after the last use of a value it holds")
```

In the function below, `v` borrows `x`, which `m` holds, so `x` stays live
while `v` is, and `m` waits for the use of `v` at step 6:

```lisp
(defn operand [facts owned it live live-by live-at]
  "The clause for live under the Allocation operand rule, for releases."
  (def share-by (proof:relation (map (fn [[x v a]] [v x]) (get (operands facts) 0))))
  (fn []
    # live(x, p) :- share(x, v, a), live(v, p).
    (proof:from-join live live-by share-by (fn [v p x] [p x]))))

# (defn p []
#   (def m @[])          # 0  m is made
#   (def y @[1])         # 1  y is made
#   (push m y)           # 2  y is stored into m
#   (def x (get m 0))    # 3  x is read out of m
#   (def v (pair x ()))  # 4  v is made, with x as an operand
#   (length m)           # 5  m is used
#   (length (first v)))  # 6  v is used
#                        # 7  the exit
(def p {:next      (map (fn [p] [p (+ p 1)]) (range 7))
        :alloc     [[:m 0] [:y 1] [:v 4]]
        :read      [[:x :m 3]]
        :store     [[:y :m 2]]
        :operand   [[:x :v 4]]
        :immutable [:v]
        :use       [[:m 2] [:y 2] [:m 3] [:x 4] [:m 5] [:v 6]]})
(assert (= [[:m 6 7] [:v 6 7]] (releases p (ownership p) operand))
        "an immutable value keeps the owner of its operand waiting")
```

In the function below, the `then` arm stores `b` into `a`, and the `else` arm
keeps it. `b` is released on the edge out of the `else` arm alone:

```lisp
# (defn j [n]
#   (def a @[])          # 0  a is made
#   (def b (array 1))    # 1  b is made
#   (if (> n 0)          # 2  the test
#     (push a b)         # 3  b is stored into a
#     (length b))        # 4  b is used
#   (length a))          # 5  a is used, where the paths join
#                        # 6  the exit
(def j {:next  [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5] [5 6]]
        :alloc [[:a 0] [:b 1]]
        :store [[:b :a 3]]
        :use   [[:a 3] [:b 3] [:b 4] [:a 5]]})
(assert (= [[:a 5 6] [:b 4 5]] (releases j (ownership j)))
        "a value one path stores is released on the path that keeps it")
```

A value live where one path leaves the fiber holding it and another leaves a
container holding it is refused. The refusal is a target
([the judgment](judgment.md)). A lowering that copies the code after the join
once per state would accept it, and so would a split of each step by state in
these rules. Below, `j2` uses `b` after the join, and the loop in `l` uses on
its next pass a value it stored on the last:

```lisp
(defn two-states [facts owned live-r]
  "refused(v, p) as [v p], under the Join rule."
  (def fiber (proof:map (fiber-held owned) (fn [vp] [vp true])))
  (def held (proof:relation (map (fn [[p [v o]]] [[v p] o])
                                 (filter (fn [[p [v o]]] (not= o :fiber)) owned))))
  # refused(v, p) :- owned(v, :fiber, p), owned(v, c, p), live(v, p).
  (def both (proof:join fiber held (fn [vp _ c] [vp true])))
  (proof:elements (proof:join both (proof:map live-r (fn [vp] [vp true])) (fn [vp _ _] vp))))

(def j2 (put j :use [[:a 3] [:b 3] [:b 4] [:a 5] [:b 5]]))   # (length b) at 5
(assert (= [[:b 5]] (two-states j2 (ownership j2) (liveness j2 (ownership j2))))
        "a value live after a join in two states is refused")
(assert (empty? (two-states j (ownership j) (liveness j (ownership j))))
        "a value dead after the join is not")

# (defn l [xs]
#   (def acc @[])        # 0  acc is made
#   (def v (array 1))    # 1  v is made
#   (each x in xs        # 2  the loop head
#     (push acc v)       # 3  v is stored into acc
#     (length v))        # 4  v is used, and the loop goes back to 2
#   (length acc))        # 5  acc is used
(def l {:next  [[0 1] [1 2] [2 3] [3 4] [4 2] [2 5]]
        :alloc [[:acc 0] [:v 1]]
        :store [[:v :acc 3]]
        :use   [[:acc 3] [:v 3] [:v 4] [:acc 5]]})
(assert (= [[:v 2] [:v 3]] (two-states l (ownership l) (liveness l (ownership l))))
        "a loop that stores a value and uses it again is refused at its head")
```

## Not written yet

- Borrow before release. It refuses nothing until a rule adds a borrow whose
  owner cannot wait. [Capture](capture.md) adds one.
