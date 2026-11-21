# Group stores, in Datalog

<!-- audited: 2026-11-20 -->

How functions that call each other derive their stored facts in steps that only withdraw a fact, as Datalog that runs on proof.

[Stored](stored.md) derives the `stored` facts of one function, and
[recursion](recursion.md) the other facts of a group, in rounds. The fences
here include [place](place.md) and recursion. [Signatures](signature.md) holds
the Group stores rule in words.

| Derived | Holds when |
|---------|------------|
| `derived(f, i, t)` | the step derives `stored(f, i, t)` under the Stored parameter rule |
| `kept(f, i, t)` | the next step starts from `stored(f, i, t)` |

## The rules

| Rule | Datalog |
|------|---------|
| Group stores | `stored(g, j, :unnamed) :- member(g), param(x, j).` in the first step |
| Group stores | `store(y, [:unnamed g j p], p) :- call(r, g, p), arg(y, j, p), stored(g, j, :unnamed).` |
| Group stores | a step reads `itself(g, j)` only where `not stored(g, j, :unnamed)` |
| Group stores | `derived(f, i, :unnamed) :- probe(i), chain([:unnamed g j q], p), not open, not placed(f, i).` where `placed(f, i) :- derived(f, i, t), t ≠ :unnamed.` |
| Group stores | `kept(f, i, t) :- stored(f, i, t), derived(f, i, t).` |
| Group stores | `kept(f, i, t) :- stored(f, i, :unnamed), derived(f, i, t).` |

Each step runs the rounds with the named facts of the step before, then the
Stored parameter rule over each member, and keeps what that derives again. An
unnamed place stands for every place, so an unnamed fact agrees with each place
the step derives for it, and the kept fact names that place. The rounds read the
named facts alone, so an unnamed fact moves an argument only where the step
derives `stored`. A stored parameter is not returned itself
([itself](itself.md)), so the step drops the `itself` of each parameter with an
unnamed fact, as the rounds drop it for a named one.

A fact falls from unnamed to a place, and from a place to none, so the steps
stop. The rounds inside a step stop as [recursion](recursion.md) says, because
a member's named facts act as those of a known callee. Where the steps stop,
each fact is derived again in its own step, so the facts are sound by induction
on the depth of a call, as the rounds' are. The One place target reads the
facts the last step derives.

A fact still unnamed where the steps stop belongs to a member that never
returns. Each return of such a member follows a call that moves the parameter
to an unnamed place, so none of these calls returns first.

Below, `add-n` stores `x` into `acc` on its base case and hands it on in its
tail call. `ins` stores `v` into `t`, or calls itself on a child read out of
`t`. `spin` never returns:

