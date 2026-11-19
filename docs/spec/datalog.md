# The rules in Datalog

<!-- audited: 2026-11-19 -->

The facts the ownership rules read from a function in kernel form, and each rule written as Datalog that runs on proof.

[The ownership rules](ownership.md) state each rule in words, and
[the judgment](judgment.md) says how the rules decide acceptance. The rules
below run on [proof](../stdlib/proof.md), so the fence that shows a rule also
tests it.

## Points, values and owners

A point is one step of a function in kernel form, and
[HIR](../impl/hir.md) lists the kernel. A value is named by the binding that
receives it. A read names a new value, because the rules cannot know which
element a read returns. An owner is `:fiber` or a container. A parameter
carries no owner fact, because its owner is outside the function.

| Fact | Holds when |
|------|------------|
| `next(p, q)` | step `q` follows step `p` |
| `alloc(v, p)` | step `p` makes the value `v` |
| `read(v, c, p)` | step `p` reads the value `v` out of the container `c` |
| `store(v, c, p)` | step `p` stores the value `v` into the container `c` |
| `use(v, p)` | step `p` uses the value `v` |

| Derived | Holds when |
|---------|------------|
| `owned(v, o, p)` | at the start of step `p`, the owner `o` holds `v` |
| `live(v, p)` | a step from `p` on uses `v`, or a value that `v` holds |
| `released(v, p, q)` | `v` is released on the edge from step `p` to step `q` |
| `dies(v, c, p)` | at step `p`, the region of `v` dies with the region of `c` |
| `refused(v, c, p)` | the store of `v` into `c` at step `p` is refused |

## The rules

| Rule | Datalog |
|------|---------|
| Unheld value | `owned(v, :fiber, q) :- alloc(v, p), next(p, q).` |
| Read | `owned(v, c, q) :- read(v, c, p), next(p, q).` |
| Store | `owned(v, c, q) :- store(v, c, p), owned(v, :fiber, p), next(p, q).` |
| Store | `owned(v, o, q) :- owned(v, o, p), next(p, q), not moved(v, o, p).` where `moved(v, :fiber, p) :- store(v, c, p).` |
| Store inside a unit | `dies(v, c, p) :- owned(v, c, p).` |
| Store inside a unit | `dies(v, c, p) :- owned(v, d, p), dies(d, c, p).` |
| Store inside a unit | `refused(v, c, p) :- store(v, c, p), not owned(v, :fiber, p), not dies(v, c, p).` |
| Unheld value | `live(v, p) :- use(v, p).` |
| Unheld value | `live(v, p) :- next(p, q), live(v, q), not defined(v, p).` where `defined` is `alloc` or `read` |
| Container waits | `live(c, p) :- live(v, p), owned(v, c, p).` where `c` is a container |
| Unheld value | `released(v, p, q) :- next(p, q), was(v, p), owned(v, :fiber, q), not live(v, q).` where `was` is `live` or `alloc` |

So the region of `v` dies with the region of `c` when the chain of owners of
`v` reaches `c`. A container stays live while a value it holds is live, and
the recursion of `live` carries that up the same chain.

A release sits on the edge from the last step that needs a value to the first
step that does not. So a branch that never uses a value releases it on entry,
and a value the fiber stored no longer has a release of its own.

In the function below, the store at step 2 moves `b`, which the fiber holds,
into `a`. The store at step 4 is accepted, because `e` is held by `a` and so
already dies with it. The store at step 6 is refused, because `x` is held by
`coll`, whose owner is outside the function:

