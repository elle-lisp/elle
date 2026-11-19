# Results that borrow, in Datalog

<!-- audited: 2026-11-19 -->

How a result names each parameter or free name it borrows from, and how the caller's owner waits for it, as Datalog that runs on proof.

[Return](return.md) gives a result its kind, and its fences are included here.
[Signatures](signature.md) holds the Fresh result that borrows rule in words.

| Fact | Holds when |
|------|------------|
| `borrows(f, z)` | the result of `f` borrows from the place `z`, which is `[:param i]` or `[:free n]` |

A place is written as in [call](call.md).

## The rules

| Rule | Datalog |
|------|---------|
| Fresh result that borrows | `borrows(f, [:param i]) :- return(v, p), next(p, q), live(w, q), is(w, x), param(x, i).` |
| Fresh result that borrows | `borrows(f, [:free n]) :- return(v, p), next(p, q), live(w, q), is(w, n), free(n).` |
| Fresh result that borrows | `live(y, q) :- call(r, f, p), arg(y, i, p), borrows(f, [:param i]), live(r, q).` |
| Fresh result that borrows | `live(n, q) :- call(r, f, p), borrows(f, [:free n]), live(r, q).` |

The Return rule keeps the result live at the exit. Each wait rule then keeps
live what the result waits on: Allocation operand, Container waits, Removal
and Immutable capture. So a parameter or a free name live at the exit is one
the result borrows from. At the caller, the argument or the free name waits
for the result's last use, and Container waits carries that up to its owner.

The rule speaks of a fresh result, and these clauses are generalized: they
hold for every result. An alias result also names its parameter, as `head`
does below. The read the Return rule gives already makes that argument wait,
so the extra wait changes no release, and the clauses need no negation.

The callee's facts run under every rule, with its late captures as stores.
The caller's wait is a rule that adds a clause for `live`, so the first pass
of [captures](capture.md) and of [calls](call.md) runs it too.

Below, `keep-first` is the function in [signatures](signature.md): the pair is
fresh, and its head borrows from `c`. `pair-first` reads through the free name
`arr`. `fill` borrows from nothing, and `head` is an alias:

```lisp
(include-file "return.md")

(defn borrows-of [f facts]
  "The borrows facts of the function F, as [f z], from its FACTS under every rule."
  (def all (with-captures (returning facts)))
  (def live-r (liveness all (ownership all) removal operand captured))
  (def exit (proof:join (proof:relation (map (fn [[v p]] [p true]) (get facts :return [])))
                        (proof:relation (get facts :next)) (fn [_ _ q] [q true])))
  # each z with live(w, q) at an exit q and is(w, z)
  (def at-exit (proof:join (proof:join (proof:map live-r (fn [[w q]] [q w])) exit
                                       (fn [_ w _] [w true]))
                           (is-of facts) (fn [_ _ z] [z true])))
  (defn place [r g] (proof:elements (proof:join at-exit r g)))
  (concat (place (proof:relation (get facts :param [])) (fn [x _ i] [f [:param i]]))
          (place (proof:relation (map (fn [n] [n true]) (get facts :free [])))
                 (fn [n _ _] [f [:free n]]))))

# (defn keep-first [c]
#   (pair (first c) ()))        # 0  x is read out of c   1  v is made, with x as an operand
#                               # 2  v is returned   3  the exit
(def keep-first {:next      [[0 1] [1 2] [2 3]]
                 :param     [[:c 0]]
                 :read      [[:x :c 0]]
                 :alloc     [[:v 1]]
                 :operand   [[:x :v 1]]
                 :immutable [:v]
                 :use       [[:c 0] [:x 1]]
                 :return    [[:v 2]]})
# (fn [] (pair (get arr 0) ()))  # the same steps, with x read out of arr
(def pair-first (merge keep-first {:param [] :free [:arr] :read [[:x :arr 0]]
                                   :use [[:arr 0] [:x 1]]}))

(assert (= [[:keep-first [:param 0]]] (borrows-of :keep-first keep-first)) "v borrows from c")
(assert (= [[:pair-first [:free :arr]]] (borrows-of :pair-first pair-first)) "and from arr")
(assert (empty? (borrows-of :fill fill)) "fill borrows from nothing")
(assert (= [[:head [:param 0]]] (borrows-of :head head)) "an alias names its parameter too")
```