```lisp
(include-file "place.md")
(include-file "recursion.md")

(defn derived-of [members sigs stored]
  "derived(f, i, t) as [f i t] for MEMBERS, each [f facts], under SIGS and the STORED facts."
  (def unnamed (proof:relation (map (fn [[g j t]] [[g j] true])
                                    (filter (fn [[_ _ t]] (= t :unnamed)) stored))))
  (mapcat (fn [[f facts]]
            (def after (calls (results facts sigs) sigs (lent sigs)))
            # store(y, [:unnamed g j p], p) :- call(r, g, p), arg(y, j, p), stored(g, j, :unnamed).
            (def moved (proof:join (call-args after) unnamed
                                   (fn [[g j] [y r p] _] [y [:unnamed g j p] p])))
            (def all (put after :store (concat (get after :store []) (proof:elements moved))))
            (mapcat (fn [[x i]]
                      (def probe (with-captures (instance all [i])))
                      (def ps (places probe x))
                      (def [_ chain] (chains probe x))
                      (defn at-unnamed? [[[w p] _]] (and (array? w) (= (get w 0) :unnamed)))
                      (map (fn [t] [f i t])
                           (cond (nil? ps) []
                                 (not (empty? ps)) ps
                                 (any? at-unnamed? (proof:elements chain)) [:unnamed]
                                 true [])))
                    (get facts :param [])))
          members))

(defn kept-of [stored derived]
  "kept(f, i, t) as [f i t], from the STORED facts of a step and the facts it DERIVED."
  (distinct (mapcat (fn [[f i t]]
                      (mapcat (fn [[g j u]]
                                (cond (not (and (= f g) (= i j))) []
                                      (= t :unnamed) [[f i u]]
                                      (= u t) [[f i t]]
                                      true []))
                              derived))
                    stored)))

(defn group-stores [members & known]
  "[stored derived]: the stored facts of MEMBERS, each [f facts], a group, and the facts the
   last step derives, where KNOWN holds the signatures a call out of the group reads."
  (def outside (get known 0 {}))
  (defn named [s] (filter (fn [[_ _ t]] (not= t :unnamed)) s))
  (defn with-known [sigs]
    (struct ;(mapcat (fn [k] [k (concat (get outside k []) (get sigs k []))])
                     [:fresh :returned :through :itself :borrows :stored])))
  (defn step [s]
    (def sigs (with-known (put (rounds members (with-known {:stored (named s)})) :stored (named s))))
    (def unnamed (map (fn [[g j _]] [g j]) (filter (fn [[_ _ t]] (= t :unnamed)) s)))
    # itself(g, j), not stored(g, j, :unnamed)
    (derived-of members (put sigs :itself (filter (fn [gj] (not (any? (fn [u] (= u gj)) unnamed)))
                                                  (sigs :itself)))
                s))
  # stored(g, j, :unnamed) :- member(g), param(x, j).
  (var s (mapcat (fn [[f facts]] (map (fn [[x i]] [f i :unnamed]) (get facts :param []))) members))
  (var before nil)
  (var d [])
  (while (not= s before)
    (assign before s)
    (assign d (step s))
    (assign s (kept-of s d)))
  [(named s) d])

# (defn add-n [acc x n]
#   (if (= n 0)                 # 0  the test
#     (begin (push acc x) nil)  # 1  x is stored into acc   2  z is returned
#     (add-n acc x (- n 1))))   # 3  m is the call's result   4  m is returned   5  the exit
(def add-n {:next [[0 1] [1 2] [0 3] [3 4] [2 5] [4 5]] :param [[:acc 0] [:x 1] [:n 2]]
            :store [[:x :acc 1]] :call [[:m :add-n 3]] :arg [[:acc 0 3] [:x 1 3] [:k 2 3]]
            :use [[:n 0] [:acc 1] [:x 1] [:acc 3] [:x 3]] :return [[:z 2] [:m 4]]})
# (defn ins [t v]
#   (if (leaf? t)               # 0  t is used
#     (begin (push t v) nil)    # 1  v is stored into t   2  z is returned
#     (ins (get t 0) v)))       # 3  c is read out of t   4  m is the call's result
#                               # 5  m is returned   6  the exit
(def ins {:next [[0 1] [1 2] [2 6] [0 3] [3 4] [4 5] [5 6]] :param [[:t 0] [:v 1]]
          :store [[:v :t 1]] :read [[:c :t 3]] :call [[:m :ins 4]] :arg [[:c 0 4] [:v 1 4]]
          :use [[:t 0] [:t 1] [:v 1] [:t 3] [:c 4] [:v 4]] :return [[:z 2] [:m 5]]})
# (defn spin [x] (spin x))      # 0  m is the call's result   1  m is returned   2  the exit
(def spin {:next [[0 1] [1 2]] :param [[:x 0]] :call [[:m :spin 0]] :arg [[:x 0 0]]
           :use [[:x 0]] :return [[:m 1]]})

(def [add-stored add-derived] (group-stores [[:add-n add-n]]))
(assert (= [[:add-n 1 [:param 0]]] add-stored) "add-n stores x")
(assert (empty? (one-place add-derived)) "into one place")
(assert (= [[:ins 1 [:param 0]]] (get (group-stores [[:ins ins]]) 0)) "ins stores v into t")
(def [spin-stored spin-derived] (group-stores [[:spin spin]]))
(assert (= [[:spin 0 :unnamed]] spin-derived) "spin keeps x unnamed")
(assert (empty? spin-stored) "so the fact is dropped")
```

`h2` stores `x` into `a` on its base case and otherwise calls `k2`, which
calls `h2` back. `k2` has no base case, so the first step's unnamed places keep
both facts. In `h`, the path that calls `k` returns a new array and never
stores `x`, so neither stores, and both results are fresh:

