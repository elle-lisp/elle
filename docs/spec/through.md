# Free-name borrows, in Datalog

<!-- audited: 2026-11-21 -->

How a caller reads a result borrowed through a callee's free name: from a definition it names, or from the callee, as Datalog on proof.

[Return](return.md) gives a result its `through` facts, and
[borrows](borrows.md) its `borrows` facts. [Free](free.md) says why a caller
names its callee's definition and not its capture, and its fences are included
here. [Signatures](signature.md) holds the Free-name borrow rule in words.

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

Below, `read-first` is the closure in [return](return.md), which captures
`arr`. In `kn`, the caller names the closure `k` and not `arr`, so `e` is read
out of `k`, and `k` waits for the use of `e`:

```lisp
(include-file "free.md")

(assert (= [[:read-first :closure]] ((signature :read-first read-first) :through))
        "a result borrowed through a capture is borrowed from the closure")

# (defn make [] (def arr @[@[1]]) (fn [] (get arr 0)))
# (defn kn []
#   (def k (make))              # 0  k is made
#   (def e (k))                 # 1  e is read out of k
#   (length e))                 # 2  e is used   3  the exit
(def kn-sigs (merge (signature :k read-first) {:fresh [:make]}))
(def kn (calls (results {:next (steps 3) :call [[:k :make 0] [:e :k 1]] :use [[:k 1] [:e 2]]}
                        kn-sigs)
               kn-sigs))
(assert (= [[:e :k 1]] (kn :read)) "e is read out of the closure")
(assert (= [[:k 2 3]] (judged kn)) "so k waits for the use of e")
```

`pair-first` is the closure in [borrows](borrows.md), whose fresh result
borrows through its capture `arr`. In `kb`, `k` waits for the use of `s`:

```lisp
# (defn make2 [] (def arr @[@[5]]) (fn [] (pair (get arr 0) ())))
# (defn kb []
#   (def k (make2))             # 0  k is made
#   (def s (k))                 # 1  s is made, and borrows from k
#   (length (first s)))         # 2  s is used   3  the exit
(def kb-sigs {:fresh [:make2 :k] :borrows (borrows-of :k pair-first)})
(assert (= [[:k :closure]] (kb-sigs :borrows)) "the result borrows from the closure")
(def kb (calls (results {:next (steps 3) :call [[:k :make2 0] [:s :k 1]] :use [[:k 1] [:s 2]]}
                        kb-sigs)
               kb-sigs (lent kb-sigs)))
(assert (= [[:k 2 3] [:s 2 3]] (judged kb (lent kb-sigs))) "k waits for the use of s")
# (fn [i] (pair (get arr 0) ()))
(assert (= [[:k 0]] (lends-of :k (put pair-first :param [[:i 0]]) kb-sigs))
        "a result that borrows from the closure leaves each parameter lent")
```

Where `arr` is a definition instead, the callee's facts name it, and the
caller takes it as a free name of its own. So `peek2` returns a result
borrowed through `arr`, and `wrap2` one that borrows from `arr`:

```lisp
# (def arr @[@[1]])
# (defn peek [] (get arr 0))
# (defn peek2 []
#   (peek))                     # 0  e is read out of arr   1  e is returned   2  the exit
(def peek2 (results {:next [[0 1] [1 2]] :call [[:e :peek 0]] :return [[:e 1]]}
                    (signature :peek (put read-first :captured []))))
(assert (= [[:peek2 :arr]] ((signature :peek2 peek2) :through)) "arr is a free name of peek2")

# (defn pf [] (pair (get arr 0) ()))
# (defn wrap2 []
#   (pf))                       # 0  s is made, and borrows from arr   1  s is returned   2  the exit
(def pf-sigs {:fresh [:pf] :borrows (borrows-of :pf (put pair-first :captured []))})
(def wrap2 (calls (results {:next [[0 1] [1 2]] :call [[:s :pf 0]] :return [[:s 1]]} pf-sigs)
                  pf-sigs (lent pf-sigs)))
(assert (= [[:wrap2 [:free :arr]]] (borrows-of :wrap2 wrap2 (lent pf-sigs)))
        "and of wrap2, whose result borrows from it")
```
