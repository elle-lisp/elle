# Free-name borrows, in Datalog

<!-- audited: 2026-11-22 -->

How a caller reads a result borrowed through a callee's free name: from a definition it names, or from the callee, as Datalog on proof.

[Return](return.md) gives a result its `through` facts, and
[borrows](borrows.md) its `borrows` facts. [Free](free.md) says why a caller
names its callee's definition and not its capture.
[Signatures](signature.md) holds the Free-name borrow rule in words.

| Fact | Holds when |
|------|------------|
| `through(f, :closure)` | the result of `f` is borrowed through one of its captures, so from `f` itself |
| `borrows(f, :closure)` | the result of `f` borrows through one of its captures, so from `f` itself |

## The rules

| Rule | Datalog |
|------|---------|
| Free-name borrow | `through(f, :closure) :- source(n), captured(n).` |
| Free-name borrow | `borrows(f, :closure) :- return(v, p), next(p, q), live(w, q), is(w, n), captured(n).` |
| Free-name borrow | `read(r, f, p) :- call(r, f, p), through(f, :closure).` |
| Free-name borrow | `live(f, q) :- call(r, f, p), borrows(f, :closure), live-as(r, q).` |
| Free-name borrow | `free(n) :- call(r, f, p), through(f, n), n ≠ :closure.` |
| Free-name borrow | `free(n) :- call(r, f, p), borrows(f, [:free n]).` |

The `through` and `borrows` clauses that name a free name hold where it is not
captured. A result read out of the closure keeps the closure live until the
result's last use, under the Container waits rule, and
[captures](capture.md) carries that to each value the closure captures.

`read-first` is the closure in [return](return.md), which captures `arr`. In
`kn`, the caller names the closure `k` and not `arr`, so `e` is read out of
`k`, and `k` waits for the use of `e`.

`pair-first` is the closure in [borrows](borrows.md), whose fresh result
borrows through its capture `arr`. In `kb`, `k` waits for the use of `s`.

Where `arr` is a definition instead, the callee's facts name it, and the
caller takes it as a free name of its own. So `peek2` returns a result
borrowed through `arr`, and `wrap2` one that borrows from `arr`:

```lisp
(fn []
  (def {:fence fence :fence-of fence-of :fence-from fence-from} ((import "../spec.md")))
  (def {:steps steps} ((import "./datalog.md")))
  (def {:judged judged} ((import "./capture.md")))
  (def {:calls calls} ((import "./call.md")))
  (def return ((import "./return.md")))
  (def {:signature signature :results results} return)
  (def borrows ((import "./borrows.md")))
  (def {:borrows-of borrows-of :lent lent :after-calls after-calls} borrows)
  (def {:lends-of lends-of} ((import "./sink.md")))

  (def read-first (fence-from return 'read-first
    :doc "A result borrowed through a capture is borrowed from the closure."
    :check (fn [rf] (= [[:read-first :closure]] ((signature :read-first rf) :through)))))

  (def kn-sigs (merge (signature :k read-first:facts) {:fresh [:make]}))
  (def kn (fence 'kn :doc "e is read out of the closure, so k waits for the use of e."
    :source "(defn make [] (def arr @[@[1]]) (fn [] (get arr 0)))
(defn kn []
  (def k (make))              # 0  k is made
  (def e (k))                 # 1  e is read out of k
  (length e))                 # 2  e is used   3  the exit"
    :facts {:next (steps 3) :call [[:k :make 0] [:e :k 1]] :use [[:k 1] [:e 2]]}
    :check (comp (fn [kn] (and (= [[:e :k 1]] (kn :read)) (= [[:k 2 3]] (judged kn))))
                 (fn [facts] (calls (results facts kn-sigs) kn-sigs)))))

  (def kb-sigs {:fresh [:make2 :k]
                :borrows (borrows-of :k ((fence-of borrows 'pair-first) :facts))})
  # (fn [i] (pair (get arr 0) ())): pair-first with a parameter
  (def pair-first (fence-from borrows 'pair-first
    :doc "The result borrows from the closure, which leaves each parameter lent."
    :check (fn [pf] (and (= [[:k :closure]] (borrows-of :k pf))
                         (= [[:k 0]] (lends-of :k (put pf :param [[:i 0]]) kb-sigs))))))

  (def kb (fence 'kb :doc "k waits for the use of s."
    :source "(defn make2 [] (def arr @[@[5]]) (fn [] (pair (get arr 0) ())))
(defn kb []
  (def k (make2))             # 0  k is made
  (def s (k))                 # 1  s is made, and borrows from k
  (length (first s)))         # 2  s is used   3  the exit"
    :facts {:next (steps 3) :call [[:k :make2 0] [:s :k 1]] :use [[:k 1] [:s 2]]}
    :check (comp (fn [kb] (= [[:k 2 3] [:s 2 3]] (judged kb (lent kb-sigs)))) (after-calls kb-sigs))))

  (def peek2 (fence 'peek2 :doc "arr is a free name of peek2."
    :source "(def arr @[@[1]])
(defn peek [] (get arr 0))
(defn peek2 []
  (peek))                     # 0  e is read out of arr   1  e is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :call [[:e :peek 0]] :return [[:e 1]]}
    :check (comp (fn [peek2] (= [[:peek2 :arr]] ((signature :peek2 peek2) :through)))
                 (fn [facts] (results facts (signature :peek (put read-first:facts :captured [])))))))

  (def pf-sigs {:fresh [:pf]
                :borrows (borrows-of :pf (put ((fence-of borrows 'pair-first) :facts) :captured []))})
  (def wrap2 (fence 'wrap2 :doc "The result of wrap2 borrows from arr."
    :source "(defn pf [] (pair (get arr 0) ()))
(defn wrap2 []
  (pf))                       # 0  s is made, and borrows from arr   1  s is returned   2  the exit"
    :facts {:next [[0 1] [1 2]] :call [[:s :pf 0]] :return [[:s 1]]}
    :check (comp (fn [wrap2] (= [[:wrap2 [:free :arr]]] (borrows-of :wrap2 wrap2 (lent pf-sigs))))
                 (after-calls pf-sigs))))

  {:fences [read-first kn pair-first kb peek2 wrap2]})
```
