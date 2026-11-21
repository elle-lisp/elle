# The rules in Datalog

<!-- audited: 2026-11-20 -->

The facts the ownership rules read from a function in kernel form, and the rules of who owns each value, as Datalog that runs on proof.

[The ownership rules](ownership.md) state each rule in words, and
[the judgment](judgment.md) says how the rules decide acceptance. The rules
run on [proof](../stdlib/proof.md), so the fence that shows a rule also tests
it. [Liveness](liveness.md) keeps each value live until its last use,
[release](release.md) places each release, [removal](removal.md) adds the waits
a removal needs, and [capture](capture.md) decides what a closure takes.
[Made anew](anew.md) shows a loop that makes a value under an old name on each
pass.
[Closure](closure.md) keeps a closure that a jump hands on live for the checks
of its captures. [Recur](recur.md) covers a loop, a block and an eval, and
[call](call.md) a call. [Stored](stored.md) finds the parameters a function
stores, and [place](place.md) gives the callee their ownership.
[Return](return.md) gives a result its kind, [itself](itself.md) finds a result
that is a parameter itself, [recursion](recursion.md) derives the kinds of
functions that call each other, [borrows](borrows.md) names what a result
borrows from, and [group](group.md) what such a function's result borrows. [Sink](sink.md) lets a callee release an argument,
[aliased](aliased.md) an argument that another one borrows from, and
[Owner not passed](owner.md) the owner of what a call lends.

## Points, values and owners

A point is one step of a function in kernel form, and
[HIR](../impl/hir.md) lists the kernel. A value is named by the binding that
receives it. A read names a new value, because the rules cannot know which
element a read returns. An owner is `:fiber` or a container. A parameter
carries no owner fact, because its owner is outside the function.

A step that takes a value out of a container, where no binding receives the
value, names it `[:out p]`. So `(del t :k)` at step 4 gives
`remove([:out 4], t, 4)`, and an overwrite `(put t :k x)` at step 2 gives
`store(x, t, 2)` and `remove([:out 2], t, 2)`. Where such a step takes out no
value, the release of `[:out p]` frees nothing.

| Fact | Holds when |
|------|------------|
| `next(p, q)` | step `q` follows step `p` |
| `alloc(v, p)` | step `p` makes the value `v` |
| `read(v, c, p)` | step `p` reads the value `v` out of the container `c` |
| `store(v, c, p)` | step `p` stores the value `v` into the container `c` |
| `remove(v, c, p)` | step `p` takes the value `v` out of the container `c` |
| `capture(x, k, p)` | step `p` makes the closure `k`, which captures the value `x` |
| `pass(y, x, p)` | step `p` hands the value `y` to `x`: a jump to the parameter `x` of the block it enters, a call to its result `x`, or a call that sinks `y` into its callee |
| `operand(x, v, p)` | step `p` makes the value `v` with the value `x` as an operand |
| `immutable(v)` | the value `v` is immutable |
| `use(v, p)` | step `p` uses the value `v` |

| Derived | Holds when |
|---------|------------|
| `owned(v, o, p)` | at the start of step `p`, the owner `o` holds `v` |
| `dies(v, c, p)` | at step `p`, the region of `v` dies with the region of `c` |
| `refused(v, c, p)` | the store of `v` into `c` at step `p` is refused |
| `share(x, v, p)` | the immutable value `v` that step `p` makes borrows its operand `x` |

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
| Removal | `owned(v, :fiber, q) :- remove(v, c, p), next(p, q).` |
| Made anew | `moved(v, o, p) :- made(v, p), owned(v, o, p).` where `made` is `alloc` or `remove` |
| Allocation operand | `share(x, v, a) :- operand(x, v, a), immutable(v).` |
| Allocation operand | `store(x, v, a) :- operand(x, v, a), not immutable(v).` |
| Allocation operand | `owned(x, v, q) :- share(x, v, a), owned(x, :fiber, a), next(a, q).` where `moved(x, :fiber, a) :- share(x, v, a).` |
| Recur | `moved(y, :fiber, p) :- pass(y, x, p).` |
| Recur | `moved(x, o, p) :- pass(y, x, p), owned(x, o, p).` |
| Recur | `owned(x, o, q) :- pass(y, x, p), owned(y, o, p), o ≠ x, next(p, q).` |
| Recur | `owned(x, o, q) :- pass(y, x, p), owned(y, x, p), owned(x, o, p), next(p, q).` |

