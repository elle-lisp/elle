# Returned itself, in Datalog

<!-- audited: 2026-11-21 -->

How a function's result is found to be one parameter itself on every path, as Datalog that runs on proof.

[Return](return.md) gives a result its kind, and [states](states.md) refuses
one in two states. The fences here include the second document, so they run
the first too.
[Call](call.md) reads the `itself` fact, and [signatures](signature.md) holds
the Returned itself rule in words.

| Derived | Holds when |
|---------|------------|
| `target(x)` | a pass defines `x` |
| `origin(v, z)` | the chain of passes that defines `v` may start at `z` |
| `stray(x)` | a returned value may start somewhere other than the parameter `x` |
| `taken(z)` | the function stores a value that may start at `z` |

## The rules

| Rule | Datalog |
|------|---------|
| Returned itself | `target(x) :- pass(y, x, p).` |
| Returned itself | `origin(v, v) :- return(v, p), not target(v).` |
| Returned itself | `origin(y, y) :- pass(y, x, p), not target(y).` |
| Returned itself | `origin(x, z) :- pass(y, x, p), origin(y, z).` |
| Returned itself | `stray(x) :- param(x, i), return(v, p), origin(v, z), z ≠ x.` |
| Returned itself | `taken(z) :- store(v, c, p), origin(v, z).` |
| Returned itself | `itself(f, i) :- param(x, i), not stray(x), not taken(x).` |

A value that no pass defines starts its own chain: a parameter, a free name, a
value the function makes or reads, or a constant. So the result is the
parameter `x` itself on every path where each returned value starts at `x`. A
store of a value that may start at `x` keeps it in the callee, and a kept value
is not the fiber's to hand over.

The facts are those after [calls](call.md). A callee's `itself` fact makes its
call pass the argument on, and the other facts of its signature make the
result a read or a new value. A function that never returns has `itself` for
each parameter it does not store. Nothing follows such a call, so the
hand-over changes no release.

Below, `id` returns `x` itself, and `randomize` stores into `g` but does not
store `g`. `keep-it` stores `x`, and `kept-r` stores it under the name `r`.
`maybe` returns `nil` on one path, and the functions of [return](return.md)
and [states](states.md) return a value they make or read, or one of two
parameters:

```lisp
(include-file "states.md")

(defn origins [facts]
  "origin(v, z) as a relation of [v z], under the Returned itself rule."
  (def pass (get facts :pass []))
  # origin(v, v) :- return(v, p), not target(v).  origin(y, y) :- pass(y, x, p), not target(y).
  (def values (concat (map (fn [[v p]] v) (get facts :return [])) (map (fn [[y x p]] y) pass)))
  # target(x) :- pass(y, x, p).
  (def base (proof:antijoin (proof:relation (map (fn [v] [v v]) values))
                            (proof:relation (map (fn [[y x p]] x) pass)) (fn [v z] [v z])))
  (def it (proof:iteration))
  (def origin (proof:variable it "origin"))
  (proof:insert origin base)
  (while (proof:changed? it)
    # origin(x, z) :- pass(y, x, p), origin(y, z).
    (proof:from-join origin origin (proof:relation (map (fn [[y x p]] [y x]) pass))
                     (fn [y z x] [x z])))
  (proof:complete origin))

(defn itself-of [f facts]
  "itself(f, i) as [f i], from the FACTS of F, under the Returned itself rule."
  (def origin (origins facts))
  # stray(x) :- param(x, i), return(v, p), origin(v, z), z ≠ x.
  (def zs (proof:elements (proof:join (proof:relation (map (fn [[v p]] [v true])
                                                           (get facts :return [])))
                                      origin (fn [v _ z] z))))
  (def stray (filter (fn [x] (any? (fn [z] (not= z x)) zs))
                     (map (fn [[x i]] x) (get facts :param []))))
  # taken(z) :- store(v, c, p), origin(v, z).
  (def taken (proof:join (proof:relation (map (fn [[v c p]] [v true])
                                              (get (operands (with-captures facts)) 1)))
                         origin (fn [v _ z] z)))
  # itself(f, i) :- param(x, i), not stray(x), not taken(x).
  (proof:elements (proof:antijoin (proof:antijoin (proof:relation (get facts :param []))
                                                  (proof:relation stray) (fn [x i] [x i]))
                                  taken (fn [x i] [f i]))))

# (defn randomize [g]
#   (put g 0 1)                 # 0  one is stored into g
#   g)                          # 1  g is returned   2  the exit
(def randomize {:next [[0 1] [1 2]] :param [[:g 0]] :store [[:one :g 0]] :use [[:g 0]]
                :return [[:g 1]]})
# (defn keep-it [x]
#   (push log x)                # 0  x is stored into log
#   x)                          # 1  x is returned   2  the exit
(def keep-it {:next [[0 1] [1 2]] :param [[:x 0]] :free [:log] :store [[:x :log 0]]
              :use [[:x 0]] :return [[:x 1]]})
# (defn kept-r [x c]
#   (def r (if c x x))          # 0  the test   1  x is handed to r   2  x is handed to r
#   (push log r)                # 3  r is stored into log
#   r)                          # 4  r is returned   5  the exit
(def kept-r {:next   [[0 1] [0 2] [1 3] [2 3] [3 4] [4 5]]
             :param  [[:x 0]]
             :free   [:log]
             :pass   [[:x :r 1] [:x :r 2]]
             :store  [[:r :log 3]]
             :use    [[:r 3]]
             :return [[:r 4]]})
# (defn maybe [x c]
#   (if c x nil))               # 0  the test   1  x is handed to r   2  nil is handed to r
#                               # 3  r is returned   4  the exit
(def maybe {:next   [[0 1] [0 2] [1 3] [2 3] [3 4]]
            :param  [[:x 0]]
            :pass   [[:x :r 1] [:nil :r 2]]
            :return [[:r 3]]})

(assert (= [[:id 0]] (itself-of :id id)) "id returns x itself")
(assert (= [[:randomize 0]] (itself-of :randomize randomize)) "a store into g keeps g itself")
(assert (empty? (mapcat (fn [[f facts]] (itself-of f facts))
                        [[:either either] [:head head] [:fill fill] [:fresh-or fresh-or]]))
        "a result made, read, or one of two parameters is not itself")
(assert (empty? (itself-of :keep-it keep-it)) "a stored parameter is not handed over")
(assert (empty? (itself-of :kept-r kept-r)) "nor one stored under another name")
(assert (empty? (itself-of :maybe maybe)) "nor one returned on some paths alone")
```

In `fwd1` below, nothing uses `x` after the call, so the call to `id` hands
`x` to `r`, and `r` starts at `x`. In `fwd2`, `x` is used after the call, so
`r` is read out of `x` and starts a chain of its own:

```lisp
(def isigs {:itself (itself-of :id id)})

# (defn fwd1 [x]
#   (id x))                     # 0  x is handed to r   1  r is returned   2  the exit
(def fwd1 (calls {:next [[0 1] [1 2]] :param [[:x 0]] :call [[:r :id 0]] :arg [[:x 0 0]]
                  :use [[:x 0]] :return [[:r 1]]} isigs))
(assert (= [[:fwd1 0]] (itself-of :fwd1 fwd1)) "a hand-over keeps x itself")

# (defn fwd2 [x]
#   (def r (id x))              # 0  r is read out of x
#   (length x)                  # 1  x is used
#   r)                          # 2  r is returned   3  the exit
(def fwd2 (calls {:next (steps 3) :param [[:x 0]] :call [[:r :id 0]] :arg [[:x 0 0]]
                  :use [[:x 0] [:x 1]] :return [[:r 2]]} isigs))
(assert (empty? (itself-of :fwd2 fwd2)) "a read does not")
```
