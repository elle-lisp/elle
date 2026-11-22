# The callee of a stored parameter, in Datalog

<!-- audited: 2026-11-22 -->

How the callee owns a stored parameter on entry, and how a parameter with two places is refused, as Datalog that runs on proof.

[Stored](stored.md) derives the `stored` fact. [Signatures](signature.md) holds
the Stored parameter rule and the One place target in words, and
[the judgment](judgment.md) lists the target.

| Derived | Holds when |
|---------|------------|
| `refused(f, i)` | the parameter `i` of `f` has two places |

## The rules

| Rule | Datalog |
|------|---------|
| Stored parameter | `alloc(x, :entry) :- stored(f, i, t), param(x, i).` |
| One place | `refused(f, i) :- stored(f, i, t), stored(f, i, u), t ≠ u.` |

The call moves a stored argument, so the callee's code owns the parameter on
entry, as the probe does. Without that, `keep` would refuse its own store of
`x`, which has no owner fact.

A parameter stored into a local container alone reaches no place, as in
`into-local`. The Sink rule then lends or sinks it: a sunk instance accepts the
store, and a lent one refuses it under the Store inside a unit rule. A `lends`
fact withheld from such a parameter would refuse the same calls at the call: a
value the call cannot sink is one the callee cannot own.

A parameter that reaches two places is refused, under the One place target.
The caller would move one argument to two owners. `split` stores `x` into
`acc` on one path and into `log` on the other, and `into-either` stores `x`
into `r`, which is `a` or `b`:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence :fence-from fence-from} ((import "../spec.md")))
  (def {:refusals refusals :ownership ownership :steps steps} ((import "./datalog.md")))
  (def {:instance instance :lends-of lends-of} ((import "./sink.md")))
  (def stored ((import "./stored.md")))
  (def {:stored-of stored-of} stored)

  (defn one-place [stored]
    "refused(f, i) as [f i], under the One place target, from the STORED facts."
    # refused(f, i) :- stored(f, i, t), stored(f, i, u), t ≠ u.
    (def by (proof:relation (map (fn [[f i t]] [[f i] t]) stored)))
    (distinct (map (fn [[fi t u]] fi) (filter (fn [[fi t u]] (not= t u))
                                              (proof:elements (proof:join by by (fn [fi t u] [fi t u])))))))

  (def keep (fence-from stored 'keep
    :doc "keep owns x, so its store moves it, and without the clause the store is refused."
    :check (fn [keep]
             # alloc(x, :entry) :- stored(f, i, t), param(x, i).
             (def kp (instance keep (map (fn [[f i t]] i) (stored-of :keep keep))))
             (and (empty? (refusals kp (ownership kp)))
                  (= [[:x :acc 0]] (refusals keep (ownership keep)))))))

  (def into-local (fence 'into-local
    :doc "A local container is no place, so the call may lend x, which is refused, or sink it."
    :source "(defn into-local [x]          # 0  a is made   1  x is stored into a
  (def a @[]) (push a x) (length a))   # 2  n is returned   3  the exit"
    :facts {:next (steps 3) :param [[:x 0]] :alloc [[:a 0]] :store [[:x :a 1]]
            :use [[:a 1] [:x 1]] :return [[:n 2]]}
    :check (fn [into-local]
             (def il (instance into-local [0]))
             (and (empty? (stored-of :into-local into-local))
                  (= [[:into-local 0]] (lends-of :into-local into-local {}))
                  (= [[:x :a 1]] (refusals into-local (ownership into-local)))
                  (empty? (refusals il (ownership il)))))))

  (def split (fence 'split :doc "split stores x into two places, and is refused."
    :source "(defn split [acc x c]
  (if c (push acc x)          # 0  the test   1  x is stored into acc
    (push log x)) nil)        # 2  x is stored into log   3  n is returned   4  the exit"
    :facts {:next [[0 1] [0 2] [1 3] [2 3] [3 4]] :param [[:acc 0] [:x 1]] :free [:log]
            :store [[:x :acc 1] [:x :log 2]] :use [[:acc 1] [:x 1] [:log 2] [:x 2]]
            :return [[:n 3]]}
    :check (fn [split] (= [[:split 1]] (one-place (stored-of :split split))))))

  (def into-either (fence 'into-either :doc "into-either stores x into r, which is a or b, and is refused."
    :source "(defn into-either [a b x c]
  (def r (if c a b))          # 0  the test   1  a is handed to r   2  b is handed to r
  (push r x) nil)             # 3  x is stored into r   4  n is returned   5  the exit"
    :facts {:next [[0 1] [0 2] [1 3] [2 3] [3 4] [4 5]] :param [[:a 0] [:b 1] [:x 2]]
            :pass [[:a :r 1] [:b :r 2]] :store [[:x :r 3]] :use [[:r 3] [:x 3]]
            :return [[:n 4]]}
    :check (fn [ie] (= [[:into-either 2]] (one-place (stored-of :into-either ie))))))

  (def via (fence-from stored 'via :doc "via stores x into one place."
                       :check (fn [via] (empty? (one-place (stored-of :via via))))))

  {:one-place one-place :fences [keep into-local split into-either via]})
```