So the region of `v` dies with the region of `c` when the chain of owners of
`v` reaches `c`.

A loop parameter names one value on each pass. The old value does not cross the
back edge, and a new value read out of the old one keeps the old one's owner.
The second Recur clause and the Made anew clause negate `owned` through
themselves, so the fence reads them as `passed(x, p)`, which drops each owner
`x` has at `p`: a pass to `x`, or a step that makes `x` or takes it out.

In the function below, the store at step 2 moves `b`, which the fiber holds,
into `a`. The store at step 4 is accepted, because `e` is held by `a` and so
already dies with it. The store at step 6 is refused, because `x` is held by
`coll`, whose owner is outside the function:

```lisp
(def proof ((import "std/proof")))

(defn steps [n]
  "next(p, p + 1) for each step p below N, as [p q]: a body with no branch."
  (map (fn [p] [p (+ p 1)]) (range n)))

(defn operands [facts]
  "[share store]: the operands of each immutable value, as [x v a], and the stores the
   Store rule reads, with each operand of a value that is not immutable."
  (def by-value (proof:relation (map (fn [[x v a]] [v [x a]]) (get facts :operand []))))
  (def immutable (proof:relation (get facts :immutable [])))
  # share(x, v, a) :- operand(x, v, a), immutable(v).
  (def share (proof:join by-value (proof:map immutable (fn [v] [v true]))
                         (fn [v [x a] _] [x v a])))
  # store(x, v, a) :- operand(x, v, a), not immutable(v).
  (def stored (proof:antijoin by-value immutable (fn [v [x a]] [x v a])))
  [(proof:elements share) (concat (get facts :store []) (proof:elements stored))])

(defn ownership [facts]
  "owned(v, o, p) as [p [v o]], under the Unheld value, Read, Store, Removal,
   Allocation operand and Recur rules."
  (def [next alloc read remove pass]
    (map (fn [k] (get facts k [])) [:next :alloc :read :remove :pass]))
  (def [share stores] (operands facts))
  (def store (concat stores share))   # a share moves an operand the fiber holds
  (def next-r (proof:relation next))
  (def it (proof:iteration))
  (def owned (proof:variable it "owned"))
  (def owned-at (proof:variable it "owned-at"))  # [[v o p] q]
  (def owned-kept (proof:variable it "owned-kept"))   # [[v p] [o q]]
  (def owned-vp (proof:variable it "owned-vp"))       # [[v p] o]
  (def handed (proof:variable it "handed"))           # [[x p] [o q]]
  (def inside (proof:variable it "inside"))           # [[x p] q]
  (def made (concat alloc (map (fn [[v c p]] [v p]) remove)))
  (proof:insert owned (proof:join (proof:relation (map (fn [[v p]] [p v]) made)) next-r
                                  (fn [p v q] [q [v :fiber]])))
  (proof:insert owned (proof:join (proof:relation (map (fn [[v c p]] [p [v c]]) read)) next-r
                                  (fn [p vc q] [q vc])))
  (def moved (proof:relation (concat (map (fn [[v c p]] [v :fiber p]) store)
                                     (map (fn [[y x p]] [y :fiber p]) pass))))
  (def passed (proof:relation (concat (map (fn [[y x p]] [x p]) pass) made)))
  (def pass-next (proof:join (proof:relation (map (fn [[y x p]] [p [y x]]) pass)) next-r
                             (fn [p [y x] q] [[y p] [x q]])))
  (def store-next (proof:join (proof:relation (map (fn [[v c p]] [p [v c]]) store)) next-r
                              (fn [p [v c] q] [[v :fiber p] [c q]])))
  (while (proof:changed? it)
    (proof:from-join owned-at owned next-r (fn [p [v o] q] [[v o p] q]))
    (proof:from-antijoin owned-kept owned-at moved (fn [[v o p] q] [[v p] [o q]]))
    (proof:from-antijoin owned owned-kept passed (fn [[v p] [o q]] [q [v o]]))
    (proof:from-join owned owned-at store-next (fn [k q [c q2]] [q2 [(get k 0) c]]))
    (proof:from-map owned-vp owned (fn [[p [v o]]] [[v p] o]))
    (proof:from-join handed owned-vp pass-next (fn [[y p] o [x q]] [[x p] [o q]]))
    # owned(x, o, q) :- pass(y, x, p), owned(y, o, p), o ≠ x, next(p, q).
    (proof:from-leapjoin owned handed (proof:prefix-filter (fn [[[x p] [o q]]] (not= o x)))
                         (fn [[[x p] [o q]] _] [q [x o]]))
    # owned(x, o, q) :- pass(y, x, p), owned(y, x, p), owned(x, o, p), next(p, q).
    (proof:from-leapjoin inside handed (proof:prefix-filter (fn [[[x p] [o q]]] (= o x)))
                         (fn [[[x p] [o q]] _] [[x p] q]))
    (proof:from-join owned inside owned-vp (fn [[x p] q o] [q [x o]])))
  (proof:elements (proof:complete owned)))

# (defn f [coll acc]
#   (def a @[])          # 0  a is made
#   (def b (array 1))    # 1  b is made
#   (push a b)           # 2  b is stored into a
#   (def e (get a 0))    # 3  e is read out of a
#   (push a e)           # 4  e is stored into a
#   (def x (get coll 0)) # 5  x is read out of coll
#   (push acc x))        # 6  x is stored into acc
(def f {:next  (steps 7)
        :alloc [[:a 0] [:b 1]]
        :read  [[:e :a 3] [:x :coll 5]]
        :store [[:b :a 2] [:e :a 4] [:x :acc 6]]})

(defn units [owned]
  "dies(v, c, p) as [[v p] c], under the Store inside a unit rule."
  (def owned-by (proof:relation (map (fn [[p [v o]]] [[o p] v]) owned)))
  (def it (proof:iteration))
  (def dies (proof:variable it "dies"))
  (proof:extend dies (map (fn [[p [v o]]] [[v p] o]) owned))
  (while (proof:changed? it)
    (proof:from-join dies dies owned-by (fn [[d p] c v] [[v p] c])))
  (proof:elements (proof:complete dies)))

(defn fiber-held [owned]
  "owned(v, :fiber, p) as a relation of [v p], from the OWNED that ownership returns."
  (proof:relation (map (fn [[p [v o]]] [v p]) (filter (fn [[p [v o]]] (= o :fiber)) owned))))

(defn refusals [facts owned]
  "refused(v, c, p) as [v c p], under the Store inside a unit rule."
  (def unit (proof:relation (map (fn [[[v p] c]] [v c p]) (units owned))))
  (def not-fiber
    (proof:antijoin (proof:relation (map (fn [[v c p]] [[v p] c]) (get (operands facts) 1)))
                    (fiber-held owned) (fn [[v p] c] [[v c p] true])))
  (proof:elements (proof:antijoin not-fiber unit (fn [k _] k))))

(assert (= [[:x :acc 6]] (refusals f (ownership f))) "only the store of x is refused")
```

## Not written yet

- The Datalog form of every other rule in [the ownership rules](ownership.md)
  and [signatures](signature.md).
