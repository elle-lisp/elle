# Crossing a fiber, in Datalog

<!-- audited: 2026-11-21 -->

How an emit and a resume move a value to another fiber, and which crossings are refused, as Datalog that runs on proof.

[The ownership rules](ownership.md) state the rule for a crossing in words, and
[the judgment](judgment.md) names it a target. [Sink](sink.md) lets a call sink
an argument, and its fences are included here.

| Fact | Holds when |
|------|------------|
| `cross(y, p)` | step `p` passes the value `y` to another fiber: an emit its payload, or a resume its value |

| Derived | Holds when |
|---------|------------|
| `refused(y, p)` | the crossing of `y` at step `p` is refused |

## The rules

| Rule | Datalog |
|------|---------|
| Crossing a fiber | `pass(y, [:cross p], p) :- cross(y, p).` |
| Crossing a fiber | `refused(y, p) :- cross(y, p), not owned(y, :fiber, p).` |
| Crossing a fiber | `refused(y, p) :- cross(y, p), next(p, q), live(y, q).` |
| Crossing a fiber | `refused(y, p) :- cross(y, p), live(x, [:on p]), x ≠ y.` in a probe pass with `next(p, [:on p]) :- cross(y, p).` and `use(y, [:on p]) :- cross(y, p).` |
| Crossing a fiber | `refused(y, p) :- cross(y, p), capture(x, y, a), not store(x, y, a).` |

A step that passes a value to another fiber ends its fiber's hold, and is
refused where its fiber does not own the value, where the value is live after
the step, where the value borrows from a value that stays, or where a capture
of the value is not a store.

The other fiber's hold is not nested inside this one's, so a crossing is a
move. The pass ends the hold, as a sink does, and nothing reads `[:cross p]`,
so this fiber holds no release for the value. A value borrows from `x` where
`x` would stay live if the value alone were used after the step. The probe step
`[:on p]` adds that use, so every wait rule decides it, as in
[Owner not passed](owner.md). A closure that borrows a mutable capture adds no
wait, so the last clause reads the captures. A late capture is a store, and
moves with the closure.

An emit and a resume each make their result, as an eval does: another fiber
moved it here, so `alloc(r, p)` holds for it. The facts are those after the late
captures of [captures](capture.md), as for a sink.

In `relay` below, the resume at step 1 moves `w` to the child, and the emit at
step 2 moves `r`, the child's payload, to the parent. Neither has a release
here, and `s`, which the parent resumes with, is released at once:

```lisp
(include-file "sink.md")

(defn crossing [facts & waits]
  "FACTS with the late captures as stores, read from a first pass with WAITS, and a pass to
   [:cross p] for each value that step p passes to another fiber."
  (def all (with-captures facts ;waits))
  # pass(y, [:cross p], p) :- cross(y, p).
  (put all :pass (concat (get all :pass [])
                         (map (fn [[y p]] [y [:cross p] p]) (get facts :cross [])))))

(defn cross-refusals [facts & waits]
  "refused(y, p) as [y p], under the Crossing a fiber rule, read from the facts crossing
   returns and a probe pass with WAITS."
  (def all (crossing facts ;waits))
  (def cross (get facts :cross []))
  # refused(y, p) :- cross(y, p), not owned(y, :fiber, p).
  (def unowned (proof:antijoin (proof:relation (map (fn [yp] [yp true]) cross))
                               (fiber-held (ownership all)) (fn [yp _] yp)))
  # refused(y, p) :- cross(y, p), next(p, q), live(y, q).
  (def after (proof:join (proof:join (proof:relation (map (fn [[y p]] [p y]) cross))
                                     (proof:relation (all :next)) (fn [p y q] [[y q] [y p]]))
                         (proof:map (live-all all ;waits) (fn [yq] [yq true])) (fn [_ yp _] yp)))
  # next(p, [:on p]) :- cross(y, p).  use(y, [:on p]) :- cross(y, p).
  (def probe (merge all {:next (concat (all :next) (map (fn [[y p]] [p [:on p]]) cross))
                         :use  (concat (get all :use []) (map (fn [[y p]] [y [:on p]]) cross))}))
  # refused(y, p) :- cross(y, p), live(x, [:on p]), x ≠ y.
  (def on (proof:join (proof:map (live-all probe ;waits) (fn [[x q]] [q x]))
                      (proof:relation (map (fn [[y p]] [[:on p] [y p]]) cross))
                      (fn [_ x yp] [x yp])))
  (def borrowing (map (fn [[x yp]] yp) (filter (fn [[x [y p]]] (not= x y)) (proof:elements on))))
  # refused(y, p) :- cross(y, p), capture(x, y, a), not store(x, y, a).
  (def borrowed (proof:antijoin (proof:relation (map (fn [xka] [xka true]) (get facts :capture [])))
                                (proof:relation (get all :store [])) (fn [[x k a] _] [k true])))
  (def capturing (proof:join borrowed (proof:relation cross) (fn [y _ p] [y p])))
  (proof:elements (proof:relation (concat (proof:elements unowned) (proof:elements after)
                                          borrowing (proof:elements capturing)))))

# (defn relay [f]
#   (def w @[1])                # 0  w is made
#   (def r (fiber/resume f w))  # 1  w crosses, and r is made
#   (yield r)                   # 2  r crosses, and s is made
#   nil)                        # 3  the exit
(def relay {:next  (steps 3)
            :param [[:f 0]]
            :alloc [[:w 0] [:r 1] [:s 2]]
            :cross [[:w 1] [:r 2]]
            :use   [[:f 1] [:w 1] [:r 2]]})
(assert (empty? (cross-refusals relay)) "each value relay passes on is its fiber's to move")
(assert (= [[:s 2 3]] (released-all (crossing relay))) "and has no release in relay")
```

