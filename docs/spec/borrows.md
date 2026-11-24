# Results that borrow, in Datalog

<!-- audited: 2026-11-22 -->

How a result names each parameter or free name it borrows from, and how the caller's owner waits for it, as Datalog that runs on proof.

[Return](return.md) gives a result its kind, and [itself](itself.md) finds a
result that is a parameter itself.
[Signatures](signature.md) holds the Fresh result that borrows rule in words.

| Fact | Holds when |
|------|------------|
| `borrows(f, z)` | the result of `f` borrows from the place `z`, which is `[:param i]` or `[:free n]` |

A place is written as in [call](call.md).

## The rules

| Rule | Datalog |
|------|---------|
| Fresh result that borrows | `borrows(f, [:param i]) :- return(v, p), next(p, q), live(w, q), is(w, x), param(x, i), not itself(f, i).` |
| Fresh result that borrows | `borrows(f, [:free n]) :- return(v, p), next(p, q), live(w, q), is(w, n), free(n), not captured(n).` |
| Fresh result that borrows | `live(y, q) :- call(r, f, p), arg(y, i, p), borrows(f, [:param i]), live-as(r, q).` |
| Fresh result that borrows | `live(n, q) :- call(r, f, p), borrows(f, [:free n]), live-as(r, q).` |

The Return rule keeps the result live at the exit. Each wait rule then keeps
live what the result waits on: Allocation operand, Container waits, Removal
and Immutable capture. So a parameter or a free name live at the exit is one
the result borrows from. At the caller, the argument or the free name waits
for the result's last use, and Container waits carries that up to its owner.
Where the free name is a capture, the closure waits instead
([through](through.md)).

The rule speaks of a fresh result, and these clauses are generalized: they
hold for every result. An alias result also names its parameter, as `head`
does below. The read the Return rule gives already makes that argument wait,
so the extra wait changes no release. A result that is a parameter itself
borrows from nothing. A call at that argument's last use hands it to the
result, under the Returned itself rule, and a wait on it would keep it live
after the call, so no call could hand it over.

The callee's facts run under every rule, with its late captures as stores.
The caller's wait is a rule that adds a clause for `live`, so the first pass
of [captures](capture.md) and of [calls](call.md) runs it too.

`keep-first` is the function in [signatures](signature.md): the pair is fresh,
and its head borrows from `c`. `pair-first` reads through its capture `arr`.
`fill` of [return](return.md) borrows from nothing, and `head` is an alias.
`after-calls` gives the facts a caller's rules read, under the signatures it
is given and the wait `lent` reads from them.

In `kc`, `r` borrows from `xs`, so `xs` waits for the use of `r` at step 6. `s`
borrows from the closure `pair-first`, which waits for the use of `s` at step
7, so `arr` moves into it as a late capture ([through](through.md)).

In `kh`, nothing uses `a` after `randomize` returns it, but `r2` still borrows
from it. The first pass of [calls](call.md) runs the wait, so `a` keeps its
hold, `r` is read out of it, and `a` waits for `r2`.

