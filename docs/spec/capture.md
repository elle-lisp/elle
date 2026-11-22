# Captures in Datalog

<!-- audited: 2026-11-22 -->

How a closure borrows or takes each value it captures, decided in one pass, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts, [release](release.md) places each
release, and [removal](removal.md) adds the waits a removal needs.
[Hand-over at last use](handover.md) decides where a jump hands its value over.
[The ownership rules](ownership.md) state each rule in words.

| Derived | Holds when |
|---------|------------|
| `gone(x, q)` | the region of `x` has ended by step `q` |
| `late(x, k, a)` | in the first pass, the closure `k` is live where the region of `x` has ended |

## The rules

| Rule | Datalog |
|------|---------|
| Immutable capture | `live(x, p) :- capture(x, k, a), immutable(x), live-as(k, p).` |
| Mutable capture | `gone(x, q) :- released(x, p, q).` |
| Mutable capture | `gone(x, q) :- dies(x, w, p), released(w, p, q).` |
| Mutable capture | `gone(x, q) :- remove(r, c, p0), owned(x, c, p0), released(r, p, q).` |
| Mutable capture | `gone(x, q2) :- gone(x, q), next(q, q2), not defined(x, q).` where `defined` is `alloc`, `read`, `remove` or `pass` |
| Mutable capture | `late(x, k, a) :- capture(x, k, a), not immutable(x), gone(x, q), live-as(k, q).` |
| Mutable capture | `store(x, k, a) :- late(x, k, a).` |

The first pass runs every rule but these. `gone` reads its `released`, and
`late` its `live-as(k, q)` from [liveness](liveness.md), under
[the Closure handed over rule](closure.md). The program then runs again with
the stores that `late` adds and with the Immutable capture wait. Before that
run, Hand-over at last use reads each jump whose value is live after it, in a
pass of its own with those stores and waits. A mutable capture that is not
late is a borrow, and its owner does not wait for the closure.
[Closure](closure.md) refuses a closure that outlives a value it borrows.

A store changes `owned`, and `live` reads `owned`, so a choice read from the
final pass would negate through recursion. The choice is made once. Iteration
to a fixpoint waits for a program that needs it.

A fiber made in a parameterize body captures each value the body binds, so
the Parameter inheritance rule gives a `capture` fact for each, and adds no
clause.

In `c1`, `k` is used before the last use of `a`, so `k` borrows `a`. In `c2`
the order is the other way round, so the capture is late, and `a` moves into
`k` and is released with it.

In `c3`, `x` is read out of `m`, and `k` is used after `m` is released. So the
capture is late, and the Store inside a unit rule refuses its store. In `c4`,
`a` is immutable, so `k` borrows it and `a` waits for `k`.

