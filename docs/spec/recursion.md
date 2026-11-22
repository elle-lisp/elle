# Recursion, in Datalog

<!-- audited: 2026-11-22 -->

How functions that call each other derive their signatures in rounds, starting from their base cases, as Datalog that runs on proof.

[Return](return.md) gives a result its kind, [itself](itself.md) finds a result
that is a parameter itself, and [borrows](borrows.md) names what a result
borrows from.
[Signatures](signature.md) holds the Return and Pending member rules in words,
and [group](group.md) writes the Group borrows rule.

| Fact | Holds when |
|------|------------|
| `member(g)` | the function `g` belongs to the group |

| Derived | Holds when |
|---------|------------|
| `into(p)` | step `p` calls a function of the group |
| `reach(p)` | in the round, step `p` follows step 0 without passing a step where `into` holds |
| `answered(g)` | in the round before, a return of `g` is a step where `reach` holds |

## The rules

| Rule | Datalog |
|------|---------|
| Return | `into(p) :- call(r, g, p), member(g).` in the first round |
| Return | `reach(0).` |
| Return | `reach(q) :- reach(p), next(p, q), not into(p).` |
| Pending member | `answered(g) :- return(v, p), reach(p).` over the facts of `g` in the round before |
| Pending member | `into(p) :- call(r, g, p), member(g), not answered(g).` in each round after the first |

A group is a function that calls itself, or functions that call each other.
Each function's result kind reads the signatures of the group, so the group
derives them together, in rounds. In the first round, each fact at a step `p`
holds only where `reach(p)` holds, so the round sees the base cases alone. Each
later round reads the signatures of the round before, and cuts its facts the
same way at each call into a member that answered nothing in the round before.
The derivation stops when a round derives the signatures, and cuts the
members, of the round before.

The first round treats a call into the group as one that does not return.
It sees the fewest returned values, so it strays the fewest parameters, and
`itself` holds the most. A member that answered keeps its returns, so each
round cuts fewer members and sees more paths. A withdrawn `itself` makes the
call's result an origin of its own, which strays more parameters, so `itself`
only falls from round to round. A member that answered returns a value that
starts at the parameter, and that value is then read out of it, so `fresh`,
`through`, and `returned` or `itself` together only grow. `borrows` only grows
too: a wait only adds live facts, and a falling `itself` turns a hand-over into
a read and lifts the `not itself` condition. Each is finite, so the rounds stop.

A member that answered nothing has `itself` for each parameter it does not
store, and that `itself` falls with nothing in its place. The Pending member
rule cuts each call into it, so no round reads that `itself`.

The argument needs each function of the group to start from its base cases.
Where `ev` below starts from every path and `od` from its base cases, the two
trade `itself` in every round and never stop.

The signatures where the rounds stop are sound by induction on the depth of a
call. Where each call of a smaller depth returns as the signatures say, the
body returns as they say too. `consume2` returns `a` itself on the base case,
and its tail call hands `a` on, so `a` stays itself. `count-list` returns `acc`
itself on its base case, but its call hands on a new value, so the second round
withdraws `itself`.

`build` makes a new array on its base case, and `last-of` reads an element out
of its parameter. `ev` and `od` call each other, and each returns `x` itself.

