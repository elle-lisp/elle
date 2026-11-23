# Stored parameters, in Datalog

<!-- audited: 2026-11-23 -->

How a function is found to store a parameter into a place, read from a probe of its body, as Datalog that runs on proof.

[Call](call.md) reads the `stored` fact, and [sink](sink.md) lends or sinks a
parameter that has none.
[Signatures](signature.md) holds the Stored parameter rule in words, and
[place](place.md) the callee's side of it.

| Fact | Holds when |
|------|------------|
| `probe(i)` | the pass runs the function with its parameter `i` owned by the fiber on entry |

| Derived | Holds when |
|---------|------------|
| `self(u, p)` | in the probe, at the return step `p`, `u` is the parameter or a block parameter that carries it |
| `chain(w, p)` | in the probe, at the return step `p`, `w` owns or carries a value in `self` or `chain` |
| `open` | in the probe, a chain of owners ends at the fiber, not at a returned value |

## The rules

| Rule | Datalog |
|------|---------|
| Stored parameter | `alloc(x, :entry) :- probe(i), param(x, i).` with `next(:entry, 0)`, in a probe pass for each parameter `i` |
| Stored parameter | `self(x, p) :- probe(i), param(x, i), return(v, p).` |
| Stored parameter | `self(s, p) :- self(u, p), carries(s, u, p).` |
| Stored parameter | `chain(o, p) :- self(u, p), owned(u, o, p).` and `chain(o, p) :- chain(w, p), owned(w, o, p).` |
| Stored parameter | `chain(s, p) :- chain(w, p), carries(s, w, p).` |
| Stored parameter | `open :- self(u, p), owned(u, :fiber, p).` |
| Stored parameter | `open :- chain(w, p), owned(w, :fiber, p), not return(w, p).` |
| Stored parameter | `stored(f, i, [:param j]) :- probe(i), chain(w, p), is(w, y), param(y, j), not open.` |
| Stored parameter | `stored(f, i, [:free n]) :- probe(i), chain(w, p), is(w, n), free(n), not captured(n), not open.` |
| Stored parameter | `stored(f, i, :result) :- probe(i), chain(v, p), return(v, p), owned(v, :fiber, p), not open.` |

The call moves a stored argument, so the callee's fiber owns the parameter on
entry, and each store moves it on, under the Store rule. The probe runs the
callee that way. It owns one parameter at a time, so each other parameter
carries no owner fact, and a chain stops there. The probe reads at each
return, so a container that holds the parameter and moves later carries it
along.

`wrap`, `keep` and `note` are the callees of `m` in [call](call.md), and the
derivation gives the facts `m` reads. In `via`, `x` moves into `a`, and `a`
into `acc`. `kept-r` from [itself](itself.md) hands `x` to `r` and stores `r`,
and `wrap-if` hands the array that holds `x` to the result.

Each chain must reach a place. Where a chain ends at the fiber, the callee
keeps the value on that path, and its code releases it there. The caller reads
its argument as held by the place, so it would use the value after that
release. `maybe-keep` stores `x` on one path, and `wrap-or` hands the array
that holds `x` to the result on one path. Neither has a `stored` fact, and the
probe of `maybe-keep` releases `x` on the edge into the other path.