A value made again on the next pass of a loop is a new value, so `gone` stops
at the step that makes it. In `lc`, `c1` is a loop body, and the release of `a`
at the end of one pass does not reach `k` on the next, so `k` borrows `a`. In
`l5`, `k` borrows `e`, which is read out of `m` on each pass. In `lc2`, `k` is
used after the last use of `a` in the same pass, so the capture is late:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence} ((import "../spec.md")))
  (def {:ownership ownership :units units :refusals refusals :steps steps}
    ((import "./datalog.md")))
  (def {:liveness liveness :operand operand :defined-in defined-in :live-as-of live-as-of}
    ((import "./liveness.md")))
  (def {:releases releases :released-from released-from} ((import "./release.md")))
  (def {:removal removal} ((import "./removal.md")))
  (def {:handed-over handed-over} ((import "./handover.md")))

  (defn captured [facts owned it live live-by live-at]
    "The clause for live under the Immutable capture rule, for liveness."
    (def immutable (proof:relation (map (fn [x] [x true]) (get facts :immutable []))))
    (def by-closure (proof:join (proof:relation (map (fn [[x k a]] [x k]) (get facts :capture [])))
                                immutable (fn [x k _] [k x])))
    (fn []
      # live(x, p) :- capture(x, k, a), immutable(x), live-as(k, p).
      (proof:from-join live live-by by-closure (fn [k p x] [p x]))))

  (defn live-all [facts & waits]
    "live(v, p) as a relation of [v p], from FACTS under every wait rule and WAITS."
    (liveness facts (ownership facts) removal operand captured ;waits))

  (defn released-all [facts & waits]
    "released(v, p, q) as [v p q], from FACTS under every wait rule and WAITS."
    (releases facts (ownership facts) removal operand captured ;waits))

  (defn gone-of [facts owned released]
    "gone(x, q) as [x q], from the relation RELEASED that releases returns."
    (def rel-by (proof:relation (map (fn [[w p q]] [[w p] q]) released)))
    (def may (proof:join (proof:relation (map (fn [[r c p0]] [[c p0] r]) (get facts :remove [])))
                         (proof:relation (map (fn [[p [v o]]] [[o p] v]) owned))
                         (fn [_ r x] [r x])))
    (def it (proof:iteration))
    (def gone (proof:variable it "gone"))   # [q x]
    # gone(x, q) :- released(x, p, q).
    (proof:extend gone (map (fn [[x p q]] [q x]) released))
    # gone(x, q) :- dies(x, w, p), released(w, p, q).
    (proof:insert gone (proof:join (proof:relation (map (fn [[[x p] w]] [[w p] x]) (units owned)))
                                   rel-by (fn [_ x q] [q x])))
    # gone(x, q) :- remove(r, c, p0), owned(x, c, p0), released(r, p, q).
    (proof:insert gone (proof:join may (proof:relation (map (fn [[r p q]] [r q]) released))
                                   (fn [r x q] [q x])))
    (def step (proof:variable it "step"))   # [[x q] q2]
    (def defined (defined-in facts))
    (while (proof:changed? it)
      # gone(x, q2) :- gone(x, q), next(q, q2), not defined(x, q).
      (proof:from-join step gone (proof:relation (get facts :next)) (fn [q x q2] [[x q] q2]))
      (proof:from-antijoin gone step defined (fn [[x q] q2] [q2 x])))
    (map (fn [[q x]] [x q]) (proof:elements (proof:complete gone))))

  (defn with-captures [facts & waits]
    "FACTS with a store for each late capture, read from the first pass with WAITS, and with
     each jump whose value is live after it as a read."
    (def owned (ownership facts))
    (def live-r (liveness facts owned removal operand ;waits))
    (def gone (proof:relation (map (fn [xq] [xq true])
                                   (gone-of facts owned (released-from facts owned live-r)))))
    (def mutable (proof:antijoin (proof:relation (map (fn [[x k a]] [x [k a]]) (get facts :capture [])))
                                 (proof:relation (get facts :immutable []))
                                 (fn [x [k a]] [k [x a]])))
    # late(x, k, a) :- capture(x, k, a), not immutable(x), gone(x, q), live-as(k, q).
    (def k-live (proof:join mutable (live-as-of facts live-r) (fn [k [x a] q] [[x q] [k a]])))
    (def late (proof:join k-live gone (fn [[x q] [k a] _] [x k a])))
    (handed-over (put facts :store (concat (get facts :store []) (proof:elements late)))
                 removal operand captured ;waits))

  (defn judged [facts & waits]
    "The releases of FACTS under every rule and WAITS, once the late captures are stores."
    (released-all (with-captures facts ;waits) ;waits))

  (def c1 (fence 'c1 :doc "k borrows a, and each is released alone."
    :source "(defn c1 []
  (def a @[1 2])              # 0  a is made
  (def k (fn [] (length a)))  # 1  k is made, and captures a
  (k)                         # 2  k is used
  (length a))                 # 3  a is used
                              # 4  the exit"
    :facts {:next    (steps 4)
            :alloc   [[:a 0] [:k 1]]
            :capture [[:a :k 1]]
            :use     [[:k 2] [:a 3]]}
    :check (fn [c1] (= [[:k 2 3] [:a 3 4]] (judged c1)))))

  (def c2 (fence 'c2 :doc "The capture is late, so a moves into k and dies with it."
    :facts (put c1:facts :use [[:a 2] [:k 3]])        # (length a) at 2, then (k) at 3
    :check (fn [c2] (and (= [[:a :k 1]] ((with-captures c2) :store)) (= [[:k 3 4]] (judged c2))))))

  (def c3 (fence 'c3 :doc "A late capture of a borrow is refused."
    :source "(defn c3 []
  (def m @[])                 # 0  m is made
  (def y @[1])                # 1  y is made
  (push m y)                  # 2  y is stored into m
  (def x (get m 0))           # 3  x is read out of m
  (def k (fn [] (length x)))  # 4  k is made, and captures x
  (length m)                  # 5  m is used
  (k))                        # 6  k is used
                              # 7  the exit"
    :facts {:next    (steps 7)
            :alloc   [[:m 0] [:y 1] [:k 4]]
            :store   [[:y :m 2]]
            :read    [[:x :m 3]]
            :capture [[:x :k 4]]
            :use     [[:m 2] [:y 2] [:m 3] [:m 5] [:k 6]]}
    :check (fn [facts] (def c3 (with-captures facts)) (= [[:x :k 4]] (refusals c3 (ownership c3))))))

  (def c4 (fence 'c4 :doc "a waits for the closure that borrows it."
    :facts (put c2:facts :immutable [:a])
    :check (fn [c4] (= [[:k 3 4] [:a 3 4]] (judged c4)))))

  (def lc (fence 'lc :doc "k borrows a on each pass."
    :source "(defn lc [xs]
  (each x in xs               # 0  the loop head
    (def a @[1 2])            # 1  a is made
    (def k (fn [] (length a)))   # 2  k is made, and captures a
    (k)                       # 3  k is used
    (length a)))              # 4  a is used, and the loop goes back to 0
                              # 5  the exit"
    :facts {:next    [[0 1] [1 2] [2 3] [3 4] [4 0] [0 5]]
            :alloc   [[:a 1] [:k 2]]
            :capture [[:a :k 2]]
            :use     [[:k 3] [:a 4]]}
    :check (fn [lc] (empty? ((with-captures lc) :store)))))

  (def l5 (fence 'l5 :doc "A borrow captured on each pass is accepted."
    :source "(defn l5 [xs]
  (each x in xs               # 0  the loop head
    (def m @[@[1]])           # 1  m is made
    (def e (get m 0))         # 2  e is read out of m
    (def k (fn [] (length e)))   # 3  k is made, and captures e
    (k)                       # 4  k is used
    (length m)))              # 5  m is used, and the loop goes back to 0
                              # 6  the exit"
    :facts {:next    [[0 1] [1 2] [2 3] [3 4] [4 5] [5 0] [0 6]]
            :alloc   [[:m 1] [:k 3]]
            :read    [[:e :m 2]]
            :capture [[:e :k 3]]
            :use     [[:m 2] [:k 4] [:m 5]]}
    :check (fn [facts] (def l5 (with-captures facts)) (empty? (refusals l5 (ownership l5))))))

  (def lc2 (fence 'lc2 :doc "A capture late in its own pass is a store."
    :facts (put lc:facts :use [[:a 3] [:k 4]])   # (length a) at 3, then (k) at 4
    :check (fn [lc2] (= [[:a :k 2]] ((with-captures lc2) :store)))))

  {:captured captured :live-all live-all :released-all released-all :gone-of gone-of
   :with-captures with-captures :judged judged :fences [c1 c2 c3 c4 lc l5 lc2]})
```
