# Return in Datalog

<!-- audited: 2026-11-22 -->

How a result takes its kind from what the returned value reaches, and how a call reads it, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts of a step, [call](call.md) the Call and
Returned itself rules, and [signatures](signature.md) the Return rule in words.

| Fact | Holds when |
|------|------------|
| `return(v, p)` | step `p` returns the value `v` |
| `param(x, i)` | the function's parameter `i` is named `x` |
| `free(n)` | `n` is a free name of the function |
| `captured(n)` | the free name `n` is a capture of the function, which [free](free.md) and [through](through.md) read |
| `fresh(f)` | the result of `f` is fresh |
| `through(f, n)` | the result of `f` is borrowed through the free name `n` |

| Derived | Holds when |
|---------|------------|
| `is(v, x)` | the value `v` is the parameter or free name `x` itself |
| `source(z)` | the returned value is `z` itself, or the chain of its owners reaches `z` |

## The rules

| Rule | Datalog |
|------|---------|
| Return | `live(v, q) :- return(v, p), next(p, q).` |
| Return | `is(x, x) :- param(x, i).` and `is(n, n) :- free(n).` |
| Return | `is(x, z) :- pass(y, x, p), is(y, z).` |
| Return | `source(z) :- return(v, p), is(v, z).` |
| Return | `source(z) :- return(v, p), dies(v, w, p), is(w, z).` |
| Return | `fresh(f) :- return(v, p), dies(v, :fiber, p).` |
| Return | `returned(f, i) :- source(x), param(x, i).` |
| Return | `through(f, n) :- source(n), free(n), not captured(n).` |
| Return | `alloc(r, p) :- call(r, f, p), fresh(f).` |
| Return | `read(r, n, p) :- call(r, f, p), through(f, n).` |

The facts of `f` are those of its body, and `q` is the exit, which has no next
step. A returned value's last use lies in the caller. So the value stays live
through the exit, and so does each value it waits on under the other rules.
No edge leaves the exit, so the callee releases none of them. The caller
releases a fresh result at its last use there, and that release frees the
chain the result reached in the callee.

`dies(v, :fiber, p)` holds where the chain of owners of `v` reaches the fiber.
So the value is one the call made, or one borrowed from a container the call
made. A parameter or a free name carries no owner fact, so the chain stops
there. A parameter returned itself is borrowed from its owner, under the Read
rule, so the result is an alias of it. A block parameter handed one is the
same value, so `is` follows each pass.

A result that reaches two parameters is an alias of each, and both arguments
wait for it. Where the `itself` fact holds, it replaces `returned`, and the
call hands the argument over ([call](call.md)). A function's signature is
derived before the facts of its callers, and functions that call each other
derive theirs in rounds ([recursion](recursion.md)).

`returning` adds the first clause as a use at the exit. `fill` returns the
array it made and stored into. Without the first clause, `fill` releases `a`
after the store, before the return. `inner` returns an element of an array it
made, so the result is fresh, and the callee releases neither.

`head` returns an element of its parameter, and `id` the parameter itself, so
each result is an alias of the parameter. `either` returns one of two
parameters through a block parameter, so its result is an alias of each.
`read-first` is the closure in [signatures](signature.md), which reads through
its capture `arr`, so its result is borrowed from itself
([through](through.md)).

