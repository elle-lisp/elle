# Group borrows, in Datalog

<!-- audited: 2026-11-22 -->

How a call into a group waits as the round before says its result borrows, as Datalog that runs on proof.

[Recursion](recursion.md) derives a group's signatures in rounds.
[Borrows](borrows.md) names what a result borrows from, and
[signatures](signature.md) holds the Group borrows rule in words.

## The rules

| Rule | Datalog |
|------|---------|
| Group borrows | `live(y, q) :- call(r, g, p), member(g), arg(y, i, p), borrows(g, [:param i]), live-as(r, q).` where `borrows` is the round before's |

`signature-in` in [recursion](recursion.md) runs each round's facts with the
wait that `lent` reads from the signatures of the round before, so the clause
holds in every round.

`swap-wrap` returns a fresh pair whose head is read out of `a`, and its tail
call swaps `a` and `b`. The base case borrows from `a`, so from the second round
on, the call waits on its argument 0, `b`, and the result borrows from `b` too.

`walk-graft` calls `reverse`, outside its group, whose signature the rounds
take as known. The base case borrows from `acc` through `reverse`. The tail
call waits on a new pair whose head is read out of `lst`, so the result borrows
from `lst` too. In `wg`, `xs` waits for the use of `r`:

```lisp
(fn []
  (def {:fence fence :calling calling} ((import "../spec.md")))
  (def {:steps steps} ((import "./datalog.md")))
  (def {:judged judged} ((import "./capture.md")))
  (def {:lent lent :after-calls after-calls} ((import "./borrows.md")))
  (def {:rounds rounds} ((import "./recursion.md")))

  (def swap-wrap (fence 'swap-wrap :doc "The result borrows from a and from b."
    :source "(defn swap-wrap [a b n]
  (if (= n 0)                 # 0  the test
    (pair (first a) ())       # 1  x is read out of a   2  v is made, with x as an operand
                              # 3  v is returned
    (swap-wrap b a (- n 1)))) # 4  m is the call's result   5  m is returned   6  the exit"
    :facts {:next      [[0 1] [1 2] [2 3] [3 6] [0 4] [4 5] [5 6]]
            :param     [[:a 0] [:b 1] [:n 2]]
            :read      [[:x :a 1]]
            :alloc     [[:v 2]]
            :operand   [[:x :v 2]]
            :immutable [:v]
            :call      [[:m :swap-wrap 4]]
            :arg       [[:b 0 4] [:a 1 4] [:k 2 4]]
            :use       [[:n 0] [:a 1] [:x 2]]
            :return    [[:v 3] [:m 5]]}
    :check (fn [swap-wrap] (= {:fresh [:swap-wrap] :returned [] :through [] :itself []
                               :borrows [[:swap-wrap [:param 0]] [:swap-wrap [:param 1]]]}
                              (rounds [[:swap-wrap swap-wrap]])))))

  (def reverse-sigs {:fresh [:reverse] :borrows [[:reverse [:param 0]]]})
  (def walk-graft (fence 'walk-graft :doc "The result borrows from lst and from acc."
    :source "(defn walk-graft [lst acc]
  (if (empty? lst)            # 0  lst is used
    (reverse acc)             # 1  v is the call's result   2  v is returned
    (walk-graft (rest lst)    # 3  t is read out of lst
      (pair (first lst)       # 4  h is read out of lst
            (pair 0 acc)))))  # 5  u is made, with acc as an operand
                              # 6  w is made, with h and u as operands
                              # 7  m is the call's result   8  m is returned   9  the exit"
    :facts {:next      [[0 1] [1 2] [2 9] [0 3] [3 4] [4 5] [5 6] [6 7] [7 8] [8 9]]
            :param     [[:lst 0] [:acc 1]]
            :read      [[:t :lst 3] [:h :lst 4]]
            :alloc     [[:u 5] [:w 6]]
            :operand   [[:acc :u 5] [:h :w 6] [:u :w 6]]
            :immutable [:u :w]
            :call      [[:v :reverse 1] [:m :walk-graft 7]]
            :arg       [[:acc 0 1] [:t 0 7] [:w 1 7]]
            :use       [[:lst 0] [:lst 3] [:lst 4]]
            :return    [[:v 2] [:m 8]]}
    :check (fn [walk-graft] (= [[:walk-graft [:param 0]] [:walk-graft [:param 1]]]
                               ((rounds [[:walk-graft walk-graft]] reverse-sigs) :borrows)))))

  (def walk-sigs (rounds [[:walk-graft walk-graft:facts]] reverse-sigs))
  (def wg (fence 'wg :doc "xs waits for r."
    :source (calling [walk-graft] "(defn wg []
  (def xs (list @[1] @[2]))   # 0  xs is made
  (def r (walk-graft xs ()))  # 1  r is made, and borrows from xs
  (length (first r)))         # 2  r is used   3  the exit")
    :facts {:next (steps 3) :alloc [[:xs 0]] :call [[:r :walk-graft 1]] :arg [[:xs 0 1]]
            :use [[:xs 1] [:r 2]]}
    :check (comp (fn [wg] (= [[:xs 2 3] [:r 2 3]] (judged wg (lent walk-sigs)))) (after-calls walk-sigs))))

  {:fences [swap-wrap walk-graft wg]})
```