Only a value's owner can move it, and a parameter carries no owner fact. So the
lent instance of `produce` below refuses its emit. A call at the last use of
its argument sinks it, under the Sink rule, and names the sunk instance. That
instance owns `x` from its entry, so its emit is accepted:

```lisp
# (defn produce [x]
#   (yield x)                   # 0  x crosses, and r is made
#   nil)                        # 1  the exit
(def produce {:next [[0 1]] :param [[:x 0]] :alloc [[:r 0]] :cross [[:x 0]] :use [[:x 0]]})
(assert (= [[:x 0]] (cross-refusals produce)) "a lent parameter is not the fiber's to move")
(assert (= [[:produce 0]] (lends-of :produce produce {})) "produce lends its parameter")
(assert (empty? (cross-refusals (instance produce [0]))) "and its sunk instance moves it")
```

In `ua` below, `w` is used after the resume moves it, while the child may
already have released it. In `sh`, the pair `v` borrows `x` out of `m`, which
stays here. In `kc`, the closure `k` borrows `a`, which is used after the emit,
so the capture is not late. In `km`, the last use of `a` comes before the emit,
so the capture is late, and `a` moves with `k`:

```lisp
# (defn ua [f]
#   (def w @[1])                # 0  w is made
#   (fiber/resume f w)          # 1  w crosses, and r is made
#   (length w))                 # 2  w is used   3  the exit
(def ua {:next  (steps 3)
         :param [[:f 0]]
         :alloc [[:w 0] [:r 1]]
         :cross [[:w 1]]
         :use   [[:f 1] [:w 1] [:w 2]]})
(assert (= [[:w 1]] (cross-refusals ua)) "a value used after it crosses is refused")

# (defn sh []
#   (def m @[@[1]])             # 0  m is made
#   (def x (get m 0))           # 1  x is read out of m
#   (def v (pair x ()))         # 2  v is made, with x as an operand
#   (yield v)                   # 3  v crosses, and r is made
#   nil)                        # 4  the exit
(def sh {:next      (steps 4)
         :alloc     [[:m 0] [:v 2] [:r 3]]
         :read      [[:x :m 1]]
         :operand   [[:x :v 2]]
         :immutable [:v]
         :cross     [[:v 3]]
         :use       [[:m 1] [:x 2] [:v 3]]})
(assert (= [[:v 3]] (cross-refusals sh)) "a value that borrows from one that stays is refused")

# (defn kc []
#   (def a @[1])                # 0  a is made
#   (def k (fn [] (length a)))  # 1  k is made, and captures a
#   (yield k)                   # 2  k crosses, and r is made
#   (length a))                 # 3  a is used   4  the exit
(def kc {:next    (steps 4)
         :alloc   [[:a 0] [:k 1] [:r 2]]
         :capture [[:a :k 1]]
         :cross   [[:k 2]]
         :use     [[:k 2] [:a 3]]})
(assert (= [[:k 2]] (cross-refusals kc)) "a closure that borrows a capture is refused")

# (defn km []
#   (def a @[1])                # 0  a is made
#   (def k (fn [] (length a)))  # 1  k is made, and captures a
#   (length a)                  # 2  a is used
#   (yield k)                   # 3  k crosses, and r is made
#   nil)                        # 4  the exit
(def km (merge kc {:alloc [[:a 0] [:k 1] [:r 3]] :cross [[:k 3]] :use [[:a 2] [:k 3]]
                   :next (steps 4)}))
(assert (empty? (cross-refusals km)) "a closure that holds its capture moves it")
(assert (= [[:r 3 4]] (released-all (crossing km))) "so neither a nor k has a release here")
```

## Not written yet

- The body of `fiber/new`, which the child runs while the parent holds the
  fiber, and the body's result, which crosses to the parent.
- A native that passes an argument to another fiber, as `fiber/abort` and
  `fiber/cancel` do.