```lisp
# (defn h2 [a x n]
#   (if (= n 0)                 # 0  the test
#     (begin (push a x) nil)    # 1  x is stored into a   2  z is returned
#     (k2 a x n)))              # 3  m is the call's result   4  m is returned   5  the exit
(def h2 {:next [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5]] :param [[:a 0] [:x 1] [:n 2]]
         :store [[:x :a 1]] :call [[:m :k2 3]] :arg [[:a 0 3] [:x 1 3] [:n 2 3]]
         :use [[:n 0] [:a 1] [:x 1] [:a 3] [:x 3]] :return [[:z 2] [:m 4]]})
# (defn k2 [a x n] (h2 a x n))  # 0  m is the call's result   1  m is returned   2  the exit
(def k2 {:next [[0 1] [1 2]] :param [[:a 0] [:x 1] [:n 2]] :call [[:m :h2 0]]
         :arg [[:a 0 0] [:x 1 0] [:n 2 0]] :use [[:a 0] [:x 0]] :return [[:m 1]]})
(assert (= [[:h2 1 [:param 0]] [:k2 1 [:param 0]]] (get (group-stores [[:h2 h2] [:k2 k2]]) 0))
        "the two store x into a")

# (defn h [a x n]
#   (if (= n 0)                 # 0  the test
#     (begin (push a x) nil)    # 1  x is stored into a   2  z is returned
#     (begin (def v @[])        # 3  v is made
#            (k a v n) v)))     # 4  v is passed to k   5  v is returned   6  the exit
(def h {:next [[0 1] [1 2] [2 6] [0 3] [3 4] [4 5] [5 6]] :param [[:a 0] [:x 1] [:n 2]]
        :store [[:x :a 1]] :alloc [[:v 3]] :call [[:m :k 4]] :arg [[:a 0 4] [:v 1 4] [:n 2 4]]
        :use [[:n 0] [:a 1] [:x 1] [:a 4] [:v 4]] :return [[:z 2] [:v 5]]})
# (defn k [a x n] (h a x n))
(def k (merge k2 {:call [[:m :h 0]]}))
(def hk (get (group-stores [[:h h] [:k k]]) 0))
(assert (empty? hk) "x is stored on one path of h alone")
(assert (= [:h :k] ((rounds [[:h h] [:k k]] {:stored hk}) :fresh)) "so v is lent, and fresh")
```

`f2` stores `x` into `a` on its base case and `g2` into `b` on its own, and
each calls the other. Each step keeps the first place, and the last step derives
both, so the One place target refuses the two:

```lisp
# (defn f2 [a b x n]
#   (if (= n 0)                 # 0  the test
#     (begin (push a x) nil)    # 1  x is stored into a   2  z is returned
#     (g2 a b x n)))            # 3  m is the call's result   4  m is returned   5  the exit
(def f2 {:next [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5]] :param [[:a 0] [:b 1] [:x 2] [:n 3]]
         :store [[:x :a 1]] :call [[:m :g2 3]] :arg [[:a 0 3] [:b 1 3] [:x 2 3] [:n 3 3]]
         :use [[:n 0] [:a 1] [:x 1] [:a 3] [:b 3] [:x 3]] :return [[:z 2] [:m 4]]})
# (defn g2 [a b x n] (if (= n 0) (begin (push b x) nil) (f2 a b x n)))
(def g2 (merge f2 {:store [[:x :b 1]] :call [[:m :f2 3]]}))
(def [fg-stored fg-derived] (group-stores [[:f2 f2] [:g2 g2]]))
(assert (= [[:f2 2 [:param 0]] [:g2 2 [:param 1]]] fg-stored) "each keeps its first place")
(assert (= [[:f2 2] [:g2 2]] (one-place fg-derived)) "and the last step refuses both")
```

`k3` returns `x` after its call into `h3`, which stores `x`, so `k3` does not
return `x` itself. Rounds that read no stored facts give `k3` that `itself`
fact. The call in `h3` then hands `x` to its result as it stores `x`, the fiber
holds that result at the return, and the steps keep neither fact:

```lisp
# (defn h3 [a x n] (if (= n 0) (begin (push a x) nil) (k3 a x n)))
(def h3 (merge h2 {:call [[:m :k3 3]]}))
# (defn k3 [a x n]
#   (h3 a x n)                  # 0  m is the call's result
#   x)                          # 1  x is returned   2  the exit
(def k3 (merge k2 {:call [[:m :h3 0]] :return [[:x 1]]}))
(assert (= [[:h3 1 [:param 0]] [:k3 1 [:param 0]]] (get (group-stores [[:h3 h3] [:k3 k3]]) 0))
        "each stores x into a")
```
