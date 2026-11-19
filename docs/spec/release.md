# Release in Datalog

<!-- audited: 2026-11-19 -->

The rules that place each release at a value's last use, and the waits a borrow adds, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts and the rules of who owns each value,
and [the ownership rules](ownership.md) state each rule in words. The fences
here include that document, so they run its rules too.

| Derived | Holds when |
|---------|------------|
| `live(v, p)` | a step from `p` on uses `v`, or a value that `v` holds |
| `released(v, p, q)` | `v` is released on the edge from step `p` to step `q` |

## The rules

| Rule | Datalog |
|------|---------|
| Unheld value | `live(v, p) :- use(v, p).` |
| Unheld value | `live(v, p) :- next(p, q), live(v, q), not defined(v, p).` where `defined` is `alloc`, `read` or `remove` |
| Container waits | `live(c, p) :- live(v, p), owned(v, c, p).` where `c` is a container |
| Removal | `live(v, p) :- remove(v, c, p0), owned(e, c, p0), live(e, p), owned(v, o, p).` |
| Unheld value | `released(v, p, q) :- next(p, q), was(v, p), owned(v, :fiber, q), not live(v, q).` where `was` is `live`, `alloc` or `remove` |

A release sits on the edge from the last step that needs a value to the first
step that does not. So a branch that never uses a value releases it on entry,
and a value the fiber stored no longer has a release of its own.

A container stays live while a value it holds is live, and the recursion of
`live` carries that up the chain of owners. A read names a new value, so any
value that `c` holds at a removal from `c` may be the value removed. A use of
it counts toward the removed value's last use, from the removal on.

In the function below, `a` is released after its use at step 3 and on entry to
the `else` branch at step 4, and `b` the other way round:

```lisp
(include-file "datalog.md")

(defn releases [facts owned]
  "released(v, p, q) as [v p q], under the Unheld value, Container waits and Removal rules."
  (def [next alloc read remove use]
    (map (fn [k] (get facts k [])) [:next :alloc :read :remove :use]))
  (def next-r (proof:relation next))
  (def by-target (proof:relation (map (fn [[p q]] [q p]) next)))
  (def made (proof:relation (concat alloc (map (fn [[v c p]] [v p]) remove))))
  (def defined (proof:merge made (proof:relation (map (fn [[v c p]] [v p]) read))))
  (def held-by (proof:relation (map (fn [[p [v o]]] [[v p] o])
                                    (filter (fn [[p [v o]]] (not= o :fiber)) owned))))
  # may(e, v): c holds e where v is removed from c, so e may be v.
  (def may (proof:join (proof:relation (map (fn [[v c p0]] [[c p0] v]) remove))
                       (proof:relation (map (fn [[p [v o]]] [[o p] v]) owned))
                       (fn [_ v e] [e v])))
  (def has-owner (proof:relation (map (fn [[p [v o]]] [[v p] true]) owned)))
  (def it (proof:iteration))
  (def live (proof:variable it "live"))                 # live(v, p) as [p v]
  (def live-at (proof:variable it "live-at"))           # [[v p] true]
  (def live-before (proof:variable it "live-before"))   # [[v p] true]
  (def live-by (proof:variable it "live-by"))           # [v p]
  (def may-live (proof:variable it "may-live"))         # [[v p] true]
  (proof:extend live (map (fn [[v p]] [p v]) use))
  (while (proof:changed? it)
    # live(v, p) :- next(p, q), live(v, q), not defined(v, p).
    (proof:from-join live-before live by-target (fn [q v p] [[v p] true]))
    (proof:from-antijoin live live-before defined (fn [[v p] _] [p v]))
    # live(c, p) :- live(v, p), owned(v, c, p).
    (proof:from-map live-at live (fn [[p v]] [[v p] true]))
    (proof:from-join live live-at held-by (fn [[v p] _ c] [p c]))
    # live(v, p) :- remove(v, c, p0), owned(e, c, p0), live(e, p), owned(v, o, p).
    (proof:from-map live-by live (fn [[p v]] [v p]))
    (proof:from-join may-live live-by may (fn [e p v] [[v p] true]))
    (proof:from-join live may-live has-owner (fn [[v p] _ _] [p v])))
  (def live-r (proof:relation (map (fn [[p v]] [v p]) (proof:elements (proof:complete live)))))
  # released(v, p, q) :- next(p, q), was(v, p), owned(v, :fiber, q), not live(v, q).
  (def candidates
    (proof:join (proof:relation (map (fn [[v p]] [p v]) (proof:elements (proof:merge live-r made))))
                next-r (fn [p v q] [[v q] p])))
  (def fiber-at (proof:relation (map (fn [[p [v o]]] [[v p] true])
                                     (filter (fn [[p [v o]]] (= o :fiber)) owned))))
  (def held (proof:join candidates fiber-at (fn [vq p _] [vq p])))
  (proof:elements (proof:antijoin held live-r (fn [[v q] p] [v p q]))))

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

In the function below, `e` is read out of `a` before the removal at step 4, so
`e` may be `r`, and `r` is released after the use of `e` at step 6:

```lisp
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
(assert (= [[:a 6 7] [:r 6 7]] (releases k (ownership k)))
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
(assert (= [[:t 5 6] [[:out 2] 2 3] [[:out 4] 5 6]] (releases s (ownership s)))
        "a value no binding receives is released after the last use of a value that may be it")
```

## Not written yet

- Every other rule that moves a release: Borrow before release, Internal
  reference, Allocation operand and the capture rules.
