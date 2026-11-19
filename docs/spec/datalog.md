# The rules in Datalog

<!-- audited: 2026-11-18 -->

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

| Derived | Holds when |
|---------|------------|
| `owned(v, o, p)` | at the start of step `p`, the owner `o` holds `v` |
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

So the region of `v` dies with the region of `c` when the chain of owners of
`v` reaches `c`. The Unheld value rule also says when a value is released, and
that part is not written yet.

In the function below, the store at step 2 moves `b`, which the fiber holds,
into `a`. The store at step 4 is accepted, because `e` is held by `a` and so
already dies with it. The store at step 6 is refused, because `x` is held by
`coll`, whose owner is outside the function:

```lisp
(def proof ((import "std/proof")))

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

(def next-r (proof:relation next))
(def it (proof:iteration))
(def owned (proof:variable it "owned"))        # owned(v, o, p) as [p [v o]]
(def owned-at (proof:variable it "owned-at"))  # [[v o p] q]
(def owned-by (proof:variable it "owned-by"))  # [[o p] v]
(def dies (proof:variable it "dies"))          # dies(v, c, p) as [[v p] c]

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
  (proof:from-join owned owned-at store-next (fn [k q [c q2]] [q2 [(get k 0) c]]))
  (proof:from-map owned-by owned (fn [[p [v o]]] [[o p] v]))
  (proof:from-map dies owned (fn [[p [v o]]] [[v p] o]))
  (proof:from-join dies owned-by dies (fn [[d p] v c] [[v p] c])))

(def fiber-held
  (proof:relation (map (fn [[p [v o]]] [v p])
                       (filter (fn [[p [v o]]] (= o :fiber))
                               (proof:elements (proof:complete owned))))))
(def unit
  (proof:relation (map (fn [[[v p] c]] [v c p]) (proof:elements (proof:complete dies)))))
(def not-fiber
  (proof:antijoin (proof:relation (map (fn [[v c p]] [[v p] c]) store)) fiber-held
                  (fn [[v p] c] [[v c p] true])))
(def refused (proof:antijoin not-fiber unit (fn [k _] k)))
(assert (= [[:x :acc 6]] (proof:elements refused)) "only the store of x is refused")
```

## Not written yet

- The release at last use, and every rule that moves a release: Removal,
  Container waits, Borrow before release, Allocation operand and the capture
  rules.
- The Datalog form of every other rule in [the ownership rules](ownership.md)
  and [signatures](signature.md).
