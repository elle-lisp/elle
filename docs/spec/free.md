# Free-name stores, in Datalog

<!-- audited: 2026-11-20 -->

How a caller reads a callee's store into a free name: into a definition the caller names, or into the callee, as Datalog that runs on proof.

[Stored](stored.md) derives the `stored` facts, and [call](call.md) reads
them. `places` in stored and `call-facts` in call run the clauses here, and the
fences include [place](place.md). [Signatures](signature.md) holds the
Free-name store rule in words.

## The rules

| Rule | Datalog |
|------|---------|
| Free-name store | `free(n) :- call(r, f, p), arg(y, i, p), stored(f, i, [:free n]).` |
| Free-name store | `stored(f, i, :closure) :- probe(i), chain(w, p), is(w, n), captured(n), not open.` |
| Free-name store | `store(y, f, p) :- call(r, f, p), arg(y, i, p), stored(f, i, :closure).` |

A definition is bound in the body of its file, and a function defined after it
can name it ([bindings](../bindings.md)). So the caller names the definition
that its callee stores into, though its own body does not. A capture names a
value of the closure's maker, which the callee's caller cannot name. That caller
holds the closure, which owns each value it captures late and is outlived by
each value it borrows, so the store lands in the closure.

Below, `m2` hands `d` to `note`, from [stored](stored.md), which stores it into
the definition `log`. `m3` hands `d` to the definition `logger`, a closure that
stores it into its capture `log`, and `m4` hands `d` to such a closure that it
made and returns:

```lisp
(include-file "place.md")

# (defn m2 [d] (note d) nil)    # 0  n is the call's result   1  z is returned   2  the exit
(def m2 {:next [[0 1] [1 2]] :param [[:d 0]] :call [[:n :note 0]] :arg [[:d 0 0]]
         :use [[:d 0]] :return [[:z 1]]})
(def m2c (calls m2 {:stored (stored-of :note note)}))
(assert (= [:log] (m2c :free)) "log is a free name of m2")
(assert (= [[:m2 0 [:free :log]]] (stored-of :m2 m2c)) "so m2 stores d into log")

# (defn make-logger [] (def log @[]) (fn [x] (push log x) nil))
# (def logger (make-logger))
# the closure:                  # 0  x is stored into log   1  z is returned   2  the exit
(def logger {:next [[0 1] [1 2]] :param [[:x 0]] :free [:log] :captured [:log]
             :store [[:x :log 0]] :use [[:x 0] [:log 0]] :return [[:z 1]]})
(def lsigs {:stored (stored-of :logger logger)})
(assert (= [[:logger 0 :closure]] (lsigs :stored)) "the closure stores x into itself")
# (defn m3 [d] (logger d) nil)
(def m3 (merge m2 {:free [:logger] :call [[:n :logger 0]]}))
(assert (= [[:m3 0 [:free :logger]]] (stored-of :m3 (calls m3 lsigs))) "m3 stores d into logger")
# (defn m4 [d]
#   (def k (make-logger))       # 0  k is made
#   (k d)                       # 1  d is stored into k
#   k)                          # 2  k is returned   3  the exit
(def m4 {:next (steps 3) :param [[:d 0]] :alloc [[:k 0]] :call [[:n :k 1]] :arg [[:d 0 1]]
         :use [[:k 1] [:d 1]] :return [[:k 2]]})
(assert (= [[:m4 0 :result]] (stored-of :m4 (calls m4 {:stored [[:k 0 :closure]]})))
        "m4 stores d into the closure it returns")
```
