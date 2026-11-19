# Removal in Datalog

<!-- audited: 2026-11-19 -->

How the release of a value taken out of a container waits for each value and reference that may reach it, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts, and how a step names a value no binding
receives. [Release](release.md) places each release at a value's last use, and
the rules here add to `live`. [The ownership rules](ownership.md) state each
rule in words.

| Derived | Holds when |
|---------|------------|
| `ref(e, c)` | a store made a reference from `c` to `e` under the Store inside a unit rule |
| `lives(c, p)` | `c`, or a container `c` dies with, is live at `p` |

## The rules

| Rule | Datalog |
|------|---------|
| Removal | `live(v, p) :- remove(v, c, p0), owned(e, c, p0), live(e, p), owned(v, o, p).` |
| Internal reference | `ref(e, c) :- store(e, c, s), not owned(e, :fiber, s).` |
| Internal reference | `lives(c, p) :- live(c, p).` |
| Internal reference | `lives(c, p) :- dies(c, w, p), live(w, p).` |
| Internal reference | `live(v, p) :- remove(v, d, p0), owned(e, d, p0), ref(e, c), lives(c, p), owned(v, o, p).` |

A read names a new value, so any value that `c` holds at a removal from `c`
may be the value removed. A use of it counts toward the removed value's last
use, from the removal on. A container that holds a reference to a value that
may be the removed one makes it wait as well, until that container dies.

In the function below, `e` is read out of `a` before the removal at step 4, so
`e` may be `r`, and `r` is released after the use of `e` at step 6:

```lisp
(include-file "release.md")

(defn removal [facts owned it live live-by live-at]
  "The clauses for live under the Removal and Internal reference rules, for releases."
  (def [store remove] (map (fn [k] (get facts k [])) [:store :remove]))
  (def fiber-at (proof:relation (map (fn [[p [v o]]] [[v p] true])
                                     (filter (fn [[p [v o]]] (= o :fiber)) owned))))
  # may(e, v): c holds e where v is removed from c, so e may be v.
  (def may (proof:join (proof:relation (map (fn [[v c p0]] [[c p0] v]) remove))
                       (proof:relation (map (fn [[p [v o]]] [[o p] v]) owned))
                       (fn [_ v e] [e v])))
  # ref(e, c) :- store(e, c, s), not owned(e, :fiber, s).
  (def ref (proof:antijoin (proof:relation (map (fn [[e c s]] [[e s] c]) store)) fiber-at
                           (fn [[e s] c] [e c])))
  (def waits (proof:join may ref (fn [e v c] [c v])))   # v waits while c lives
  (def dies-into (proof:relation (map (fn [[[c p] w]] [[w p] c]) (units owned))))
  (def has-owner (proof:relation (map (fn [[p [v o]]] [[v p] true]) owned)))
  (def may-live (proof:variable it "may-live"))         # [[v p] true]
  (def lives (proof:variable it "lives"))               # lives(c, p) as [c p]
  (fn []
    # live(v, p) :- remove(v, c, p0), owned(e, c, p0), live(e, p), owned(v, o, p).
    (proof:from-join may-live live-by may (fn [e p v] [[v p] true]))
    (proof:from-join live may-live has-owner (fn [[v p] _ _] [p v]))
    # lives(c, p) :- live(c, p).  lives(c, p) :- dies(c, w, p), live(w, p).
    (proof:from-map lives live-by (fn [[c p]] [c p]))
    (proof:from-join lives live-at dies-into (fn [[w p] _ c] [c p]))
    # live(v, p) :- remove(v, d, p0), owned(e, d, p0), ref(e, c), lives(c, p), owned(v, o, p).
    (proof:from-join may-live lives waits (fn [c p v] [[v p] true]))))

# (defn k []
#   (def a @[])          # 0  a is made
#   (def b (array 1))    # 1  b is made
#   (push a b)           # 2  b is stored into a
#   (def e (get a 0))    # 3  e is read out of a
#   (def r (pop a))      # 4  r is taken out of a
#   (length a)           # 5  a is used
#   (length e))          # 6  e is used
#                        # 7  the exit
(def k {:next   (map (fn [p] [p (+ p 1)]) (range 7))
        :alloc  [[:a 0] [:b 1]]
        :read   [[:e :a 3]]
        :store  [[:b :a 2]]
        :remove [[:r :a 4]]
        :use    [[:a 2] [:b 2] [:a 3] [:a 4] [:a 5] [:e 6]]})
(assert (= [[:a 6 7] [:r 6 7]] (releases k (ownership k) removal))
        "a removed value is released after the last use of a value that may be it")
```

