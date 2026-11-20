# Liveness in Datalog

<!-- audited: 2026-11-20 -->

The rules that keep a value live until its last use, and the waits that keep it live longer, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts and the rules of who owns each value,
and [the ownership rules](ownership.md) state each rule in words.
[Release](release.md) releases a value on the edge where it stops being live.
The fences here include the first document, so they run its rules too.

| Derived | Holds when |
|---------|------------|
| `live(v, p)` | a step from `p` on uses `v`, or a value that `v` holds |

## The rules

| Rule | Datalog |
|------|---------|
| Unheld value | `live(v, p) :- use(v, p).` |
| Unheld value | `live(v, p) :- next(p, q), live(v, q), not defined(v, p).` where `defined` is `alloc`, `read`, `remove` or `pass` |
| Recur | `live(y, p) :- pass(y, x, p).` |
| Container waits | `live(c, p) :- live(v, p), owned(v, c, p).` where `c` is a container |
| Allocation operand | `live(x, p) :- share(x, v, a), live(v, p).` |

A container stays live while a value it holds is live, and the recursion of
`live` carries that up the chain of owners.

A rule that adds a wait adds a clause for `live`, so it must run inside the
same fixpoint. `liveness` therefore takes each such rule as a function. The
function adds its variables to the iteration, and returns what to run in each
round.

In the function below, step 5 is the last step that uses `a` itself. The value
`f` is read out of `e`, and `e` out of `a`, so `a` stays live until the use of
`f` at step 7:

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

(defn live-steps [live-r v]
  "The steps at which V is live in LIVE-R, the relation liveness returns."
  (sort (map (fn [[w p]] p) (filter (fn [[w p]] (= w v)) (proof:elements live-r)))))

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
(def h {:next  (steps 8)
        :alloc [[:a 0] [:b 1] [:c 2]]
        :read  [[:e :a 5] [:f :e 6]]
        :store [[:c :b 3] [:b :a 4]]
        :use   [[:b 3] [:c 3] [:a 4] [:b 4] [:a 5] [:e 6] [:f 7]]})
(assert (= [1 2 3 4 5 6 7] (live-steps (liveness h (ownership h)) :a))
        "a container stays live until the last use of a value it holds")
```

In the function below, `v` borrows `x`, which `m` holds, so `x` stays live
while `v` is, and `m` stays live until the use of `v` at step 6:

```lisp
(defn operand [facts owned it live live-by live-at]
  "The clause for live under the Allocation operand rule, for liveness."
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
(def p {:next      (steps 7)
        :alloc     [[:m 0] [:y 1] [:v 4]]
        :read      [[:x :m 3]]
        :store     [[:y :m 2]]
        :operand   [[:x :v 4]]
        :immutable [:v]
        :use       [[:m 2] [:y 2] [:m 3] [:x 4] [:m 5] [:v 6]]})
(assert (= [1 2 3 4 5 6] (live-steps (liveness p (ownership p) operand) :m))
        "an immutable value keeps the owner of its operand live")
```
