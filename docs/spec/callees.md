# The callees of the region IR

<!-- audited: 2026-11-24 -->

How a call of the region IR names its callee, and the facts a native callee's signature gives at the call.

[The facts](facts.md) gives the rest of each step's facts, and
[call](call.md) reads `call` and `arg` beside the callee's signature.

## Callees

A call names its callee by what wrote the callee's register:

| The register's writer | The callee |
|-----------------------|------------|
| a ValueConst of a native | the native's name in `compile/primitives` |
| a ValueConst of a closure | `[:constant k]`, the closure constant `k` of the unit, numbered in order of first sight and compared with `identical?` |
| a MakeClosure of the closure id `c` | `[:closure c]` |
| a LoadSelf in the function whose closure id is `c` | `[:closure c]` |
| anything else | the register |

The closure constants are numbered across the unit, so two functions that call
one constant name it alike.

## A call

A call gives `call(d, f, p)`, `f` its callee, and `arg(y, i, p)` for each
argument `y` at position `i`. A native callee also gives the facts of
[its signature](signature.md), its `:spec`, read at the call. A resumed
parameter gives `resume(y, p)`, a crossing parameter `cross(y, p)`, and a
payload result `payload(d, y, p)`. A fresh result gives `alloc(d, p)`, and a
fresh immutable result also gives `immutable(d)` and `operand(y, d, p)` for
each argument at a returned parameter. A parameter stored into a place gives
`store(y, c, p)`, `c` the argument `j` for `[:param j]` or `d` for `:result`.
Every other mode and kind is the callee's signature, which [call](call.md)
reads.

## The module

`callee-of` names the callee in a register, given the function, a struct from
each register to the node that writes it, and the unit's closure constants.
`call-facts` answers the facts of a call to the callee `f`:

```lisp
(fn []
  (def {:fence fence} ((import "../spec.md")))
  (def {:native native} ((import "./form.md")))
  (def {:same? same?} ((import "./points.md")))

  (def natives (compile/primitives))

  (defn constant [constants v]
    "The index of the closure V in CONSTANTS, compared with identical?, appended at first sight."
    (def @k 0)
    (while (and (< k (length constants)) (not (identical? (get constants k) v)))
      (assign k (+ k 1)))
    (when (= k (length constants)) (push constants v))
    k)

  (defn callee-of [fun writer constants r]
    "The callee in the register R of FUN, by the node WRITER names for it; CONSTANTS grows."
    (def w (get writer r))
    (def v (get w :value))
    (match (get w :kind)
      :value-const (cond
                     (native-fn? v) (get (first (filter (fn [e] (identical? (get e :fn) v)) natives))
                                         :name)
                     (closure? v) [:constant (constant constants v)]
                     true r)
      :make-closure [:closure (get w :closure-id)]
      :load-self [:closure (get fun :id)]
      _ r))

  (defn call-facts [f d args p]
    "The facts of the call at P to the callee F, with ARGS, whose result is D."
    (def facts @{})
    (defn add [k t] (if (has? facts k) (push (get facts k) t) (put facts k @[t])))
    (add :call [d f p])
    (each i in (range (length args)) (add :arg [(get args i) i p]))
    (defn signed [spec]
      (def result (get spec :result))
      (def immutable (and (= result :fresh) (not (get spec :mutable))))
      (each i in (range (length args))
        (let [y (get args i)
              params (get spec :params)]
          (match (if (< i (length params)) (get params i) (get spec :rest))
            :resumed (add :resume [y p])
            :crosses (add :cross [y p])
            :returned (when immutable (add :operand [y d p]))
            [:stored place] (add :store [y (if (= place :result) d (get args (get place 1))) p])
            _ nil)))
      (when (= result :fresh) (add :alloc [d p]))
      (when immutable (add :immutable d))
      (match result [:payload i] (add :payload [d (get args i) p]) _ nil))
    (def spec (when (string? f) (get (first (filter (fn [e] (= (get e :name) f)) natives)) :spec)))
    (unless (nil? spec) (signed spec))
    (from-pairs (map (fn [[k v]] [k (freeze v)]) (pairs facts))))

  (def push-fn (get (native "%push-array-mut") :fn))
  (def one (fn [] 1))
  (def two (fn [] 2))
  (def names (fence 'names
    :doc "A native, a closure constant, a closure id and the function itself each name a callee; another register names itself."
    :facts ["%push-array-mut" [:constant 0] [:closure 4] [:closure 3] [:constant 1] 5 [:constant 0]]
    :check (fn [want]
             (def writer {0 {:kind :value-const :value push-fn} 1 {:kind :value-const :value one}
                          2 {:kind :make-closure :closure-id 4} 3 {:kind :load-self}
                          4 {:kind :value-const :value two} 5 {:kind :param :index 0}})
             (def constants @[])
             (= want (->array (map (fn [r] (callee-of {:id 3} writer constants r)) [0 1 2 3 4 5 1]))))))

  (def call (fence 'call
    :doc "A call names its callee and arguments, and a native's signature stores, makes, resumes, crosses and hands over its payload."
    :facts [{:call [[4 "%push-array-mut" 4]] :arg [[3 0 4] [2 1 4]] :store [[2 3 4]]}
            {:call [[5 "array" 5]] :arg [[1 0 5] [2 1 5]] :alloc [[5 5]] :immutable [5]
             :operand [[1 5 5] [2 5 5]]}
            {:call [[6 "fiber/resume" 6]] :arg [[7 0 6] [8 1 6]] :resume [[7 6]] :cross [[8 6]]
             :payload [[6 7 6]]}
            {:call [[9 "@array" 7]] :arg [[1 0 7]] :alloc [[9 7]] :store [[1 9 7]]}
            {:call [[10 [:closure 2] 8]] :arg [[1 0 8]]}]
    :check (fn [want]
             (def got [(call-facts "%push-array-mut" 4 [3 2] 4) (call-facts "array" 5 [1 2] 5)
                       (call-facts "fiber/resume" 6 [7 8] 6) (call-facts "@array" 9 [1] 7)
                       (call-facts [:closure 2] 10 [1] 8)])
             (all? (fn [i] (same? (get want i) (get got i))) (range (length want))))))

  {:callee-of callee-of :call-facts call-facts :fences [names call]})
```
