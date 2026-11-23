# Crossing a fiber, in Datalog

<!-- audited: 2026-11-23 -->

How an emit and a resume move a value to another fiber, and which crossings are refused, as Datalog that runs on proof.

[The ownership rules](ownership.md) state the rule for a crossing in words, and
[the judgment](judgment.md) names it a target. [Sink](sink.md) lets a call sink
an argument.

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
| Crossing a fiber | `refused(y, p) :- cross(y, p), not owned(y, :fiber, p), not immediate(y).` |
| Crossing a fiber | `refused(y, p) :- cross(y, p), next(p, q), live(y, q), not immediate(y).` |
| Crossing a fiber | `refused(y, p) :- cross(y, p), live(x, [:on p]), x ≠ y.` in a probe pass with `next(p, [:on p]) :- cross(y, p).` and `use(y, [:on p]) :- cross(y, p).` |
| Crossing a fiber | `refused(y, p) :- cross(y, p), capture(x, y, a), not store(x, y, a).` |

A step that passes a value to another fiber ends its fiber's hold. It is
refused in four cases: its fiber does not own the value, the value is live
after the step, the value borrows from a value that stays, or a capture of the
value is not a store. An immediate needs no owner and crosses as a copy, so
neither of the first two cases refuses it.

The other fiber's hold is not nested inside this one's, so a crossing is a
move. The pass ends the hold, as a sink does, and nothing reads `[:cross p]`,
so this fiber holds no release for the value. A value borrows from `x` where
`x` would stay live if the value alone were used after the step. The probe step
`[:on p]` adds that use, so every wait rule decides it, as in
[Owner not passed](owner.md). A closure that borrows a mutable capture adds no
wait, so the last clause reads the captures. A late capture is a store, and
moves with the closure.

An emit makes its result, as an eval does: the resume that answers it moved
the value here, so `alloc(r, p)` holds for it. [Payload](payload.md) says what
a resume's result is. The facts are those after the late captures of
[captures](capture.md), as for a sink.

In `drive`, the resume at step 1 moves `w` to the child, and the emit at step 3
moves `a` to the parent. Neither has a release here, and `s`, which the parent
resumes with, is released at once.

Only a value's owner can move it, and a parameter carries no owner fact. So the
lent instance of `produce` refuses its emit. A call at the last use of its
argument sinks it, under the Sink rule, and names the sunk instance. That
instance owns `x` from its entry, so its emit is accepted.

In `ua`, `w` is used after the resume moves it, while the child may already
have released it. In `sh`, the pair `v` borrows `x` out of `m`, which stays
here. In `kc`, the closure `k` borrows `a`, which is used after the emit, so
the capture is not late. In `km`, the last use of `a` comes before the emit, so
the capture is late, and `a` moves with `k`. In `yn`, the immediate `n` crosses
and is returned after:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence} ((import "../spec.md")))
  (def {:fiber-held fiber-held :ownership ownership :steps steps} ((import "./datalog.md")))
  (def {:with-captures with-captures :released-all released-all :live-all live-all}
    ((import "./capture.md")))
  (def {:lends-of lends-of :instance instance} ((import "./sink.md")))

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

  (def drive (fence 'drive
    :doc "Each value drive passes on is its fiber's to move, and has no release in drive."
    :source "(defn drive [f]
  (def w @[1])                # 0  w is made
  (fiber/resume f w)          # 1  w crosses
  (def a @[2])                # 2  a is made
  (yield a)                   # 3  a crosses, and s is made
  nil)                        # 4  the exit"
    :facts {:next  (steps 4)
            :param [[:f 0]]
            :alloc [[:w 0] [:a 2] [:s 3]]
            :cross [[:w 1] [:a 3]]
            :use   [[:f 1] [:w 1] [:a 3]]}
    :check (fn [drive] (and (empty? (cross-refusals drive))
                            (= [[:s 3 4]] (released-all (crossing drive)))))))

  (def produce (fence 'produce
    :doc "A lent parameter is not the fiber's to move, and the sunk instance of produce moves it."
    :source "(defn produce [x]
  (yield x)                   # 0  x crosses, and r is made
  nil)                        # 1  the exit"
    :facts {:next [[0 1]] :param [[:x 0]] :alloc [[:r 0]] :cross [[:x 0]] :use [[:x 0]]}
    :check (fn [produce] (and (= [[:x 0]] (cross-refusals produce))
                              (= [[:produce 0]] (lends-of :produce produce {}))
                              (empty? (cross-refusals (instance produce [0])))))))

  (def ua (fence 'ua :doc "A value used after it crosses is refused."
    :source "(defn ua [f]
  (def w @[1])                # 0  w is made
  (fiber/resume f w)          # 1  w crosses
  (length w))                 # 2  w is used   3  the exit"
    :facts {:next  (steps 3)
            :param [[:f 0]]
            :alloc [[:w 0]]
            :cross [[:w 1]]
            :use   [[:f 1] [:w 1] [:w 2]]}
    :check (fn [ua] (= [[:w 1]] (cross-refusals ua)))))

  (def sh (fence 'sh :doc "A value that borrows from one that stays is refused."
    :source "(defn sh []
  (def m @[@[1]])             # 0  m is made
  (def x (get m 0))           # 1  x is read out of m
  (def v (pair x ()))         # 2  v is made, with x as an operand
  (yield v)                   # 3  v crosses, and r is made
  nil)                        # 4  the exit"
    :facts {:next      (steps 4)
            :alloc     [[:m 0] [:v 2] [:r 3]]
            :read      [[:x :m 1]]
            :operand   [[:x :v 2]]
            :immutable [:v]
            :cross     [[:v 3]]
            :use       [[:m 1] [:x 2] [:v 3]]}
    :check (fn [sh] (= [[:v 3]] (cross-refusals sh)))))

  (def kc (fence 'kc :doc "A closure that borrows a capture is refused."
    :source "(defn kc []
  (def a @[1])                # 0  a is made
  (def k (fn [] (length a)))  # 1  k is made, and captures a
  (yield k)                   # 2  k crosses, and r is made
  (length a))                 # 3  a is used   4  the exit"
    :facts {:next    (steps 4)
            :alloc   [[:a 0] [:k 1] [:r 2]]
            :capture [[:a :k 1]]
            :cross   [[:k 2]]
            :use     [[:k 2] [:a 3]]}
    :check (fn [kc] (= [[:k 2]] (cross-refusals kc)))))

  # (defn km []
  #   (def a @[1])                # 0  a is made
  #   (def k (fn [] (length a)))  # 1  k is made, and captures a
  #   (length a)                  # 2  a is used
  #   (yield k)                   # 3  k crosses, and r is made
  #   nil)                        # 4  the exit
  (def km (fence 'km
    :doc "A closure that holds its capture moves it, so neither a nor k has a release here."
    :facts (merge kc:facts {:alloc [[:a 0] [:k 1] [:r 3]] :cross [[:k 3]] :use [[:a 2] [:k 3]]
                            :next (steps 4)})
    :check (fn [km] (and (empty? (cross-refusals km))
                         (= [[:r 3 4]] (released-all (crossing km)))))))

  {:crossing crossing :cross-refusals cross-refusals :fences [drive produce ua sh kc km]})
```

## Not written yet

- The body of `fiber/new`, which the child runs while the parent holds the
  fiber, and the body's result, which crosses to the parent.
- A native that passes an argument to another fiber, as `fiber/abort` and
  `fiber/cancel` do.
