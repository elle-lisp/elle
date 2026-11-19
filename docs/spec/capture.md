# Captures in Datalog

<!-- audited: 2026-11-19 -->

How a closure borrows or takes each value it captures, decided in one pass, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts, [release](release.md) places each
release, and [removal](removal.md) adds the waits a removal needs.
[The ownership rules](ownership.md) state each rule in words.

| Derived | Holds when |
|---------|------------|
| `gone(x, q)` | in the first pass, the region of `x` has ended by step `q` |
| `late(x, k, a)` | in the first pass, the closure `k` is live where the region of `x` has ended |

## The rules

| Rule | Datalog |
|------|---------|
| Immutable capture | `live(x, p) :- capture(x, k, a), immutable(x), live(k, p).` |
| Mutable capture | `gone(x, q) :- released(x, p, q).` |
| Mutable capture | `gone(x, q) :- dies(x, w, p), released(w, p, q).` |
| Mutable capture | `gone(x, q) :- remove(r, c, p0), owned(x, c, p0), released(r, p, q).` |
| Mutable capture | `gone(x, q2) :- gone(x, q), next(q, q2).` |
| Mutable capture | `late(x, k, a) :- capture(x, k, a), not immutable(x), gone(x, q), live(k, q).` |
| Mutable capture | `store(x, k, a) :- late(x, k, a).` |

The first pass runs every rule but these, and `gone` and `late` read its
`released` and `live`. The program then runs again with the stores that `late`
adds and with the Immutable capture wait. A mutable capture that is not late is
a borrow, and its owner does not wait for the closure.

A store changes `owned`, and `live` reads `owned`, so a choice read from the
final pass would negate through recursion. The choice is made once. Iteration
to a fixpoint waits for a program that needs it.

A fiber made in a parameterize body captures each value the body binds, so
the Parameter inheritance rule gives a `capture` fact for each, and adds no
clause.

In `c1` below, `k` is used before the last use of `a`, so `k` borrows `a`. In
`c2` the order is the other way round, so the capture is late, and `a` moves
into `k` and is released with it:

```lisp
(include-file "removal.md")

(defn captured [facts owned it live live-by live-at]
  "The clause for live under the Immutable capture rule, for liveness."
  (def immutable (proof:relation (map (fn [x] [x true]) (get facts :immutable []))))
  (def by-closure (proof:join (proof:relation (map (fn [[x k a]] [x k]) (get facts :capture [])))
                              immutable (fn [x k _] [k x])))
  (fn []
    # live(x, p) :- capture(x, k, a), immutable(x), live(k, p).
    (proof:from-join live live-by by-closure (fn [k p x] [p x]))))

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
  (while (proof:changed? it)
    # gone(x, q2) :- gone(x, q), next(q, q2).
    (proof:from-join gone gone (proof:relation (get facts :next)) (fn [q x q2] [q2 x])))
  (map (fn [[q x]] [x q]) (proof:elements (proof:complete gone))))

(defn with-captures [facts]
  "FACTS with a store for each late capture, read from the first pass."
  (def owned (ownership facts))
  (def live-r (liveness facts owned removal operand))
  (def gone (proof:relation (map (fn [xq] [xq true])
                                 (gone-of facts owned (released-from facts owned live-r)))))
  (def mutable (proof:antijoin (proof:relation (map (fn [[x k a]] [x [k a]]) (get facts :capture [])))
                               (proof:relation (get facts :immutable []))
                               (fn [x [k a]] [k [x a]])))
  # late(x, k, a) :- capture(x, k, a), not immutable(x), gone(x, q), live(k, q).
  (def k-live (proof:join mutable live-r (fn [k [x a] q] [[x q] [k a]])))
  (def late (proof:join k-live gone (fn [[x q] [k a] _] [x k a])))
  (put facts :store (concat (get facts :store []) (proof:elements late))))

(defn judged [facts]
  "The releases of FACTS under every rule, once the late captures are stores."
  (def all (with-captures facts))
  (releases all (ownership all) removal operand captured))

# (defn c1 []
#   (def a @[1 2])              # 0  a is made
#   (def k (fn [] (length a)))  # 1  k is made, and captures a
#   (k)                         # 2  k is used
#   (length a))                 # 3  a is used
#                               # 4  the exit
(def c1 {:next    (map (fn [p] [p (+ p 1)]) (range 4))
         :alloc   [[:a 0] [:k 1]]
         :capture [[:a :k 1]]
         :use     [[:k 2] [:a 3]]})
(assert (= [[:k 2 3] [:a 3 4]] (judged c1)) "k borrows a, and each is released alone")

(def c2 (put c1 :use [[:a 2] [:k 3]]))        # (length a) at 2, then (k) at 3
(assert (= [[:a :k 1]] ((with-captures c2) :store)) "the capture is late, so a store")
(assert (= [[:k 3 4]] (judged c2)) "a moves into k, and dies with it")
```

In `c3` below, `x` is read out of `m`, and `k` is used after `m` is released.
So the capture is late, and the Store inside a unit rule refuses its store. In
`c4`, `a` is immutable, so `k` borrows it and `a` waits for `k`:

```lisp
# (defn c3 []
#   (def m @[])                 # 0  m is made
#   (def y @[1])                # 1  y is made
#   (push m y)                  # 2  y is stored into m
#   (def x (get m 0))           # 3  x is read out of m
#   (def k (fn [] (length x)))  # 4  k is made, and captures x
#   (length m)                  # 5  m is used
#   (k))                        # 6  k is used
#                               # 7  the exit
(def c3 (with-captures {:next    (map (fn [p] [p (+ p 1)]) (range 7))
                        :alloc   [[:m 0] [:y 1] [:k 4]]
                        :store   [[:y :m 2]]
                        :read    [[:x :m 3]]
                        :capture [[:x :k 4]]
                        :use     [[:m 2] [:y 2] [:m 3] [:m 5] [:k 6]]}))
(assert (= [[:x :k 4]] (refusals c3 (ownership c3))) "a late capture of a borrow is refused")

(def c4 (put c2 :immutable [:a]))
(assert (= [[:k 3 4] [:a 3 4]] (judged c4)) "a waits for the closure that borrows it")
```

## Not written yet

- Borrow before release for a mutable capture that is a borrow in the first
  pass and turns out late in the final one.
