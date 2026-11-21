# A result in two states, in Datalog

<!-- audited: 2026-11-21 -->

How a value fresh on one path and a parameter or free name itself on another is refused after the join, as Datalog on proof.

[Return](return.md) derives `is`, and its fences are included here.
[Release](release.md) holds the Join clause for a value with an owner fact, and
[the ownership rules](ownership.md) state the Join rule in words.

## The rules

| Rule | Datalog |
|------|---------|
| Join | `refused(v, p) :- owned(v, :fiber, p), is(v, x), live(v, p).` |

A value fresh on one path and a parameter or free name itself on another is in
two states. A parameter carries no owner fact, so the Join clause of
[release](release.md) misses it, and the Join clause here reads `is` instead.
The refusal is part of the Join target ([the judgment](judgment.md)). A copy of
the code after the join per state does not reach a result, whose code after
the join lies in the caller. Below, `r` is a new array on one path and `a` on
the other:

```lisp
(include-file "return.md")

(defn itself-refusals [facts]
  "refused(v, p) as [v p], under the Join rule, where v is a parameter or free name itself."
  (def owned (ownership facts))
  (def fiber (fiber-held owned))
  # refused(v, p) :- owned(v, :fiber, p), is(v, x), live(v, p).
  (def named (proof:join fiber (is-of facts) (fn [v p x] [[v p] true])))
  (proof:elements (proof:join named (proof:map (liveness facts owned) (fn [vp] [vp true]))
                              (fn [vp _ _] vp))))

# (defn fresh-or [a c]
#   (if c                       # 0  the test
#     (array)                   # 1  v is made   2  v is handed to r
#     a))                       # 3  a is handed to r
#                               # 4  r is returned   5  the exit
(def fresh-or {:next   [[0 1] [1 2] [2 4] [0 3] [3 4] [4 5]]
               :param  [[:a 0]]
               :alloc  [[:v 1]]
               :pass   [[:v :r 2] [:a :r 3]]
               :return [[:r 4]]})
(assert (= [[:r 4] [:r 5]] (itself-refusals (returning fresh-or))) "r is fresh, or a itself")
(assert (empty? (mapcat (fn [f] (itself-refusals (returning f))) [either fill id]))
        "a result in one state is not refused")
(assert (empty? (itself-refusals (put fresh-or :return []))) "nor a value dead after the join")
```
