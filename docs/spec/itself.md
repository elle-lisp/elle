# Returned itself, in Datalog

<!-- audited: 2026-11-23 -->

How a function's result is found to be one parameter itself on every path, as Datalog that runs on proof.

[Return](return.md) gives a result its kind, and [states](states.md) refuses
one in two states.
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

`id` of [return](return.md) returns `x` itself, and `randomize` stores into `g`
but does not store `g`. `keep-it` stores `x`, and `kept-r` stores it under the
name `r`. `maybe` returns `nil` on one path, and `either`, `head` and `fill` of
return and `fresh-or` of [states](states.md) return a value they make or read,
or one of two parameters.

In `fwd1`, nothing uses `x` after the call, so the call to `id` hands `x` to
`r`, and `r` starts at `x`. In `fwd2`, `x` is used after the call, so `r` is
read out of `x` and starts a chain of its own:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence :fence-of fence-of :fence-from fence-from :calling calling}
    ((import "../spec.md")))
  (def {:operands operands :steps steps :refusals refusals :ownership ownership}
    ((import "./datalog.md")))
  (def {:with-captures with-captures} ((import "./capture.md")))
  (def {:calls calls} ((import "./call.md")))
  (def return ((import "./return.md")))
  (def states ((import "./states.md")))

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

  (defn not-itself [module name]
    "The fence NAME of MODULE, built here: its result is no parameter itself."
    (fence-from module name :doc (string "The result of " name " is no parameter itself.")
                :check (fn [facts] (empty? (itself-of name facts)))))

  (def id (fence-from return 'id :doc "id returns x itself."
                      :check (fn [id] (= [[:id 0]] (itself-of :id id)))))

  (def randomize (fence 'randomize :doc "A store into g is accepted, and keeps g itself."
    :source "(defn randomize [g]
  (put g 0 1)                 # 0  one is stored into g
  g)                          # 1  g is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :param [[:g 0]] :store [[:one :g 0]] :use [[:g 0]]
            :return [[:g 1]]}
    :check (fn [randomize] (and (empty? (refusals randomize (ownership randomize)))
                                (= [[:randomize 0]] (itself-of :randomize randomize))))))

  (def keep-it (fence 'keep-it :doc "A stored parameter is not handed over."
    :source "(defn keep-it [journal x]
  (push journal x)            # 0  x is stored into journal
  x)                          # 1  x is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :param [[:journal 0] [:x 1]] :store [[:x :journal 0]]
            :use [[:journal 0] [:x 0]] :return [[:x 1]]}
    :check (fn [keep-it] (empty? (itself-of :keep-it keep-it)))))

  (def kept-r (fence 'kept-r :doc "A parameter stored under another name is not handed over."
    :source "(defn kept-r [journal x c]
  (def r (if c x x))          # 0  the test   1  x is handed to r   2  x is handed to r
  (push journal r)            # 3  r is stored into journal
  r)                          # 4  r is returned   5  the exit"
    :facts {:next   [[0 1] [0 2] [1 3] [2 3] [3 4] [4 5]]
            :param  [[:journal 0] [:x 1]]
            :pass   [[:x :r 1] [:x :r 2]]
            :store  [[:r :journal 3]]
            :use    [[:journal 3] [:r 3]]
            :return [[:r 4]]}
    :check (fn [kept-r] (empty? (itself-of :kept-r kept-r)))))

  (def maybe (fence 'maybe :doc "A parameter returned on some paths alone is not itself."
    :source "(defn maybe [x c]
  (if c x nil))               # 0  the test   1  x is handed to r   2  nil is handed to r
                              # 3  r is returned   4  the exit"
    :facts {:next   [[0 1] [0 2] [1 3] [2 3] [3 4]]
            :param  [[:x 0]]
            :pass   [[:x :r 1] [:nil :r 2]]
            :return [[:r 3]]}
    :check (fn [maybe] (empty? (itself-of :maybe maybe)))))

  (def isigs {:itself (itself-of :id id:facts)})

  (def fwd1 (fence 'fwd1 :doc "A hand-over keeps x itself."
    :source (calling [(fence-of return 'id)] "(defn fwd1 [x]
  (id x))                     # 0  x is handed to r   1  r is returned   2  the exit")
    :facts {:next [[0 1] [1 2]] :param [[:x 0]] :call [[:r :id 0]] :arg [[:x 0 0]]
            :use [[:x 0]] :return [[:r 1]]}
    :check (comp (fn [fwd1] (= [[:fwd1 0]] (itself-of :fwd1 fwd1))) (fn [f] (calls f isigs)))))

  (def fwd2 (fence 'fwd2 :doc "A read does not keep x itself."
    :source (calling [(fence-of return 'id)] "(defn fwd2 [x]
  (def r (id x))              # 0  r is read out of x
  (length x)                  # 1  x is used
  r)                          # 2  r is returned   3  the exit")
    :facts {:next (steps 3) :param [[:x 0]] :call [[:r :id 0]] :arg [[:x 0 0]]
            :use [[:x 0] [:x 1]] :return [[:r 2]]}
    :check (comp (fn [fwd2] (empty? (itself-of :fwd2 fwd2))) (fn [f] (calls f isigs)))))

  {:origins origins :itself-of itself-of
   :fences [id randomize keep-it kept-r maybe ;(map (fn [n] (not-itself return n)) ['either 'head 'fill])
            (not-itself states 'fresh-or) fwd1 fwd2]})
```
