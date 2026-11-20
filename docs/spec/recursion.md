# Recursion, in Datalog

<!-- audited: 2026-11-20 -->

How functions that call each other derive their signatures in rounds, starting from their base cases, as Datalog that runs on proof.

[Return](return.md) gives a result its kind, [itself](itself.md) finds a result
that is a parameter itself, and [borrows](borrows.md) names what a result
borrows from. The fences here include the third document.
[Signatures](signature.md) holds the Return and Group borrows rules in words.

| Fact | Holds when |
|------|------------|
| `member(g)` | the function `g` belongs to the group |

| Derived | Holds when |
|---------|------------|
| `into(p)` | step `p` calls a function of the group |
| `reach(p)` | in the first round, step `p` follows step 0 without passing a call into the group |

## The rules

| Rule | Datalog |
|------|---------|
| Return | `into(p) :- call(r, g, p), member(g).` |
| Return | `reach(0).` |
| Return | `reach(q) :- reach(p), next(p, q), not into(p).` |
| Group borrows | `live(y, q) :- call(r, g, p), member(g), arg(y, i, p), borrows(g, [:param i]), live-as(r, q).` where `borrows` is the round before's |

A group is a function that calls itself, or functions that call each other.
Each function's result kind reads the signatures of the group, so the group
derives them together, in rounds. In the first round, each fact at a step `p`
holds only where `reach(p)` holds, so the round sees the base cases alone. Each
later round reads the signatures of the round before, and the derivation stops
when a round derives the signatures of the round before.

The first round treats a call into the group as one that does not return.
It sees the fewest returned values, so it strays the fewest parameters, and
`itself` holds the most. A withdrawn `itself` makes the call's result an origin
of its own, which strays more parameters, so `itself` only falls from round to
round. `fresh`, `through`, and `returned` or `itself` together only grow.
`borrows` only grows too: a wait only adds live facts, and a falling `itself`
turns a hand-over into a read and lifts the `not itself` condition. Each is
finite, so the rounds stop.

The argument needs each function of the group to start from its base cases.
Where `ev` below starts from every path and `od` from its base cases, the two
trade `itself` in every round and never stop.

The signatures where the rounds stop are sound by induction on the depth of a
call. Where each call of a smaller depth returns as the signatures say, the
body returns as they say too. Below, `consume2` returns `a` itself on the
base case, and its tail call hands `a` on, so `a` stays itself. `count-list`
returns `acc` itself on its base case, but its call hands on a new value, so
the second round withdraws `itself`:

```lisp
(include-file "borrows.md")

(defn signature-in [f facts sigs]
  "The fresh, returned, through, itself and borrows facts of F, from its FACTS, where each call
   reads the signatures SIGS and waits as their borrows facts say."
  (def wait (lent sigs))
  (def all (calls (results facts sigs) sigs wait))
  (def s (signature f all))
  (def mine (itself-of f all))
  # itself replaces returned
  (merge s {:itself   mine
            :returned (filter (fn [fi] (not (any? (fn [g] (= g fi)) mine))) (s :returned))
            :borrows  (borrows-of f all wait)}))

(defn base-cases [facts group]
  "FACTS at the steps that follow step 0 without passing a call into GROUP."
  # into(p) :- call(r, g, p), member(g).
  (def into (map (fn [[r g p]] p)
                 (filter (fn [[r g p]] (any? (fn [h] (= h g)) group)) (get facts :call []))))
  (def it (proof:iteration))
  (def reach (proof:variable it "reach"))
  # reach(0).
  (proof:extend reach [[0 true]])
  (while (proof:changed? it)
    # reach(q) :- reach(p), next(p, q), not into(p).
    (proof:from-join reach reach
                     (proof:relation (filter (fn [[p q]] (not (any? (fn [c] (= c p)) into)))
                                             (get facts :next)))
                     (fn [p _ q] [q true])))
  (def at (map (fn [[p _]] p) (proof:elements (proof:complete reach))))
  (defn here? [t] (any? (fn [p] (= p (get t (- (length t) 1)))) at))
  (merge (put facts :next (filter (fn [[p q]] (here? [p])) (get facts :next)))
         (struct ;(mapcat (fn [k] [k (filter here? (get facts k []))])
                          [:alloc :read :store :remove :capture :pass :operand :use :return
                           :call :arg]))))

(defn rounds [members & known]
  "The signatures of MEMBERS, each [f facts], a group, derived in rounds from its base cases,
   and KNOWN, the signatures a call out of the group reads."
  (def outside (get known 0 {}))
  (def group (map (fn [[f _]] f) members))
  (defn with-known [sigs]
    (struct ;(mapcat (fn [k] [k (concat (get outside k []) (get sigs k []))])
                     [:fresh :returned :through :itself :borrows :stored])))
  (defn round [sigs restrict]
    (def each-sig (map (fn [[f facts]] (signature-in f (restrict facts) (with-known sigs)))
                       members))
    (struct ;(mapcat (fn [k] [k (mapcat (fn [s] (s k)) each-sig)])
                     [:fresh :returned :through :itself :borrows])))
  (var sigs (round {} (fn [facts] (base-cases facts group))))
  (var before nil)
  (while (not= sigs before)
    (assign before sigs)
    (assign sigs (round before identity)))
  sigs)

# (defn consume2 [a depth]
#   (if (%lt depth 1)           # 0  the test
#     a                         # 1  a is handed to r
#     (consume2 a (%add depth -1))))   # 2  m is the call's result   3  m is handed to r
#                               # 4  r is returned   5  the exit
(def consume2 {:next   [[0 1] [1 4] [0 2] [2 3] [3 4] [4 5]]
               :param  [[:a 0] [:depth 1]]
               :call   [[:m :consume2 2]]
               :arg    [[:a 0 2] [:d 1 2]]
               :pass   [[:a :r 1] [:m :r 3]]
               :use    [[:depth 0] [:a 2]]
               :return [[:r 4]]})
(assert (= {:fresh [] :returned [] :through [] :itself [[:consume2 0]] :borrows []}
           (rounds [[:consume2 consume2]]))
        "a tail loop returns its parameter itself")

# (defn count-list [lst acc]
#   (if (empty? lst)            # 0  lst is used
#     acc                       # 1  acc is returned
#     (count-list (rest lst)    # 2  t is read out of lst
#                 (+ acc 1))))  # 3  t and a are passed   4  n is returned   5  the exit
(def count-list {:next   [[0 1] [0 2] [1 5] [2 3] [3 4] [4 5]]
                 :param  [[:lst 0] [:acc 1]]
                 :read   [[:t :lst 2]]
                 :call   [[:n :count-list 3]]
                 :arg    [[:t 0 3] [:a 1 3]]
                 :use    [[:lst 0] [:lst 2] [:t 3]]
                 :return [[:acc 1] [:n 4]]})
(assert (= [[:count-list 1]]
           ((signature-in :count-list (base-cases count-list [:count-list]) {}) :itself))
        "the base case alone returns acc itself")
(assert (= {:fresh [] :returned [[:count-list 1]] :through [] :itself []
            :borrows [[:count-list [:param 1]]]}
           (rounds [[:count-list count-list]]))
        "the call hands on a new value, so the result is an alias of acc")
```

`build` makes a new array on its base case, and `last-of` reads an element out
of its parameter. `ev` and `od` call each other, and each returns `x` itself:

```lisp
# (defn build [n]
#   (if (= n 0)                 # 0  the test
#     (array)                   # 1  v is made   2  v is handed to r
#     (build (- n 1))))         # 3  m is the call's result   4  m is handed to r
#                               # 5  r is returned   6  the exit
(def build {:next   [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5] [5 6]]
            :param  [[:n 0]]
            :alloc  [[:v 1]]
            :call   [[:m :build 3]]
            :arg    [[:k 0 3]]
            :pass   [[:v :r 2] [:m :r 4]]
            :use    [[:n 0]]
            :return [[:r 5]]})
# (defn last-of [xs]
#   (if (empty? (rest xs))      # 0  t is read out of xs
#     (first xs)                # 1  h is read out of xs   2  h is handed to r
#     (last-of (rest xs))))     # 3  u is read out of xs   4  m is the call's result
#                               # 5  m is handed to r   6  r is returned   7  the exit
(def last-of {:next   [[0 1] [1 2] [2 6] [0 3] [3 4] [4 5] [5 6] [6 7]]
              :param  [[:xs 0]]
              :read   [[:t :xs 0] [:h :xs 1] [:u :xs 3]]
              :call   [[:m :last-of 4]]
              :arg    [[:u 0 4]]
              :pass   [[:h :r 2] [:m :r 5]]
              :use    [[:xs 0] [:t 0] [:xs 1] [:xs 3] [:u 4]]
              :return [[:r 6]]})
(assert (= [:build] ((rounds [[:build build]]) :fresh)) "build is fresh")
(assert (= {:fresh [] :returned [[:last-of 0]] :through [] :itself []
            :borrows [[:last-of [:param 0]]]}
           (rounds [[:last-of last-of]]))
        "last-of is an alias of xs")

# (defn ev [x n] (if (= n 0) x (od x (- n 1))))   with od the same, calling ev
(defn parity [g] (merge consume2 {:call [[:m g 2]]}))
(assert (= [[:ev 0] [:od 0]] ((rounds [[:ev (parity :od)] [:od (parity :ev)]]) :itself))
        "a group derives its signatures together")
```