In `fw`, `id` returns `x` itself, so it borrows from nothing, and the call at
the last use of `x` hands `x` to `r`. A borrows fact for `id` would make `x`
wait for `r`, and the call would read `r` out of `x` instead:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence :fence-of fence-of :fence-from fence-from :calling calling}
    ((import "../spec.md")))
  (def {:steps steps} ((import "./datalog.md")))
  (def {:with-captures with-captures :live-all live-all :judged judged}
    ((import "./capture.md")))
  (def {:calls calls :tag? tag?} ((import "./call.md")))
  (def return ((import "./return.md")))
  (def {:returning returning :is-of is-of :free-names free-names :results results} return)
  (def itself ((import "./itself.md")))
  (def {:itself-of itself-of} itself)

  (defn borrows-of [f facts & waits]
    "The borrows facts of the function F, as [f z], from its FACTS under every rule and WAITS."
    (def mine (map (fn [[g i]] i) (itself-of f facts)))
    (def all (with-captures (returning facts) ;waits))
    (def live-r (live-all all ;waits))
    (def exit (proof:join (proof:relation (map (fn [[v p]] [p true]) (get facts :return [])))
                          (proof:relation (get facts :next)) (fn [_ _ q] [q true])))
    # each z with live(w, q) at an exit q and is(w, z)
    (def at-exit (proof:join (proof:join (proof:map live-r (fn [[w q]] [q w])) exit
                                         (fn [_ w _] [w true]))
                             (is-of facts) (fn [_ _ z] [z true])))
    (defn place [r g] (proof:elements (proof:join at-exit r g)))
    # not itself(f, i)
    (def lent-params (filter (fn [[x i]] (not (any? (fn [j] (= j i)) mine))) (get facts :param [])))
    (def [free captured] (free-names facts))
    (concat (place (proof:relation lent-params) (fn [x _ i] [f [:param i]]))
            (place free (fn [n _ _] [f [:free n]]))
            # borrows(f, :closure) :- return(v, p), next(p, q), live(w, q), is(w, n), captured(n).
            (place captured (fn [_ _ _] [f :closure]))))

  (defn lent [sigs]
    "The clauses for live under the Fresh result that borrows rule, for releases, from SIGS."
    (fn [facts owned it live live-by live-at]
      (def by-call (proof:elements (proof:join (proof:relation (map (fn [[r f p]] [f [r p]])
                                                                    (get facts :call [])))
                                               (proof:relation (get sigs :borrows []))
                                               # live(f, q) :- call(r, f, p), borrows(f, :closure),
                                               #               live-as(r, q).
                                               (fn [f rp z] [rp (if (= z :closure) [:free f] z)]))))
      # live(y, q) :- call(r, f, p), arg(y, i, p), borrows(f, [:param i]), live-as(r, q).
      (def from-arg (proof:join (proof:relation (map (fn [[[r p] z]] [[p z] r]) by-call))
                                (proof:relation (map (fn [[y i p]] [[p [:param i]] y])
                                                     (get facts :arg [])))
                                (fn [_ r y] [r y])))
      # live(n, q) :- call(r, f, p), borrows(f, [:free n]), live-as(r, q).
      (def from-free (map (fn [[[r p] [_ n]]] [r n])
                          (filter (fn [[_ z]] (tag? :free z)) by-call)))
      (def waits (proof:merge from-arg (proof:relation from-free)))
      (fn []
        (proof:from-join live live-by waits (fn [r q y] [q y])))))

  (defn after-calls [sigs]
    "A function from facts to the facts after results and calls under SIGS, with the lent wait."
    (fn [facts] (calls (results facts sigs) sigs (lent sigs))))

  (def keep-first (fence 'keep-first :doc "v borrows from c."
    :source "(defn keep-first [c]
  (pair (first c) ()))        # 0  x is read out of c   1  v is made, with x as an operand
                              # 2  v is returned   3  the exit"
    :facts {:next      [[0 1] [1 2] [2 3]]
            :param     [[:c 0]]
            :read      [[:x :c 0]]
            :alloc     [[:v 1]]
            :operand   [[:x :v 1]]
            :immutable [:v]
            :use       [[:c 0] [:x 1]]
            :return    [[:v 2]]}
    :check (fn [keep-first] (= [[:keep-first [:param 0]]] (borrows-of :keep-first keep-first)))))

  # (fn [] (pair (get arr 0) ())): the steps of keep-first, with x read out of arr
  (def pair-first (fence 'pair-first :doc "The result borrows from the closure that captures arr."
    :facts (merge keep-first:facts {:param [] :free [:arr] :captured [:arr] :read [[:x :arr 0]]
                                    :use [[:arr 0] [:x 1]]})
    :check (fn [pair-first] (= [[:pair-first :closure]] (borrows-of :pair-first pair-first)))))

  (def fill (fence-from return 'fill :doc "fill borrows from nothing."
                        :check (fn [fill] (empty? (borrows-of :fill fill)))))
  (def head (fence-from return 'head :doc "An alias names its parameter too."
                        :check (fn [head] (= [[:head [:param 0]]] (borrows-of :head head)))))

  (def sigs {:fresh   [:keep-first :pair-first]
             :borrows (concat (borrows-of :keep-first keep-first:facts)
                              (borrows-of :pair-first pair-first:facts))})

  (def kc (fence 'kc :doc "Each owner a result borrows from waits for that result."
    :source (calling [keep-first] "(defn kc []
  (def xs (list @[1 2] @[3 4]))   # 0  xs is made
  (def arr @[@[5]])               # 1  arr is made
  (def pair-first (fn []          # 2  pair-first is made, and captures arr
    (pair (get arr 0) ())))
  (def r (keep-first xs))         # 3  r is made, and borrows from xs
  (def s (pair-first))            # 4  s is made, and borrows from pair-first
  (length xs)                     # 5  xs is used
  (length (first r))              # 6  r is used
  (length (first s)))             # 7  s is used
                                  # 8  the exit")
    :facts {:next    (steps 8)
            :alloc   [[:xs 0] [:arr 1] [:pair-first 2]]
            :capture [[:arr :pair-first 2]]
            :call    [[:r :keep-first 3] [:s :pair-first 4]]
            :arg     [[:xs 0 3]]
            :use     [[:xs 3] [:pair-first 4] [:xs 5] [:r 6] [:s 7]]}
    :check (comp (fn [kc] (= [[:pair-first 7 8] [:xs 6 7] [:s 7 8] [:r 6 7]] (judged kc (lent sigs))))
                 (after-calls sigs))))

  (def kh-sigs (merge sigs {:itself [[:randomize 0]]}))

  (def kh (fence 'kh :doc "A borrowed argument is not handed over, and a waits for r2."
    :source (calling [keep-first (fence-of itself 'randomize)] "(defn kh []
  (def a (array 1 2))         # 0  a is made
  (def r2 (keep-first a))     # 1  r2 is made, and borrows from a
  (def r (randomize a))       # 2  r is read out of a
  (length r)                  # 3  r is used
  (length (first r2)))        # 4  r2 is used
                              # 5  the exit")
    :facts {:next  (steps 5)
            :alloc [[:a 0]]
            :call  [[:r2 :keep-first 1] [:r :randomize 2]]
            :arg   [[:a 0 1] [:a 0 2]]
            :use   [[:a 1] [:a 2] [:r 3] [:r2 4]]}
    :check (comp (fn [kh] (and (= [[:r :a 2]] (kh :read))
                               (= [[:r2 4 5] [:a 4 5]] (judged kh (lent kh-sigs)))))
                 (after-calls kh-sigs))))

  (def id (fence-from return 'id :doc "A result that is x itself borrows from nothing."
                      :check (fn [id] (empty? (borrows-of :id id)))))
  (def id-sigs {:itself (itself-of :id id:facts) :borrows (borrows-of :id id:facts)})

  (def fw (fence 'fw :doc "The call hands x over to a result that borrows from nothing."
    :source (calling [(fence-of return 'id)] "(defn fw [x]
  (def r (id x))              # 0  x is handed to r
  (length r))                 # 1  r is used   2  the exit")
    :facts {:next (steps 2) :param [[:x 0]] :call [[:r :id 0]] :arg [[:x 0 0]]
            :use [[:x 0] [:r 1]]}
    :check (comp (fn [fw] (= [[:x :r 0]] (fw :pass))) (fn [f] (calls f id-sigs (lent id-sigs))))))

  {:borrows-of borrows-of :lent lent :after-calls after-calls
   :fences [keep-first pair-first fill head kc kh id fw]})
```