In the function below, no binding receives what the `put` and the `del` take
out, so each step names it. `[:out 2]` has no use and is released at once. `f`
may be `[:out 4]`, so `[:out 4]` waits for the use of `f` at step 5:

```lisp
# (defn s []
#   (def t @{})          # 0  t is made
#   (def x (array 1))    # 1  x is made
#   (put t :k x)         # 2  x is stored into t, and [:out 2] is taken out
#   (def f (get t :k))   # 3  f is read out of t
#   (del t :k)           # 4  [:out 4] is taken out of t
#   (length f))          # 5  f is used
#                        # 6  the exit
(def s {:next   (map (fn [p] [p (+ p 1)]) (range 6))
        :alloc  [[:t 0] [:x 1]]
        :read   [[:f :t 3]]
        :store  [[:x :t 2]]
        :remove [[[:out 2] :t 2] [[:out 4] :t 4]]
        :use    [[:t 2] [:x 2] [:t 3] [:t 4] [:f 5]]})
(assert (= [[:t 5 6] [[:out 2] 2 3] [[:out 4] 5 6]] (releases s (ownership s) removal))
        "a value no binding receives is released after the last use of a value that may be it")
```

In the function below, the store at step 8 makes a reference from `c` to `e`,
which `d` holds. `e` may be `[:out 9]`, so `[:out 9]` waits until `c` dies with
`w`, after step 12, though `g` reads it out of `c` for the last time at step 11:

```lisp
# (defn u []
#   (def w @[])          # 0  w is made
#   (def c @[])          # 1  c is made
#   (push w c)           # 2  c is stored into w
#   (def d @[])          # 3  d is made
#   (push c d)           # 4  d is stored into c
#   (def x (array 1))    # 5  x is made
#   (push d x)           # 6  x is stored into d
#   (def e (get d 0))    # 7  e is read out of d
#   (push c e)           # 8  e is stored into c, inside the unit
#   (remove d 0)         # 9  [:out 9] is taken out of d
#   (def g (get c 1))    # 10 g is read out of c
#   (length g)           # 11 g is used
#   (length w))          # 12 w is used
#                        # 13 the exit
(def u {:next   (map (fn [p] [p (+ p 1)]) (range 13))
        :alloc  [[:w 0] [:c 1] [:d 3] [:x 5]]
        :read   [[:e :d 7] [:g :c 10]]
        :store  [[:c :w 2] [:d :c 4] [:x :d 6] [:e :c 8]]
        :remove [[[:out 9] :d 9]]
        :use    [[:w 2] [:c 2] [:c 4] [:d 4] [:d 6] [:x 6] [:d 7] [:c 8] [:e 8] [:d 9]
                 [:c 10] [:g 11] [:w 12]]})
(assert (= [[:w 12 13] [[:out 9] 12 13]] (releases u (ownership u) removal))
        "a removed value waits for the death of each container that refers to it")
```

In the function below, the store at step 2 moves `y`, which the fiber holds,
so it makes no reference. `[:out 3]` has no use and is released at once, while
`m` lives on:

```lisp
# (defn q []
#   (def m @[])          # 0  m is made
#   (def y (array 1))    # 1  y is made
#   (push m y)           # 2  y is stored into m
#   (remove m 0)         # 3  [:out 3] is taken out of m
#   (length m))          # 4  m is used
#                        # 5  the exit
(def q {:next   (map (fn [p] [p (+ p 1)]) (range 5))
        :alloc  [[:m 0] [:y 1]]
        :store  [[:y :m 2]]
        :remove [[[:out 3] :m 3]]
        :use    [[:m 2] [:y 2] [:m 3] [:m 4]]})
(assert (= [[:m 4 5] [[:out 3] 3 4]] (releases q (ownership q) removal))
        "a store of a value the fiber holds makes no reference")
```
