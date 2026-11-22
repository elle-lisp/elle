# Owner not passed, in Datalog

<!-- audited: 2026-11-22 -->

How a call passes the owner of what it lends at that owner's last use, so the callee releases it, as Datalog that runs on proof.

[Sink](sink.md) lets a call sink an argument. [Signatures](signature.md) holds
the Owner not passed rule in words, and [the region IR](ir.md) says what an
instance is.

| Fact | Holds when |
|------|------------|
| `owner(k)` | the instance takes an extra argument `[:owner k]` |
| `owner-of(j, k)` | the instance's parameter `j` borrows from its extra argument `[:owner k]` |

| Derived | Holds when |
|---------|------------|
| `holds(x, j, p)` | the argument `j` that the call at `p` lends borrows from `x` |
| `extra(x, p)` | the call at `p` passes `x` as the owner of what it lends |

## The rules

| Rule | Datalog |
|------|---------|
| Owner not passed | `holds(x, j, p) :- call(r, f, p), arg(z, j, p), lends(f, j), live(x, [:on j p]).` in a probe pass with `next(p, [:on j p]) :- arg(z, j, p).` and `use(z, [:on j p]) :- arg(z, j, p).` |
| Owner not passed | `extra(x, p) :- holds(x, j, p), owned(x, :fiber, p), next(p, q), not live(x, q), not passed(x, p).` where `passed(x, p) :- arg(x, i, p).` |
| Owner not passed | `pass(x, [:owner x p], p) :- extra(x, p).` |
| Owner not passed | `alloc([:owner k], :entry) :- owner(k).` |
| Owner not passed | `live([:owner k], q) :- owner-of(j, k), param(z, j), live-as(z, q).` |

An argument borrows from `x` where `x` would stay live if that argument alone
were used after the call. The probe step `[:on j p]` adds that use, so every
wait rule decides it: Container waits for a read, Immutable capture for a
closure, and Fresh result that borrows for a fresh result.

The pass ends the caller's hold, as a sink does, and nothing reads
`[:owner x p]`. The call names its instance with `owner(k)` for each value it
passes so, and `owner-of(j, k)` for each argument `j` that holds it. The
instance makes `[:owner k]` at `:entry`, as it makes a sunk parameter, and
`[:owner k]` waits for each such `j`. So the Unheld value rule releases it at
their last use, or the instance passes it on.

In `o1`, the call at step 3 is the last use of `xs`, and lends `h` and `t`,
both read out of it.

The Sink rule decides where a call passes no owner, in `o1b`, `o4` and `o5`.
`keep-first` has no `lends` fact, so in `o6`, the result of `keep-first` keeps
`xs` live.

In `o2`, the closure `k` captures the immutable `xs`, so `xs` waits for `k`. In
`o3`, the result of `keep-first` borrows from `xs`. `call-it` calls its
parameter, which is a target ([signatures](signature.md)); its facts here say
only that it uses `k`.

