# Aliased arguments, in Datalog

<!-- audited: 2026-11-19 -->

How a call sinks an argument that another argument borrows from, and the instance waits for each borrower, as Datalog that runs on proof.

[Sink](sink.md) lets a call sink an argument, and
[Owner not passed](owner.md) holds the probe pass that the rules here read. The
fences here include the second document. [Signatures](signature.md) holds the
Aliased arguments rule in words, and [the region IR](ir.md) says what an
instance is.

| Fact | Holds when |
|------|------------|
| `borrower(j, i)` | the instance's parameter `j` borrows from its sunk parameter `i` |

| Derived | Holds when |
|---------|------------|
| `borrowed-by(y, i, j, p)` | the argument `j` of the call at `p` borrows from `y`, its argument `i` |
| `unlent(y, i, p)` | the call at `p` does not lend an argument that borrows from `y`, its argument `i` |
| `earlier(y, i, p)` | the call at `p` passes `y`, its argument `i`, in an earlier position too |

## The rules

| Rule | Datalog |
|------|---------|
| Aliased arguments | `borrowed-by(y, i, j, p) :- arg(y, i, p), arg(z, j, p), i ≠ j, live(y, [:on j p]).` in the probe pass of the Owner not passed rule |
| Aliased arguments | `unlent(y, i, p) :- call(r, f, p), borrowed-by(y, i, j, p), not lends(f, j).` |
| Aliased arguments | `earlier(y, i, p) :- arg(y, i, p), arg(y, h, p), h < i.` |
| Aliased arguments | `sinks(y, i, p) :- call(r, f, p), arg(y, i, p), lends(f, i), owned(y, :fiber, p), next(p, q), not live(y, q), not unlent(y, i, p), not earlier(y, i, p).` |
| Aliased arguments | `live(x, q) :- borrower(j, i), param(x, i), param(z, j), live(z, q).` |

The clause for `sinks` replaces the Sink clause. It keeps every condition but
`not aliased(y, i, p)`, which `not unlent(y, i, p)` and `not earlier(y, i, p)`
replace. Where no other argument borrows from `y`, neither holds, so the clause
sinks what the Sink clause sinks.

The argument `j` borrows from `y` where `y` would stay live if the argument `j`
alone were used after the call. The probe step `[:on j p]` adds that use, so
every wait rule decides it. An argument that is `y` itself borrows from it
under the Read rule, so a call that passes `y` twice sinks it in its first
position alone.

A parameter that the callee stores or returns, or that a result borrows from,
outlives the call. A sunk parameter that waits for it would have a release in
neither function, so the call lends `y` unless it lends each argument that
borrows from `y`.

The call is the last use of `y`, so no borrow of `y` is live in the caller
after it, because a live borrow keeps `y` live under the wait rules. The call
names its instance with `sunk(i)`, and with `borrower(j, i)` for each
`borrowed-by(y, i, j, p)`. In the instance, the sunk parameter waits for each
borrower, so the Unheld value rule releases it at the last use of them all.

In `a1` below, `h` is read out of `t`, and the call at step 2 is the last use
of `t`. So the call sinks `t`, and its instance of `pair-read` takes `b` as a
borrower of `a`. `a2` passes `t` twice, and `a4` passes two values that borrow
nothing from each other. `pick` returns `b`, so in `a3` the call lends `t`:

```lisp
(include-file "owner.md")

(defn borrowed-by [facts & waits]
  "borrowed-by(y, i, j, p) as [[y i p] j], read from the probe pass of the Owner not passed
   rule with WAITS."
  # holds-of with every argument lent gives each x with live(x, [:on j p])
  (def every (map (fn [[fj _]] fj) (proof:elements (call-args facts))))
  (def held (proof:relation (map (fn [[x j p]] [[x p] j]) (holds-of facts every ;waits))))
  (def at (proof:relation (map (fn [[y i p]] [[y p] i]) (get facts :arg []))))
  # borrowed-by(y, i, j, p) :- arg(y, i, p), arg(z, j, p), i ≠ j, live(y, [:on j p]).
  (filter (fn [[[y i p] j]] (not= i j))
          (proof:elements (proof:join held at (fn [[y p] j i] [[y i p] j])))))

(defn aliased-sinks [facts lends & waits]
  "FACTS with a pass to [:sink i p] for each argument y that a call sinks, and :borrower
   with each [p i j] where the call at p names its instance with borrower(j, i)."
  (def by (borrowed-by facts ;waits))
  (def callee (proof:relation (map (fn [[r f p]] [p f]) (get facts :call []))))
  # unlent(y, i, p) :- call(r, f, p), borrowed-by(y, i, j, p), not lends(f, j).
  (def unlent (proof:antijoin (proof:join (proof:relation (map (fn [[[y i p] j]] [p [y i j]]) by))
                                          callee (fn [p [y i j] f] [[f j] [y i p]]))
                              (proof:relation lends) (fn [_ yip] yip)))
  # earlier(y, i, p) :- arg(y, i, p), arg(y, h, p), h < i.
  (def at (proof:relation (map (fn [[y i p]] [[y p] i]) (get facts :arg []))))
  (def earlier (proof:relation (map (fn [[yip]] yip)
                                    (filter (fn [[_ i h]] (< h i))
                                            (proof:elements (proof:join at at
                                                                        (fn [[y p] i h] [[y i p] i h])))))))
  # sinks(y, i, p) :- the Sink conditions, not unlent(y, i, p), not earlier(y, i, p).
  (def sunk (proof:antijoin (proof:antijoin (proof:relation (map (fn [yip] [yip true])
                                                                 (last-uses facts lends ;waits)))
                                            unlent (fn [yip _] [yip true]))
                            earlier (fn [yip _] [yip true])))
  (merge facts
         {:pass     (concat (get facts :pass [])
                            (map (fn [[[y i p] _]] [y [:sink i p] p]) (proof:elements sunk)))
          :borrower (proof:elements (proof:join sunk (proof:relation by) (fn [[y i p] _ j] [p i j])))}))

# (defn pair-read [a b]
#   (def x (length a))          # 0  a is used
#   (+ x (length b)))           # 1  b is used   2  n is returned   3  the exit
(def pair-read {:next (steps 3) :param [[:a 0] [:b 1]] :use [[:a 0] [:b 1]] :return [[:n 2]]})
# (defn pick [a b] (length a) b)   # 0  a is used   1  b is returned   2  the exit
(def pick {:next (steps 2) :param [[:a 0] [:b 1]] :use [[:a 0]] :return [[:b 1]]})
(def psigs {:returned [[:pick 1]]})
(def alends (concat lends (lends-of :pair-read pair-read {}) (lends-of :pick pick psigs)))

# (defn a1 []
#   (def t (list @[1] 2))       # 0  t is made
#   (def h (first t))           # 1  h is read out of t
#   (pair-read t h))            # 2  t is sunk, and h is lent as its borrower
#                               # 3  the exit
(def a1-facts {:next  (steps 3)
               :alloc [[:t 0]]
               :read  [[:h :t 1]]
               :call  [[:n :pair-read 2]]
               :arg   [[:t 0 2] [:h 1 2]]
               :use   [[:t 1] [:t 2] [:h 2]]})
(def a1 (aliased-sinks a1-facts alends))
(assert (= [[:t [:sink 0 2] 2]] (a1 :pass)) "the call sinks t")
(assert (= [[2 0 1]] (a1 :borrower)) "and its instance takes b as a borrower of a")
(assert (empty? (releases a1 (ownership a1) removal operand captured)) "so a1 releases nothing")

# (defn a2 []
#   (def t (array 1))           # 0  t is made
#   (pair-read t t))            # 1  t is sunk as a, and lent as b   2  the exit
(def a2 (aliased-sinks {:next (steps 2) :alloc [[:t 0]] :call [[:n :pair-read 1]]
                        :arg [[:t 0 1] [:t 1 1]] :use [[:t 1]]} alends))
(assert (= [[:t [:sink 0 1] 1]] (a2 :pass)) "a value passed twice is sunk once")
(assert (= [[1 0 1]] (a2 :borrower)) "in its first position")

# (defn a4 []
#   (def u (array 1))           # 0  u is made
#   (def v (array 2))           # 1  v is made
#   (two u v))                  # 2  u and v are sunk   3  the exit
(def a4 (aliased-sinks {:next (steps 3) :alloc [[:u 0] [:v 1]] :call [[:n :two 2]]
                        :arg [[:u 0 2] [:v 1 2]] :use [[:u 2] [:v 2]]} alends))
(assert (= [[:u [:sink 0 2] 2] [:v [:sink 1 2] 2]] (a4 :pass)) "each argument is sunk")
(assert (empty? (a4 :borrower)) "and neither borrows from the other")

# (defn a3 []
#   (def t (list @[1] 2))       # 0  t is made
#   (def h (first t))           # 1  h is read out of t
#   (pick t h))                 # 2  t and h are lent, and r is read out of h
#                               # 3  the exit
(def a3 (aliased-sinks (calls (put a1-facts :call [[:r :pick 2]]) psigs) alends))
(assert (empty? (a3 :pass)) "pick returns its borrower, so the call lends t")
(assert (= [[:t 2 3]] (releases a3 (ownership a3) removal operand captured)) "and a3 releases t")
```

The instance of `pair-read` that `a1` names releases `a` after the last use of
`b`. The instance of `pick` would release `a` nowhere, which is why `a3` lends
`t`. In the instance of `fwd`, the call to `count-up` lends `b`, so it passes
`a` as the owner of `b` under the Owner not passed rule:

```lisp
(defn borrowers [pairs]
  "The clause for live under the Aliased arguments rule, with borrower(j, i) for each j in
   JS, per [i js] in PAIRS."
  (fn [facts owned it live live-by live-at]
    (def params (proof:relation (map (fn [[x i]] [i x]) (get facts :param []))))
    (def by-j (proof:join params (proof:relation (mapcat (fn [[i js]] (map (fn [j] [j i]) js))
                                                         pairs))
                          (fn [j z i] [i z])))
    (def waits (proof:join by-j params (fn [i z x] [z x])))
    (fn []
      # live(x, q) :- borrower(j, i), param(x, i), param(z, j), live(z, q).
      (proof:from-join live live-by waits (fn [z q x] [q x])))))

(def b01 (borrowers [[0 [1]]]))
(def pr (returning (instance pair-read [0])))
(assert (= [[:a 1 2]] (releases pr (ownership pr) removal operand captured b01))
        "the instance releases a after the last use of b")
(def pk (returning (instance pick [0])))
(assert (empty? (releases pk (ownership pk) removal operand captured b01))
        "a sunk parameter that waits for a returned borrower is never released")

# (defn fwd [a b]
#   (count-up b))               # 0  b is lent, and a is passed as its owner
#                               # 1  n is returned   2  the exit
(def fwd (returning (instance {:next   [[0 1] [1 2]]
                               :param  [[:a 0] [:b 1]]
                               :call   [[:n :count-up 0]]
                               :arg    [[:b 0 0]]
                               :use    [[:b 0]]
                               :return [[:n 1]]} [0])))
(assert (= [[:a [:owner :a 0] 0]] ((passed-on fwd alends b01) :pass)) "the instance passes a on")
(assert (empty? (owner-judged fwd alends b01)) "so it releases nothing itself")
```