`ev2` returns `nil` on its base case, and `od2` only calls it. In the first
round `od2` answers nothing, so the second round cuts the call to it. Read
instead, the `itself` of `od2` would make `returned` pass between the two in
every round:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence} ((import "../spec.md")))
  (def {:steps steps} ((import "./datalog.md")))
  (def {:calls calls} ((import "./call.md")))
  (def {:results results :signature signature} ((import "./return.md")))
  (def {:itself-of itself-of} ((import "./itself.md")))
  (def {:lent lent :borrows-of borrows-of} ((import "./borrows.md")))

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
    (defn round [[sigs cut]]
      "[sigs cut]: the round's signatures, read from facts cut at each call into CUT, and the
       members that answered nothing."
      (def each-sig (map (fn [[f facts]] (def mine (base-cases facts cut))
                           [(signature-in f mine (with-known sigs)) f (empty? (get mine :return []))])
                         members))
      [(struct ;(mapcat (fn [k] [k (mapcat (fn [[s]] (s k)) each-sig)])
                        [:fresh :returned :through :itself :borrows]))
       # into(p) :- call(r, g, p), member(g), not answered(g).
       (map (fn [[_ f]] f) (filter (fn [[_ _ none]] none) each-sig))])
    (var state (round [{} group]))   # in the first round, no member has answered
    (var before nil)
    (while (not= state before)
      (assign before state)
      (assign state (round before)))
    (get state 0))

  (def consume2 (fence 'consume2 :doc "A tail loop returns its parameter itself."
    :source "(defn consume2 [a depth]
  (if (%lt depth 1)           # 0  the test
    a                         # 1  a is handed to r
    (consume2 a (%add depth -1))))   # 2  m is the call's result   3  m is handed to r
                              # 4  r is returned   5  the exit"
    :facts {:next   [[0 1] [1 4] [0 2] [2 3] [3 4] [4 5]]
            :param  [[:a 0] [:depth 1]]
            :call   [[:m :consume2 2]]
            :arg    [[:a 0 2] [:d 1 2]]
            :pass   [[:a :r 1] [:m :r 3]]
            :use    [[:depth 0] [:a 2]]
            :return [[:r 4]]}
    :check (fn [consume2] (= {:fresh [] :returned [] :through [] :itself [[:consume2 0]] :borrows []}
                             (rounds [[:consume2 consume2]])))))

  (def count-list (fence 'count-list
    :doc "The base case alone returns acc itself, but the call hands on a new value, so the result is an alias of acc."
    :source "(defn count-list [lst acc]
  (if (empty? lst)            # 0  lst is used
    acc                       # 1  acc is returned
    (count-list (rest lst)    # 2  t is read out of lst
                (+ acc 1))))  # 3  t and a are passed   4  n is returned   5  the exit"
    :facts {:next   [[0 1] [0 2] [1 5] [2 3] [3 4] [4 5]]
            :param  [[:lst 0] [:acc 1]]
            :read   [[:t :lst 2]]
            :call   [[:n :count-list 3]]
            :arg    [[:t 0 3] [:a 1 3]]
            :use    [[:lst 0] [:lst 2] [:t 3]]
            :return [[:acc 1] [:n 4]]}
    :check (fn [count-list]
             (and (= [[:count-list 1]]
                     ((signature-in :count-list (base-cases count-list [:count-list]) {}) :itself))
                  (= {:fresh [] :returned [[:count-list 1]] :through [] :itself []
                      :borrows [[:count-list [:param 1]]]}
                     (rounds [[:count-list count-list]]))))))

  (def build (fence 'build :doc "build is fresh."
    :source "(defn build [n]
  (if (= n 0)                 # 0  the test
    (array)                   # 1  v is made   2  v is handed to r
    (build (- n 1))))         # 3  m is the call's result   4  m is handed to r
                              # 5  r is returned   6  the exit"
    :facts {:next   [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5] [5 6]]
            :param  [[:n 0]]
            :alloc  [[:v 1]]
            :call   [[:m :build 3]]
            :arg    [[:k 0 3]]
            :pass   [[:v :r 2] [:m :r 4]]
            :use    [[:n 0]]
            :return [[:r 5]]}
    :check (fn [build] (= [:build] ((rounds [[:build build]]) :fresh)))))

  (def last-of (fence 'last-of :doc "last-of is an alias of xs."
    :source "(defn last-of [xs]
  (if (empty? (rest xs))      # 0  t is read out of xs
    (first xs)                # 1  h is read out of xs   2  h is handed to r
    (last-of (rest xs))))     # 3  u is read out of xs   4  m is the call's result
                              # 5  m is handed to r   6  r is returned   7  the exit"
    :facts {:next   [[0 1] [1 2] [2 6] [0 3] [3 4] [4 5] [5 6] [6 7]]
            :param  [[:xs 0]]
            :read   [[:t :xs 0] [:h :xs 1] [:u :xs 3]]
            :call   [[:m :last-of 4]]
            :arg    [[:u 0 4]]
            :pass   [[:h :r 2] [:m :r 5]]
            :use    [[:xs 0] [:t 0] [:xs 1] [:xs 3] [:u 4]]
            :return [[:r 6]]}
    :check (fn [last-of] (= {:fresh [] :returned [[:last-of 0]] :through [] :itself []
                             :borrows [[:last-of [:param 0]]]}
                            (rounds [[:last-of last-of]])))))

  (defn parity [g] (merge consume2:facts {:call [[:m g 2]]}))

  # (defn ev [x n] (if (= n 0) x (od x (- n 1))))   with od the same, calling ev
  (def ev (fence 'ev :doc "ev and od derive their signatures together, each returning x itself."
    :facts (parity :od)
    :check (fn [ev] (= [[:ev 0] [:od 0]] ((rounds [[:ev ev] [:od (parity :ev)]]) :itself)))))

  # (defn ev2 [a depth] (if (%lt depth 1) nil (od2 a (%add depth -1))))
  (def ev2 (merge (parity :od2) {:pass [[:z :r 1] [:m :r 3]]}))
  (def od2 (fence 'od2 :doc "A member that answered nothing is cut, so the group returns no parameter."
    :source "(defn od2 [a depth]
  (ev2 a depth))              # 0  m is the call's result   1  m is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :param [[:a 0] [:depth 1]] :call [[:m :ev2 0]]
            :arg [[:a 0 0] [:d 1 0]] :use [[:a 0]] :return [[:m 1]]}
    :check (fn [od2] (= {:fresh [] :returned [] :through [] :itself [] :borrows []}
                        (rounds [[:ev2 ev2] [:od2 od2]])))))

  {:signature-in signature-in :base-cases base-cases :rounds rounds
   :fences [consume2 count-list build last-of ev od2]})
```
