# The callee of a stored parameter, in Datalog

<!-- audited: 2026-11-20 -->

How the callee owns a stored parameter on entry, and how a parameter with two places is refused, as Datalog that runs on proof.

[Stored](stored.md) derives the `stored` fact, and its fences are included
here. [Signatures](signature.md) holds the Stored parameter rule and the One
place target in words, and [the judgment](judgment.md) lists the target.

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
`x`, which has no owner fact:

```lisp
(include-file "stored.md")

# alloc(x, :entry) :- stored(f, i, t), param(x, i).
(def kp (instance keep (map (fn [[f i t]] i) (stored-of :keep keep))))
(assert (empty? (refusals kp (ownership kp))) "keep owns x, so its store moves it")
(assert (= [[:x :acc 0]] (refusals keep (ownership keep))) "without the clause it is refused")
```

A parameter stored into a local container alone reaches no place. The Sink
rule then lends or sinks it: a sunk instance accepts the store, and a lent one
refuses it under the Store inside a unit rule. A `lends` fact withheld from
such a parameter would refuse the same calls at the call: a value the call
cannot sink is one the callee cannot own.

```lisp
# (defn into-local [x]          # 0  a is made   1  x is stored into a
#   (def a @[]) (push a x) (length a))   # 2  n is returned   3  the exit
(def into-local {:next (steps 3) :param [[:x 0]] :alloc [[:a 0]] :store [[:x :a 1]]
                 :use [[:a 1] [:x 1]] :return [[:n 2]]})
(assert (empty? (stored-of :into-local into-local)) "a local container is no place")
(assert (= [[:into-local 0]] (lends-of :into-local into-local {})) "so the call may lend x")
(assert (= [[:x :a 1]] (refusals into-local (ownership into-local))) "a lent x is refused")
(def il (instance into-local [0]))
(assert (empty? (refusals il (ownership il))) "a sunk x is stored")
```

A parameter that reaches two places is refused, under the One place target.
The caller would move one argument to two owners. `split` stores `x` into
`acc` on one path and into `log` on the other, and `into-either` stores `x`
into `r`, which is `a` or `b`:

```lisp
(defn one-place [stored]
  "refused(f, i) as [f i], under the One place target, from the STORED facts."
  # refused(f, i) :- stored(f, i, t), stored(f, i, u), t ≠ u.
  (def by (proof:relation (map (fn [[f i t]] [[f i] t]) stored)))
  (distinct (map (fn [[fi t u]] fi) (filter (fn [[fi t u]] (not= t u))
                                            (proof:elements (proof:join by by (fn [fi t u] [fi t u])))))))

# (defn split [acc x c]
#   (if c (push acc x)          # 0  the test   1  x is stored into acc
#     (push log x)) nil)        # 2  x is stored into log   3  n is returned   4  the exit
(def split {:next [[0 1] [0 2] [1 3] [2 3] [3 4]] :param [[:acc 0] [:x 1]] :free [:log]
            :store [[:x :acc 1] [:x :log 2]] :use [[:acc 1] [:x 1] [:log 2] [:x 2]]
            :return [[:n 3]]})
# (defn into-either [a b x c]
#   (def r (if c a b))          # 0  the test   1  a is handed to r   2  b is handed to r
#   (push r x) nil)             # 3  x is stored into r   4  n is returned   5  the exit
(def into-either {:next [[0 1] [0 2] [1 3] [2 3] [3 4] [4 5]] :param [[:a 0] [:b 1] [:x 2]]
                  :pass [[:a :r 1] [:b :r 2]] :store [[:x :r 3]] :use [[:r 3] [:x 3]]
                  :return [[:n 4]]})
(assert (= [[:into-either 2] [:split 1]]
           (one-place (mapcat (fn [[f facts]] (stored-of f facts))
                              [[:split split] [:into-either into-either] [:via via]])))
        "a parameter with two places is refused")
```