`owned` merges paths at a join, so a chain may pass through an owner that
another path keeps. In `wrap-both`, each path stores `x` into an array made
before the branch and returns it, so the array the path keeps is open, and
`wrap-both` has no `stored` fact:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence :fence-of fence-of :fence-from fence-from} ((import "../spec.md")))
  (def {:ownership ownership :fiber-held fiber-held :steps steps} ((import "./datalog.md")))
  (def {:carried carried} ((import "./liveness.md")))
  (def {:with-captures with-captures :released-all released-all} ((import "./capture.md")))
  (def {:is-of is-of :free-names free-names :returning returning} ((import "./return.md")))
  (def {:instance instance} ((import "./sink.md")))
  (def itself ((import "./itself.md")))

  (defn chains [all x]
    "[self chain] as relations of [[u p] true], from ALL, the facts of the probe of X."
    (def owned (proof:relation (map (fn [[p [v o]]] [[v p] o]) (ownership all))))
    (def carrier (proof:relation (map (fn [[[s p] u]] [[u p] s]) (proof:elements (carried all)))))
    (def it (proof:iteration))
    (def self (proof:variable it "self"))
    (def chain (proof:variable it "chain"))
    # self(x, p) :- probe(i), param(x, i), return(v, p).
    (proof:extend self (map (fn [[v p]] [[x p] true]) (get all :return [])))
    (while (proof:changed? it)
      # self(s, p) :- self(u, p), carries(s, u, p).
      (proof:from-join self self carrier (fn [[u p] _ s] [[s p] true]))
      # chain(o, p) :- self(u, p), owned(u, o, p).  chain(o, p) :- chain(w, p), owned(w, o, p).
      (proof:from-join chain self owned (fn [[u p] _ o] [[o p] true]))
      (proof:from-join chain chain owned (fn [[w p] _ o] [[o p] true]))
      # chain(s, p) :- chain(w, p), carries(s, w, p).
      (proof:from-join chain chain carrier (fn [[w p] _ s] [[s p] true])))
    [(proof:complete self) (proof:complete chain)])

  (defn places [all x]
    "Each place the parameter X reaches in ALL, the facts of its probe; nil where a chain is open."
    (def [self chain] (chains all x))
    (def fiber (proof:map (fiber-held (ownership all)) (fn [vp] [vp true])))
    (def returns (proof:relation (get all :return [])))
    (def held (proof:join chain fiber (fn [k _ _] [k true])))   # chain(w, p), owned(w, :fiber, p)
    # open :- self(u, p), owned(u, :fiber, p).  open :- held, not return(w, p).
    (def open (concat (proof:elements (proof:join self fiber (fn [k _ _] k)))
                      (proof:elements (proof:antijoin held returns (fn [k _] k)))))
    (def named (proof:join (proof:map chain (fn [[[w p] _]] [w true])) (is-of all)
                           (fn [w _ z] [z true])))
    (def result (proof:join held (proof:map returns (fn [vp] [vp true])) (fn [_ _ _] :result)))
    (def [free captured] (free-names all))
    (if (empty? open)
      (concat (proof:elements (proof:join named (proof:relation (get all :param []))
                                          (fn [y _ j] [:param j])))
              (proof:elements (proof:join named free (fn [n _ _] [:free n])))
              # stored(f, i, :closure) :- probe(i), chain(w, p), is(w, n), captured(n), not open.
              (proof:elements (proof:join named captured (fn [_ _ _] :closure)))
              (proof:elements result))
      nil))

  (defn stored-of [f facts]
    "stored(f, i, t) as [f i t], from the FACTS of F after calls, under the Stored parameter rule."
    # alloc(x, :entry) :- probe(i), param(x, i).
    (mapcat (fn [[x i]] (map (fn [t] [f i t])
                             (or (places (with-captures (instance facts [i])) x) [])))
            (get facts :param [])))

  (def wrap (fence 'wrap :doc "wrap stores x into its fresh result."
    :source "(defn wrap [x] @[x])          # 0  v is made, with x as an operand   1  v is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :param [[:x 0]] :alloc [[:v 0]] :operand [[:x :v 0]]
            :use [[:x 0]] :return [[:v 1]]}
    :check (fn [wrap] (= [[:wrap 0 :result]] (stored-of :wrap wrap)))))

  (def keep (fence 'keep :doc "keep stores x into acc."
    :source "(defn keep [acc x] (push acc x))   # 0  x is stored into acc   1  acc is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :param [[:acc 0] [:x 1]] :store [[:x :acc 0]]
            :use [[:acc 0] [:x 0]] :return [[:acc 1]]}
    :check (fn [keep] (= [[:keep 1 [:param 0]]] (stored-of :keep keep)))))

  (def note (fence 'note :doc "note stores x into the definition journal."
    :source "(defn note [x] (push journal x))  # 0  x is stored into journal   1  journal is returned   2  the exit"
    :facts (merge ((fence-of itself 'keep-it) :facts) {:return [[:journal 1]]})
    :check (fn [note] (= [[:note 0 [:free :journal]]] (stored-of :note note)))))

  (def via (fence 'via :doc "A container moves x on."
    :source "(defn via [acc x]             # 0  a is made   1  x is stored into a   2  a is stored into acc
  (def a @[]) (push a x) (push acc a) nil)   # 3  n is returned   4  the exit"
    :facts {:next (steps 4) :param [[:acc 0] [:x 1]] :alloc [[:a 0]] :store [[:x :a 1] [:a :acc 2]]
            :use [[:a 1] [:x 1] [:acc 2] [:a 2]] :return [[:n 3]]}
    :check (fn [via] (= [[:via 1 [:param 0]]] (stored-of :via via)))))

  (def kept-r (fence-from itself 'kept-r :doc "A hand-over moves x on."
                          :check (fn [kept-r] (= [[:kept-r 0 [:free :journal]]] (stored-of :kept-r kept-r)))))

  (def wrap-if (fence 'wrap-if :doc "Each path's array moves x on."
    :source "(defn wrap-if [x c]           # 0  the test   1  a is made   2  a is handed to r
  (if c @[x] @[1 x]))         # 3  b is made   4  b is handed to r   5  r is returned   6  the exit"
    :facts {:next [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5] [5 6]] :param [[:x 0]] :alloc [[:a 1] [:b 3]]
            :operand [[:x :a 1] [:x :b 3]] :pass [[:a :r 2] [:b :r 4]] :return [[:r 5]]}
    :check (fn [wrap-if] (= [[:wrap-if 0 :result]] (stored-of :wrap-if wrap-if)))))

  (def maybe-keep (fence 'maybe-keep
    :doc "A parameter stored on one path alone is not stored, and the probe releases it on the other."
    :source "(defn maybe-keep [x c]
  (if c (push journal x) nil) # 0  the test   1  x is stored into journal   2  nil
  nil)                        # 3  n is returned   4  the exit"
    :facts {:next [[0 1] [0 2] [1 3] [2 3] [3 4]] :param [[:x 0]] :free [:journal]
            :store [[:x :journal 1]] :use [[:x 1] [:journal 1]] :return [[:n 3]]}
    :check (fn [mk] (and (empty? (stored-of :maybe-keep mk))
                         (= [[:x 0 2]] (released-all (returning (instance mk [0]))))))))

  (def wrap-or (fence 'wrap-or :doc "A parameter handed to the result on one path alone is not stored."
    :source "(defn wrap-or [x c]           # 0  a is made   1  x is stored into a   2  the test
  (def a @[]) (push a x)      # 3  a is handed to r   4  u is made   5  u is handed to r
  (if c a (array)))           # 6  r is returned   7  the exit"
    :facts {:next [[0 1] [1 2] [2 3] [3 6] [2 4] [4 5] [5 6] [6 7]] :param [[:x 0]]
            :alloc [[:a 0] [:u 4]] :store [[:x :a 1]] :pass [[:a :r 3] [:u :r 5]]
            :use [[:a 1] [:x 1]] :return [[:r 6]]}
    :check (fn [wrap-or] (empty? (stored-of :wrap-or wrap-or)))))

  (def wrap-both (fence 'wrap-both :doc "A chain through the other path's array is open."
    :source "(defn wrap-both [x c]         # 0  a and b are made   1  the test
  (def a @[]) (def b @[])     # 2  x is stored into a   3  a is handed to r
  (if c (begin (push a x) a)  # 4  x is stored into b   5  b is handed to r
    (begin (push b x) b)))    # 6  r is returned   7  the exit"
    :facts {:next [[0 1] [1 2] [2 3] [3 6] [1 4] [4 5] [5 6] [6 7]] :param [[:x 0]]
            :alloc [[:a 0] [:b 0]] :store [[:x :a 2] [:x :b 4]] :pass [[:a :r 3] [:b :r 5]]
            :use [[:a 2] [:x 2] [:b 4] [:x 4]] :return [[:r 6]]}
    :check (fn [wrap-both] (empty? (stored-of :wrap-both wrap-both)))))

  {:chains chains :places places :stored-of stored-of
   :fences [wrap keep note via kept-r wrap-if maybe-keep wrap-or wrap-both]})
```