```lisp
(def proof ((import "std/proof")))

(defn ownership [next alloc read store]
  "owned(v, o, p) as [p [v o]], under the Unheld value, Read and Store rules."
  (def next-r (proof:relation next))
  (def it (proof:iteration))
  (def owned (proof:variable it "owned"))
  (def owned-at (proof:variable it "owned-at"))  # [[v o p] q]
  (proof:insert owned (proof:join (proof:relation (map (fn [[v p]] [p v]) alloc)) next-r
                                  (fn [p v q] [q [v :fiber]])))
  (proof:insert owned (proof:join (proof:relation (map (fn [[v c p]] [p [v c]]) read)) next-r
                                  (fn [p vc q] [q vc])))
  (def moved (proof:relation (map (fn [[v c p]] [v :fiber p]) store)))
  (def store-next (proof:join (proof:relation (map (fn [[v c p]] [p [v c]]) store)) next-r
                              (fn [p [v c] q] [[v :fiber p] [c q]])))
  (while (proof:changed? it)
    (proof:from-join owned-at owned next-r (fn [p [v o] q] [[v o p] q]))
    (proof:from-antijoin owned owned-at moved (fn [[v o p] q] [q [v o]]))
    (proof:from-join owned owned-at store-next (fn [k q [c q2]] [q2 [(get k 0) c]])))
  (proof:elements (proof:complete owned)))

# (defn f [coll acc]
#   (def a @[])          # 0  a is made
#   (def b (array 1))    # 1  b is made
#   (push a b)           # 2  b is stored into a
#   (def e (get a 0))    # 3  e is read out of a
#   (push a e)           # 4  e is stored into a
#   (def x (get coll 0)) # 5  x is read out of coll
#   (push acc x))        # 6  x is stored into acc

(def next  [[0 1] [1 2] [2 3] [3 4] [4 5] [5 6] [6 7]])
(def alloc [[:a 0] [:b 1]])
(def read  [[:e :a 3] [:x :coll 5]])
(def store [[:b :a 2] [:e :a 4] [:x :acc 6]])

(def owned (ownership next alloc read store))
(def owned-by (proof:relation (map (fn [[p [v o]]] [[o p] v]) owned)))
(def it (proof:iteration))
(def dies (proof:variable it "dies"))          # dies(v, c, p) as [[v p] c]
(proof:extend dies (map (fn [[p [v o]]] [[v p] o]) owned))
(while (proof:changed? it)
  (proof:from-join dies dies owned-by (fn [[d p] c v] [[v p] c])))

(def fiber-held
  (proof:relation (map (fn [[p [v o]]] [v p]) (filter (fn [[p [v o]]] (= o :fiber)) owned))))
(def unit
  (proof:relation (map (fn [[[v p] c]] [v c p]) (proof:elements (proof:complete dies)))))
(def not-fiber
  (proof:antijoin (proof:relation (map (fn [[v c p]] [[v p] c]) store)) fiber-held
                  (fn [[v p] c] [[v c p] true])))
(def refused (proof:antijoin not-fiber unit (fn [k _] k)))
(assert (= [[:x :acc 6]] (proof:elements refused)) "only the store of x is refused")
```

In the function below, `a` is released after its use at step 3 and on entry to
the `else` branch at step 4, and `b` the other way round:

```lisp
(defn releases [next alloc read use owned]
  "released(v, p, q) as [v p q], under the Unheld value and Container waits rules."
  (def next-r (proof:relation next))
  (def by-target (proof:relation (map (fn [[p q]] [q p]) next)))
  (def defined (proof:merge (proof:relation alloc)
                            (proof:relation (map (fn [[v c p]] [v p]) read))))
  (def held-by (proof:relation (map (fn [[p [v o]]] [[v p] o])
                                    (filter (fn [[p [v o]]] (not= o :fiber)) owned))))
  (def it (proof:iteration))
  (def live (proof:variable it "live"))                 # live(v, p) as [p v]
  (def live-at (proof:variable it "live-at"))           # [[v p] true]
  (def live-before (proof:variable it "live-before"))   # [[v p] true]
  (proof:extend live (map (fn [[v p]] [p v]) use))
  (while (proof:changed? it)
    # live(v, p) :- next(p, q), live(v, q), not defined(v, p).
    (proof:from-join live-before live by-target (fn [q v p] [[v p] true]))
    (proof:from-antijoin live live-before defined (fn [[v p] _] [p v]))
    # live(c, p) :- live(v, p), owned(v, c, p).
    (proof:from-map live-at live (fn [[p v]] [[v p] true]))
    (proof:from-join live live-at held-by (fn [[v p] _ c] [p c])))
  (def live-r (proof:relation (map (fn [[p v]] [v p]) (proof:elements (proof:complete live)))))
  (def was (proof:merge live-r (proof:relation alloc)))
  # released(v, p, q) :- next(p, q), was(v, p), owned(v, :fiber, q), not live(v, q).
  (def candidates
    (proof:join (proof:relation (map (fn [[v p]] [p v]) (proof:elements was))) next-r
                (fn [p v q] [[v q] p])))
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
(def next  [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5]])
(def alloc [[:a 0] [:b 1]])
(def use   [[:a 3] [:b 4]])               # use(v, p)
(assert (= [[:a 2 4] [:a 3 5] [:b 2 3] [:b 4 5]]
           (releases next alloc [] use (ownership next alloc [] [])))
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
(def next  (map (fn [p] [p (+ p 1)]) (range 8)))
(def alloc [[:a 0] [:b 1] [:c 2]])
(def read  [[:e :a 5] [:f :e 6]])
(def store [[:c :b 3] [:b :a 4]])
(def use   [[:b 3] [:c 3] [:a 4] [:b 4] [:a 5] [:e 6] [:f 7]])
(assert (= [[:a 7 8]] (releases next alloc read use (ownership next alloc read store)))
        "a container is released after the last use of a value it holds")
```

## Not written yet

- Every rule that moves a release: Removal, Borrow before release, Allocation
  operand and the capture rules.
- The half of the Join rule that keeps each value in one state.
- The Datalog form of every other rule in [the ownership rules](ownership.md)
  and [signatures](signature.md).