`swap-wrap` returns a fresh pair whose head is read out of `a`, and its tail
call swaps `a` and `b`. The base case borrows from `a`, so from the second round
on, the call waits on its argument 0, `b`, and the result borrows from `b` too:

```lisp
# (defn swap-wrap [a b n]
#   (if (= n 0)                 # 0  the test
#     (pair (first a) ())       # 1  x is read out of a   2  v is made, with x as an operand
#                               # 3  v is returned
#     (swap-wrap b a (- n 1)))) # 4  m is the call's result   5  m is returned   6  the exit
(def swap-wrap {:next      [[0 1] [1 2] [2 3] [3 6] [0 4] [4 5] [5 6]]
                :param     [[:a 0] [:b 1] [:n 2]]
                :read      [[:x :a 1]]
                :alloc     [[:v 2]]
                :operand   [[:x :v 2]]
                :immutable [:v]
                :call      [[:m :swap-wrap 4]]
                :arg       [[:b 0 4] [:a 1 4] [:k 2 4]]
                :use       [[:n 0] [:a 1] [:x 2]]
                :return    [[:v 3] [:m 5]]})
(assert (= {:fresh [:swap-wrap] :returned [] :through [] :itself []
            :borrows [[:swap-wrap [:param 0]] [:swap-wrap [:param 1]]]}
           (rounds [[:swap-wrap swap-wrap]]))
        "the result borrows from a and from b")
```

`walk-graft` calls `reverse`, outside its group, whose signature the rounds
take as known. The base case borrows from `acc` through `reverse`. The tail
call waits on a new pair whose head is read out of `lst`, so the result borrows
from `lst` too. In `wg`, `xs` waits for the use of `r`:

```lisp
# (defn walk-graft [lst acc]
#   (if (empty? lst)            # 0  lst is used
#     (reverse acc)             # 1  v is the call's result   2  v is returned
#     (walk-graft (rest lst)    # 3  t is read out of lst
#       (pair (first lst)       # 4  h is read out of lst
#             (pair 0 acc)))))  # 5  u is made, with acc as an operand
#                               # 6  w is made, with h and u as operands
#                               # 7  m is the call's result   8  m is returned   9  the exit
(def walk-graft {:next      [[0 1] [1 2] [2 9] [0 3] [3 4] [4 5] [5 6] [6 7] [7 8] [8 9]]
                 :param     [[:lst 0] [:acc 1]]
                 :read      [[:t :lst 3] [:h :lst 4]]
                 :alloc     [[:u 5] [:w 6]]
                 :operand   [[:acc :u 5] [:h :w 6] [:u :w 6]]
                 :immutable [:u :w]
                 :call      [[:v :reverse 1] [:m :walk-graft 7]]
                 :arg       [[:acc 0 1] [:t 0 7] [:w 1 7]]
                 :use       [[:lst 0] [:lst 3] [:lst 4]]
                 :return    [[:v 2] [:m 8]]})
(def walk-sigs (rounds [[:walk-graft walk-graft]]
                       {:fresh [:reverse] :borrows [[:reverse [:param 0]]]}))
(assert (= [[:walk-graft [:param 0]] [:walk-graft [:param 1]]] (walk-sigs :borrows))
        "the result borrows from lst and from acc")

# (defn wg []
#   (def xs (list @[1] @[2]))   # 0  xs is made
#   (def r (walk-graft xs ()))  # 1  r is made, and borrows from xs
#   (length (first r)))         # 2  r is used   3  the exit
(def wg (calls (results {:next (steps 3) :alloc [[:xs 0]] :call [[:r :walk-graft 1]]
                         :arg [[:xs 0 1]] :use [[:xs 1] [:r 2]]} walk-sigs)
               walk-sigs (lent walk-sigs)))
(assert (= [[:xs 2 3] [:r 2 3]] (judged wg (lent walk-sigs))) "xs waits for r")
```