In `kc` below, `r` borrows from `xs`, so `xs` waits for the use of `r` at
step 6. `s` borrows through the free name `arr`, which `kc` made and `pair-first`
captures, so `arr` waits for the use of `s` at step 7:

```lisp
(defn lent [sigs]
  "The clauses for live under the Fresh result that borrows rule, for releases, from SIGS."
  (fn [facts owned it live live-by live-at]
    (def by-call (proof:elements (proof:join (proof:relation (map (fn [[r f p]] [f [r p]])
                                                                  (get facts :call [])))
                                             (proof:relation (get sigs :borrows []))
                                             (fn [_ rp z] [rp z]))))
    # live(y, q) :- call(r, f, p), arg(y, i, p), borrows(f, [:param i]), live(r, q).
    (def from-arg (proof:join (proof:relation (map (fn [[[r p] z]] [[p z] r]) by-call))
                              (proof:relation (map (fn [[y i p]] [[p [:param i]] y])
                                                   (get facts :arg [])))
                              (fn [_ r y] [r y])))
    # live(n, q) :- call(r, f, p), borrows(f, [:free n]), live(r, q).
    (def from-free (map (fn [[[r p] [_ n]]] [r n])
                        (filter (fn [[_ [tag]]] (= tag :free)) by-call)))
    (def waits (proof:merge from-arg (proof:relation from-free)))
    (fn []
      (proof:from-join live live-by waits (fn [r q y] [q y])))))

(def sigs {:fresh   [:keep-first :pair-first]
           :borrows (concat (borrows-of :keep-first keep-first) (borrows-of :pair-first pair-first))})

# (defn kc []
#   (def xs (list @[1 2] @[3 4]))   # 0  xs is made
#   (def arr @[@[5]])               # 1  arr is made
#   (def pair-first (fn []          # 2  pair-first is made, and captures arr
#     (pair (get arr 0) ())))
#   (def r (keep-first xs))         # 3  r is made, and borrows from xs
#   (def s (pair-first))            # 4  s is made, and borrows from arr
#   (length xs)                     # 5  xs is used
#   (length (first r))              # 6  r is used
#   (length (first s)))             # 7  s is used
#                                   # 8  the exit
(def kc (calls (results {:next    (map (fn [p] [p (+ p 1)]) (range 8))
                         :alloc   [[:xs 0] [:arr 1] [:pair-first 2]]
                         :capture [[:arr :pair-first 2]]
                         :call    [[:r :keep-first 3] [:s :pair-first 4]]
                         :arg     [[:xs 0 3]]
                         :use     [[:xs 3] [:pair-first 4] [:xs 5] [:r 6] [:s 7]]} sigs)
               sigs (lent sigs)))
(assert (= [[:pair-first 4 5] [:arr 7 8] [:xs 6 7] [:s 7 8] [:r 6 7]] (judged kc (lent sigs)))
        "each owner a result borrows from waits for that result")
```

In `kh` below, nothing uses `a` after `randomize` returns it, but `r2` still
borrows from it. The first pass of [calls](call.md) runs the wait, so `a`
keeps its hold, `r` is read out of it, and `a` waits for `r2`:

```lisp
(def kh-sigs (merge sigs {:itself [[:randomize 0]]}))

# (defn kh []
#   (def a (array 1 2))         # 0  a is made
#   (def r2 (keep-first a))     # 1  r2 is made, and borrows from a
#   (def r (randomize a))       # 2  r is read out of a
#   (length r)                  # 3  r is used
#   (length (first r2)))        # 4  r2 is used
#                               # 5  the exit
(def kh (calls (results {:next  (map (fn [p] [p (+ p 1)]) (range 5))
                         :alloc [[:a 0]]
                         :call  [[:r2 :keep-first 1] [:r :randomize 2]]
                         :arg   [[:a 0 1] [:a 0 2]]
                         :use   [[:a 1] [:a 2] [:r 3] [:r2 4]]} kh-sigs)
               kh-sigs (lent kh-sigs)))
(assert (= [[:r :a 2]] (kh :read)) "a borrowed argument is not handed over")
(assert (= [[:r2 4 5] [:a 4 5]] (judged kh (lent kh-sigs))) "and a waits for r2")
```