In `caller`, `w` is fresh, so the caller releases it after its use. `h` is
read out of `coll`, and has no release. `e` is read out of `read-first`, which
waits for the use of `e`, so `arr` moves into it as a late capture. `r` is `a`
itself, so `a` waits for the use of `r`:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence} ((import "../spec.md")))
  (def {:units units :ownership ownership :steps steps} ((import "./datalog.md")))
  (def {:releases releases} ((import "./release.md")))
  (def {:judged judged} ((import "./capture.md")))
  (def {:calls calls :tag? tag?} ((import "./call.md")))

  (defn returning [facts]
    "FACTS with live(v, q) at the exit after each return, under the Return rule."
    # live(v, q) :- return(v, p), next(p, q).
    (def after (proof:join (proof:relation (map (fn [[v p]] [p v]) (get facts :return [])))
                           (proof:relation (get facts :next)) (fn [p v q] [v q])))
    (put facts :use (concat (get facts :use []) (proof:elements after))))

  (defn is-of [facts]
    "is(v, x) as a relation of [v x], under the Return rule."
    (def it (proof:iteration))
    (def is (proof:variable it "is"))
    # is(x, x) :- param(x, i).  is(n, n) :- free(n).
    (proof:extend is (concat (map (fn [[x i]] [x x]) (get facts :param []))
                             (map (fn [n] [n n]) (get facts :free []))))
    (def by-from (proof:relation (map (fn [[y x p]] [y x]) (get facts :pass []))))
    (while (proof:changed? it)
      # is(x, z) :- pass(y, x, p), is(y, z).
      (proof:from-join is is by-from (fn [y z x] [x z])))
    (proof:complete is))

  (defn free-names [facts]
    "[free captured]: the free names in FACTS that are not captures, and the captures, each a
     relation of [n true]."
    (defn names [k] (proof:relation (map (fn [n] [n true]) (get facts k []))))
    [(proof:antijoin (names :free) (proof:relation (get facts :captured [])) (fn [n _] [n true]))
     (names :captured)])

  (defn signature [f facts]
    "The fresh, returned and through facts of the function F, from its FACTS."
    (def ret (proof:relation (map (fn [vp] [vp true]) (get facts :return []))))
    (def chain (proof:join ret (proof:relation (units (ownership facts))) (fn [_ _ w] [w true])))
    # source(z) :- return(v, p), is(v, z).  source(z) :- return(v, p), dies(v, w, p), is(w, z).
    (def source (proof:join (proof:merge (proof:map ret (fn [[[v p] _]] [v true])) chain)
                            (is-of facts) (fn [_ _ z] [z true])))
    (defn sig [r g] (proof:elements (proof:join r source g)))
    (def [free captured] (free-names facts))
    # fresh(f) :- return(v, p), dies(v, :fiber, p).
    {:fresh    (proof:elements (proof:join chain (proof:relation [[:fiber true]]) (fn [_ _ _] f)))
     # returned(f, i) :- source(x), param(x, i).
     :returned (sig (proof:relation (get facts :param [])) (fn [x i _] [f i]))
     # through(f, n) :- source(n), free(n), not captured(n).
     :through  (concat (sig free (fn [n _ _] [f n]))
                       # through(f, :closure) :- source(n), captured(n).
                       (sig captured (fn [_ _ _] [f :closure])))})

  (defn results [facts sigs]
    "FACTS with the allocs and reads the Return rule derives from each call's callee."
    (def by-callee (proof:relation (map (fn [[r f p]] [f [r p]]) (get facts :call []))))
    # alloc(r, p) :- call(r, f, p), fresh(f).
    (def made (proof:join by-callee (proof:relation (map (fn [f] [f true]) (get sigs :fresh [])))
                          (fn [_ [r p] _] [r p])))
    # read(r, n, p) :- call(r, f, p), through(f, n).
    # read(r, f, p) :- call(r, f, p), through(f, :closure).
    (def reads (proof:join by-callee (proof:relation (get sigs :through []))
                           (fn [f [r p] n] [r (if (= n :closure) f n) p])))
    # free(n) :- call(r, f, p), through(f, n), n ≠ :closure.
    # free(n) :- call(r, f, p), borrows(f, [:free n]).
    (def named (concat (filter (fn [[f n]] (not= n :closure)) (get sigs :through []))
                       (map (fn [[f [_ n]]] [f n])
                            (filter (fn [[f z]] (tag? :free z)) (get sigs :borrows [])))))
    (def names (proof:join by-callee (proof:relation named) (fn [_ _ n] n)))
    (merge facts {:alloc (concat (get facts :alloc []) (proof:elements made))
                  :read  (concat (get facts :read []) (proof:elements reads))
                  :free  (distinct (concat (get facts :free []) (proof:elements names)))}))

  (def fill (fence 'fill :doc "fill is fresh, and releases nothing itself."
    :source "(defn fill []
  (def a @[])                 # 0  a is made
  (def b (array 1))           # 1  b is made
  (push a b)                  # 2  b is stored into a
  a)                          # 3  a is returned
                              # 4  the exit"
    :facts {:next   (steps 4)
            :alloc  [[:a 0] [:b 1]]
            :store  [[:b :a 2]]
            :use    [[:a 2] [:b 2]]
            :return [[:a 3]]}
    :check (fn [fill] (and (= {:fresh [:fill] :returned [] :through []} (signature :fill fill))
                           (empty? (releases (returning fill) (ownership fill)))))))

  (def inner (fence 'inner :doc "A borrow from a fresh container is fresh, and a outlives the call."
    :source "(defn inner []
  (def a @[@[1]])             # 0  a is made
  (def x (get a 0))           # 1  x is read out of a
  x)                          # 2  x is returned
                              # 3  the exit"
    :facts {:next   (steps 3)
            :alloc  [[:a 0]]
            :read   [[:x :a 1]]
            :use    [[:a 1]]
            :return [[:x 2]]}
    :check (fn [inner] (and (= [:inner] ((signature :inner inner) :fresh))
                            (empty? (releases (returning inner) (ownership inner)))))))

  (def head (fence 'head :doc "An element of c is an alias of c."
    :source "(defn head [c] (get c 0))     # 0  x is read out of c   1  x is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :param [[:c 0]] :read [[:x :c 0]] :use [[:c 0]] :return [[:x 1]]}
    :check (fn [head] (= [[:head 0]] ((signature :head head) :returned)))))

  (def id (fence 'id :doc "A parameter itself is an alias of it."
    :source "(defn id [x] x)               # 0  x is returned   1  the exit"
    :facts {:next [[0 1]] :param [[:x 0]] :return [[:x 0]]}
    :check (fn [id] (= [[:id 0]] ((signature :id id) :returned)))))

  (def either (fence 'either :doc "A result that may be either parameter is an alias of each."
    :source "(defn either [a b c]
  (if c a b))                 # 0  the test   1  a is handed to r   2  b is handed to r
                              # 3  r is returned   4  the exit"
    :facts {:next   [[0 1] [0 2] [1 3] [2 3] [3 4]]
            :param  [[:a 0] [:b 1]]
            :pass   [[:a :r 1] [:b :r 2]]
            :return [[:r 3]]}
    :check (fn [either] (= [[:either 0] [:either 1]] ((signature :either either) :returned)))))

  (def read-first (fence 'read-first :doc "A result read through the capture arr borrows from the closure."
    :source "(fn [] (get arr 0))           # 0  x is read out of arr   1  x is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :free [:arr] :captured [:arr] :read [[:x :arr 0]]
            :use [[:arr 0]] :return [[:x 1]]}
    :check (fn [read-first] (= {:fresh [] :returned [] :through [[:read-first :closure]]}
                               (signature :read-first read-first)))))

  (def callees (map (fn [[f facts]] (signature f facts))
                    [[:fill fill:facts] [:head head:facts] [:id id:facts]
                     [:read-first read-first:facts]]))
  (def sigs (struct ;(mapcat (fn [k] [k (mapcat (fn [s] (s k)) callees)])
                            [:fresh :returned :through])))

  (def caller (fence 'caller
    :doc "A fresh result is released by its caller, and the owner of an alias waits."
    :source "(defn caller [coll]
  (def arr @[@[1]])           # 0  arr is made
  (def read-first             # 1  read-first is made, and captures arr
    (fn [] (get arr 0)))
  (def w (fill))              # 2  w is made
  (def h (head coll))         # 3  h is read out of coll
  (def e (read-first))        # 4  e is read out of read-first
  (def a (array 1))           # 5  a is made
  (def r (id a))              # 6  r is read out of a
  (length h) (length r)       # 7  h and r are used
  (length e)                  # 8  e is used
  (length w))                 # 9  w is used
                              # 10 the exit"
    :facts {:next    (steps 10)
            :alloc   [[:arr 0] [:read-first 1] [:a 5]]
            :capture [[:arr :read-first 1]]
            :call    [[:w :fill 2] [:h :head 3] [:e :read-first 4] [:r :id 6]]
            :arg     [[:coll 0 3] [:a 0 6]]
            :use     [[:coll 3] [:read-first 4] [:a 6] [:h 7] [:r 7] [:e 8] [:w 9]]}
    :check (comp (fn [caller] (= [[:read-first 8 9] [:w 9 10] [:a 7 8]] (judged caller)))
                 (fn [facts] (calls (results facts sigs) sigs)))))

  {:returning returning :is-of is-of :free-names free-names :signature signature
   :results results :fences [fill inner head id either read-first caller]})
```

[States](states.md) refuses a result in two states.