The rule covers every call, and a tail call forces it: a release in the caller
would come after the call, so the call would no longer be a tail call. In
`count-list` of [recursion](recursion.md), the instance with an owner passes it
on to itself, so the recursion keeps its tail calls:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence :fence-from fence-from} ((import "../spec.md")))
  (def {:ownership ownership :fiber-held fiber-held :steps steps} ((import "./datalog.md")))
  (def {:with-captures with-captures :released-all released-all :live-all live-all}
    ((import "./capture.md")))
  (def {:call-args call-args} ((import "./call.md")))
  (def {:returning returning} ((import "./return.md")))
  (def {:lent lent :after-calls after-calls} ((import "./borrows.md")))
  (def recursion ((import "./recursion.md")))
  (def sink ((import "./sink.md")))
  (def {:sinks sinks :instance instance :lends-of lends-of} sink)
  (def lends ((sink :lends)))
  (def ksigs ((sink :ksigs)))

  (defn holds-of [facts lends & waits]
    "holds(x, j, p) as [x j p], read from a probe pass with WAITS."
    (def args (get facts :arg []))
    # next(p, [:on j p]) :- arg(z, j, p).  use(z, [:on j p]) :- arg(z, j, p).
    (def probe (merge facts {:next (concat (get facts :next) (map (fn [[z j p]] [p [:on j p]]) args))
                             :use  (concat (get facts :use []) (map (fn [[z j p]] [z [:on j p]]) args))}))
    (def live-probe (live-all probe ;waits))
    # call(r, f, p), arg(z, j, p), lends(f, j)
    (def lent-at (proof:join (call-args facts) (proof:relation (map (fn [fi] [fi true]) lends))
                             (fn [[f j] [z r p] _] [[:on j p] [j p]])))
    # holds(x, j, p) :- call(r, f, p), arg(z, j, p), lends(f, j), live(x, [:on j p]).
    (proof:elements (proof:join (proof:map live-probe (fn [[x q]] [q x])) lent-at
                                (fn [_ x [j p]] [x j p]))))

  (defn extras [facts lends & waits]
    "extra(x, p) as [x p], read from a first pass and a probe pass with WAITS."
    (def owned (ownership facts))
    (def fiber (proof:map (fiber-held owned) (fn [vp] [vp true])))
    (def live-r (live-all facts ;waits))
    (def passed (proof:relation (map (fn [[x i p]] [x p]) (get facts :arg []))))
    # holds(x, j, p), owned(x, :fiber, p), not passed(x, p)
    (def mine (proof:antijoin (proof:join (proof:relation (map (fn [[x j p]] [[x p] j])
                                                               (holds-of facts lends ;waits)))
                                          fiber (fn [xp j _] [xp true]))
                              passed (fn [[x p] _] [p x])))
    # next(p, q), not live(x, q)
    (def at-next (proof:join mine (proof:relation (get facts :next)) (fn [p x q] [[x q] p])))
    (proof:elements (proof:antijoin at-next live-r (fn [[x q] p] [x p]))))

  (defn passed-on [facts lends & waits]
    "FACTS with the late captures as stores, and a pass for each sink and owner not passed."
    (def all (with-captures facts ;waits))
    (def sunk (sinks all lends ;waits))
    # pass(x, [:owner x p], p) :- extra(x, p).
    (put sunk :pass (concat (sunk :pass)
                            (map (fn [[x p]] [x [:owner x p] p]) (extras all lends ;waits)))))

  (defn owner-judged [facts lends & waits]
    "The releases of FACTS once each sink and each owner not passed is a pass."
    (released-all (passed-on facts lends ;waits) ;waits))

  (defn owning [facts owned-by]
    "FACTS of the instance with owner(k) for each [k js] in OWNED-BY."
    # alloc([:owner k], :entry) :- owner(k).
    (put facts :alloc (concat (get facts :alloc [])
                              (map (fn [[k js]] [[:owner k] :entry]) owned-by))))

  (defn waits-on [owned-by]
    "The clause for live under the Owner not passed rule, with owner-of(j, k) for each j in
     JS, per [k js] in OWNED-BY."
    (fn [facts owned it live live-by live-at]
      (def by-param (proof:join (proof:relation (map (fn [[z j]] [j z]) (get facts :param [])))
                                (proof:relation (mapcat (fn [[k js]] (map (fn [j] [j [:owner k]]) js))
                                                        owned-by))
                                (fn [j z o] [z o])))
      (fn []
        # live([:owner k], q) :- owner-of(j, k), param(z, j), live-as(z, q).
        (proof:from-join live live-by by-param (fn [z q o] [q o])))))

  (def o1 (fence 'o1 :doc "The call passes xs, and o1 releases nothing."
    :source "(defn o1 []
  (def xs (list @[1] @[2]))   # 0  xs is made
  (def h (first xs))          # 1  h is read out of xs
  (def t (rest xs))           # 2  t is read out of xs
  (two h t))                  # 3  h and t are lent, and xs is passed as their owner
                              # 4  the exit"
    :facts {:next  (steps 4)
            :alloc [[:xs 0]]
            :read  [[:h :xs 1] [:t :xs 2]]
            :call  [[:n :two 3]]
            :arg   [[:h 0 3] [:t 1 3]]
            :use   [[:xs 1] [:xs 2] [:h 3] [:t 3]]}
    :check (fn [o1] (and (= [[:xs 0 3] [:xs 1 3]] (filter (fn [[x]] (= x :xs)) (holds-of o1 lends)))
                         (= [[:xs [:owner :xs 3] 3]] ((passed-on o1 lends) :pass))
                         (empty? (owner-judged o1 lends))))))

  (def two (fence-from sink 'two
    :doc "The instance of two releases xs after the last use of u and v."
    :check (fn [two] (= [[[:owner 0] 0 1]]
                        (released-all (returning (owning (instance two []) [[0 [0 1]]]))
                                      (waits-on [[0 [0 1]]]))))))

  (defn passes-none [name doc facts]
    "A fence whose FACTS pass no owner."
    (fence name :doc doc :facts facts :check (fn [f] (empty? (extras f lends)))))
  (def o1b (passes-none 'o1b "A value used after the call is not passed."
             (put o1:facts :use [[:xs 1] [:xs 2] [:h 3] [:t 3] [:xs 4]])))   # (length xs) at 4
  # (defn o4 [xs] (two (first xs) (rest xs)))
  (def o4 (passes-none 'o4 "A parameter is not the fiber's to pass."
            (merge o1:facts {:alloc [] :param [[:xs 0]]})))
  # (defn o5 [] (def xs (list @[1] @[2])) (def h (first xs)) (two xs h))
  (def o5 (passes-none 'o5 "A value passed itself is not passed again as an owner."
            (merge o1:facts {:read [[:h :xs 1]] :arg [[:xs 0 3] [:h 1 3]]
                             :use [[:xs 1] [:xs 3] [:h 3]]})))

  (def o6 (fence 'o6 :doc "xs is not passed, so o6 releases xs and the result."
    :source "(defn o6 []
  (def xs (list @[1] @[2]))   # 0  xs is made
  (def h (first xs))          # 1  h is read out of xs
  (keep-first h)              # 2  h is lent, and the result is dropped
  nil)                        # 3  the exit"
    :facts {:next  (steps 3)
            :alloc [[:xs 0]]
            :read  [[:h :xs 1]]
            :call  [[:r :keep-first 2]]
            :arg   [[:h 0 2]]
            :use   [[:xs 1] [:h 2]]}
    :check (comp (fn [o6] (and (empty? (extras o6 lends (lent ksigs)))
                               (= [[:xs 2 3] [:r 2 3]] (owner-judged o6 lends (lent ksigs)))))
                 (after-calls ksigs))))

  # (defn call-it [k] (k) nil)    # 0  k is used   1  nil is returned   2  the exit
  (def call-it {:next [[0 1] [1 2]] :param [[:k 0]] :use [[:k 0]]})
  (def clends (concat lends (lends-of :call-it call-it {})))

  (def o2 (fence 'o2 :doc "The closure is sunk with xs as its owner, and o2 releases nothing."
    :source "(defn o2 []
  (def xs (list @[1] @[2]))   # 0  xs is made
  (def k (fn [] (length xs))) # 1  k is made, and captures xs
  (call-it k))                # 2  k is sunk, and xs is passed as its owner
                              # 3  the exit"
    :facts {:next      (steps 3)
            :alloc     [[:xs 0] [:k 1]]
            :capture   [[:xs :k 1]]
            :immutable [:xs]
            :call      [[:n :call-it 2]]
            :arg       [[:k 0 2]]
            :use       [[:k 2]]}
    :check (fn [o2] (and (= [[:k [:sink 0 2] 2] [:xs [:owner :xs 2] 2]] ((passed-on o2 clends) :pass))
                         (empty? (owner-judged o2 clends))))))

  (def o3 (fence 'o3 :doc "The result is sunk with xs as its owner, and o3 releases nothing."
    :source "(defn o3 []
  (def xs (list @[1] @[2]))   # 0  xs is made
  (def r (keep-first xs))     # 1  r is made, and borrows from xs
  (count-up r))               # 2  r is sunk, and xs is passed as its owner
                              # 3  the exit"
    :facts {:next  (steps 3)
            :alloc [[:xs 0]]
            :call  [[:r :keep-first 1] [:n :count-up 2]]
            :arg   [[:xs 0 1] [:r 0 2]]
            :use   [[:xs 1] [:r 2]]}
    :check (comp (fn [o3] (and (= [[:r [:sink 0 2] 2] [:xs [:owner :xs 2] 2]]
                                  ((passed-on o3 lends (lent ksigs)) :pass))
                               (empty? (owner-judged o3 lends (lent ksigs)))))
                 (after-calls ksigs))))

  (def count-list (fence-from recursion 'count-list
    :doc "Each instance passes lst on as the owner of t, and releases it where the list ends."
    :check (fn [count-list]
             (def llends (lends-of :count-list count-list {:returned [[:count-list 1]]}))
             (def cl-sunk (returning (instance count-list [0])))
             (def cl-owner (returning (owning (instance count-list []) [[0 [0]]])))
             (def on0 (waits-on [[0 [0]]]))
             (and (= [[:lst [:owner :lst 3] 3]] ((passed-on cl-sunk llends) :pass))
                  (= [[:lst 0 1]] (owner-judged cl-sunk llends))
                  (= [[[:owner 0] [:owner [:owner 0] 3] 3]] ((passed-on cl-owner llends on0) :pass))
                  (= [[[:owner 0] 0 1]] (owner-judged cl-owner llends on0))))))

  {:holds-of holds-of :extras extras :passed-on passed-on :owner-judged owner-judged
   :owning owning :waits-on waits-on :fences [o1 two o1b o4 o5 o6 o2 o3 count-list]})
```
