# The facts of the region IR

<!-- audited: 2026-11-24 -->

The facts each instruction of the region IR gives, beside its point, its uses and its call.

[The region IR](ir.md) says what the IR holds, and [the data form](form.md)
how Elle code reads it. [Datalog](datalog.md) and the documents it names
define each fact below. They write the facts of the kernel forms by hand, and
this document derives the same facts from a function of the IR.
[The points](points.md) gives each step's point, its `next`, its `use` facts,
and what a jump hands to a block's parameters. [The callees](callees.md) gives
a call's facts, and [refused](refused.md) lists the kinds the verifier refuses.

## Values

A value is named by the register that holds it. A step that takes a value out
of a container, where no register receives the value, names it `[:out p]`, as
[datalog](datalog.md) says.

## The rules

Every kind gives the facts of [the points](points.md) beside those below. In
each row, `d` is the register the instruction writes, `s` the container or the
scrutinee it reads, `c` a cell, and `i` an index.

| Kind | Facts |
|------|-------|
| Param | `param(d, i)`, and no other fact |
| Release | `release(v, p)`, and no other fact |
| Const | `immediate(d)` |
| ValueConst | `immediate(d)` for an immediate or a native, and `free(d)` for any other value |
| MaterializeConst | `alloc(d, p)` and `immutable(d)` |
| LoadCaptureRaw | `free(d)` and `captured(d)`, at an index below the capture count |
| LoadSelf | `free(d)` |
| MakeClosure | `alloc(d, p)`, and `capture(x, d, p)` for each capture `x` |
| List | `alloc(d, p)`, `immutable(d)`, and `operand(x, d, p)` for its head and its tail |
| MakeArrayMut | `alloc(d, p)`, and `operand(x, d, p)` for each element |
| Freeze, StructRest, ArraySliceFrom | `alloc(d, p)`, `immutable(d)` and `operand(s, d, p)`: a fresh immutable copy borrows what it holds from `s`, as `%freeze`'s signature says at a call |
| MakeCaptureCell | `alloc(d, p)` and `operand(x, d, p)` for its value |
| LoadCaptureCell | `read(d, c, p)` |
| StoreCaptureCell | `store(x, c, p)` and `remove([:out p], c, p)` |
| First, Rest, FirstOrNil, RestOrNil, FirstDestructure, RestDestructure, ArrayMutRefDestructure, ArrayMutRefOrNil, StructGetOrNil, StructGetDestructure | `read(d, s, p)`, `s` the container |
| Pop | `remove(d, s, p)` |
| ArrayMutPush | the facts of a call to the native `%push-array-mut` with the array and the value |
| BinOp, UnaryOp, Convert, Compare, Identical, Length, Has, TypeOf, ArrayMutLen, the type checks | `immediate(d)` |
| CheckSignalBound | `use` alone |
| Call, SuspendingCall | the facts of [a call](callees.md) |
| TailCall | the facts of a call, and `return(d, p)` |
| MatchFail | `cross(s, p)` for the scrutinee and `alloc(d, p)` |
| Eval | `alloc(d, p)` |
| Return | `return(v, p)` |
| Emit | `cross(v, p)` |
| Jump, Branch, Unreachable | no fact but those of [the points](points.md) |

The type checks are IsNil, IsPair, IsArray, IsArrayMut, IsStruct, IsStructMut,
IsSet, IsSetMut, IsEmpty, IsBool, IsInt, IsFloat, IsString, IsKeyword,
IsSymbolCheck, IsBytes, IsBox, IsClosure and IsFiber. Each answers an
immediate and allocates nothing, so its value has no owner fact.

A cell is a container of one value, so an overwrite takes the old value out,
as `put` does in [datalog](datalog.md). A kind [refused](refused.md) lists
gives no fact of this table.

## The module

`facts-of` answers one struct per function of a unit, in the unit's order. Its
`:facts` maps each fact's key to its tuples, keyed as the fences of
[datalog](datalog.md) key them, and its `:refused` holds each refusal
[refused](refused.md) names. Each fence below is a hand-built unit and the
facts it gives:

```lisp
(fn []
  (def {:fence fence} ((import "../spec.md")))
  (def {:native native} ((import "./form.md")))
  (def {:points points :same? same? :function function :make-block make-block}
    ((import "./points.md")))
  (def {:callee-of callee-of :call-facts call-facts} ((import "./callees.md")))
  (def {:refusal refusal} ((import "./refused.md")))

  (defn constant? [v]
    "Whether V lives in no region: an immediate or a native."
    (match (type-of v)
      (or :nil :boolean :integer :float :symbol :keyword) true
      :list (empty? v)
      _ (native-fn? v)))

  (defn function-facts [fun constants]
    "The struct facts-of answers for FUN, numbering closure constants in CONSTANTS."
    (def {:nodes nodes :next next :use use :pass pass :alloc alloc} (points fun))
    (def facts @{})
    (defn add [k t] (if (has? facts k) (push (get facts k) t) (put facts k @[t])))
    (each [k ts] in [[:next next] [:use use] [:pass pass] [:alloc alloc]] (each t in ts (add k t)))
    (def writer @{})
    (each {:node n} in nodes (when (has? n :dst) (put writer (get n :dst) n)))
    (def refused @[])
    (defn call [d f args p]
      (each [k ts] in (pairs (call-facts f d args p)) (each t in ts (add k t))))
    (each p in (range (length nodes))
      (def node (get (get nodes p) :node))
      (defn g [k] (get node k))
      (def [d s c] [(g :dst) (g :src) (g :cell)])
      (def why (refusal fun writer constants node))
      (if why
        (push refused [why p])
        (match (g :kind)
          :param (add :param [d (g :index)])
          :release (add :release [(g :value) p])
          :const (add :immediate d)
          :value-const (add (if (constant? (g :value)) :immediate :free) d)
          :materialize-const (begin (add :alloc [d p]) (add :immutable d))
          :load-capture-raw (begin (add :free d) (add :captured d))
          :load-self (add :free d)
          :make-closure (begin (add :alloc [d p]) (each x in (g :captures) (add :capture [x d p])))
          :list (begin (add :alloc [d p]) (add :immutable d)
                       (add :operand [(g :head) d p]) (add :operand [(g :tail) d p]))
          :make-array-mut (begin (add :alloc [d p]) (each x in (g :elements) (add :operand [x d p])))
          (or :freeze :struct-rest) (begin (add :alloc [d p]) (add :immutable d) (add :operand [s d p]))
          :make-capture-cell (begin (add :alloc [d p]) (add :operand [(g :value) d p]))
          :load-capture-cell (add :read [d c p])
          :store-capture-cell (begin (add :store [(g :value) c p]) (add :remove [[:out p] c p]))
          (or :first :rest) (add :read [d (g :pair) p])
          (or :first-or-nil :rest-or-nil :first-destructure :rest-destructure
              :array-mut-ref-destructure :array-mut-ref-or-nil :struct-get-or-nil
              :struct-get-destructure) (add :read [d s p])
          :pop (add :remove [d s p])
          :array-mut-push (call d "%push-array-mut" [(g :array) (g :value)] p)
          (or :bin-op :unary-op :convert :compare :identical :length :has :type-of :array-mut-len
              :is-nil :is-pair :is-array :is-array-mut :is-struct :is-struct-mut :is-set :is-set-mut
              :is-empty :is-bool :is-int :is-float :is-string :is-keyword :is-symbol-check :is-bytes
              :is-box :is-closure :is-fiber) (add :immediate d)
          (or :call :suspending-call) (call d (callee-of fun writer constants (g :func)) (g :args) p)
          :tail-call (begin (call d (callee-of fun writer constants (g :func)) (g :args) p)
                            (add :return [d p]))
          :match-fail (begin (add :cross [s p]) (add :alloc [d p]))
          :eval (add :alloc [d p])
          :return (add :return [(g :value) p])
          :emit (add :cross [(g :value) p])
          (or :check-signal-bound :jump :branch :unreachable) nil)))
    {:id (get fun :id) :name (get fun :name) :refused (freeze refused)
     :facts (from-pairs (map (fn [[k v]] [k (freeze v)]) (pairs facts)))})

  (defn facts-of [unit]
    "One struct per function of UNIT: its :id, :name, :facts and :refused."
    (def constants @[])
    (->array (map (fn [fun] (function-facts fun constants)) unit)))

  (defn gives [unit & wants]
    "A check that UNIT's functions give WANTS, one [facts refused] per function."
    (fn [_]
      (def answers (facts-of unit))
      (and (= (length wants) (length answers))
           (all? (fn [i]
                   (def [facts refused] (get wants i))
                   (def answer (get answers i))
                   (and (same? facts (get answer :facts))
                        (= (sort refused) (sort (get answer :refused)))))
                 (range (length wants))))))

  (defn exits [n]
    "next(p, p + 1) for each point p below N, and next(N, [:exit N]): a block ending in a Return."
    (concat (map (fn [p] [p (+ p 1)]) (range n)) [[n [:exit n]]]))

  (def straight-facts
    {:param [[0 0]] :immediate [1] :alloc [[2 2] [4 5] [5 6]] :immutable [2 4 5]
     :operand [[0 2 2] [1 2 2] [3 4 5] [4 5 6]]
     :use [[0 2] [1 2] [2 4] [1 4] [3 5] [4 6] [2 7]]
     :release [[0 3]] :return [[2 7]] :next (exits 7)
     :call [[3 "%push-array-mut" 4]] :arg [[2 0 4] [1 1 4]] :store [[1 2 4]]})
  (def straight (fence 'straight
    :doc "A Param names its parameter, a List makes an immutable value of its operands, a Release uses nothing, an ArrayMutPush is a call, and a Freeze and an ArraySliceFrom copy."
    :facts straight-facts
    :check (gives [(function [(make-block 0 [{:kind :param :dst 0 :index 0}
                                        {:kind :const :dst 1 :value nil}
                                        {:kind :list :dst 2 :head 0 :tail 1}
                                        {:kind :release :value 0}
                                        {:kind :array-mut-push :dst 3 :array 2 :value 1}
                                        {:kind :freeze :dst 4 :src 3 :region 0}
                                        {:kind :array-slice-from :dst 5 :src 4 :index 1}]
                                     {:kind :return :value 2})] :params 1)]
                  [straight-facts []])))

  (def resume-facts
    {:param [[0 0]] :alloc [[1 1] [6 4]] :operand [[0 1 1]] :read [[2 1 2]] :store [[2 1 3]]
     :remove [[[:out 3] 1 3]] :cross [[2 4]] :return [[7 6]] :next (exits 6)
     :use [[0 1] [1 2] [1 3] [2 3] [2 4] [6 5] [0 5] [7 6]]})
  (def resume (fence 'resume
    :doc "A cell is a container of one value, an Emit's resume block makes its parameter, and a Get is refused."
    :facts resume-facts
    :check (gives [(function [(make-block 0 [{:kind :param :dst 0 :index 0}
                                        {:kind :make-capture-cell :dst 1 :value 0}
                                        {:kind :load-capture-cell :dst 2 :cell 1}
                                        {:kind :store-capture-cell :cell 1 :value 2}]
                                     {:kind :emit :signal |:yield| :value 2 :resume-label 1})
                              (make-block 1 [{:kind :get :dst 7 :obj 6 :key 0}]
                                     {:kind :return :value 7} :params [6])] :params 1)]
                  [resume-facts [[:get 5]]])))

  (def one (fn [] 1))
  (def free-facts
    {:free [0 1 2 7] :captured [0] :immediate [3] :call [[5 [:constant 0] 5]] :arg [[1 0 5]]
     :use [[3 4] [0 4] [2 5] [1 5] [5 8]] :return [[5 5] [5 8]]
     :next (concat (exits 8) [[5 [:exit 5]]])})
  (def free (fence 'free
    :doc "A capture, a heap constant and the function itself are free names, a TailCall returns, and an undeclared native and a slot are refused."
    :facts free-facts
    :check (gives [(function [(make-block 0 [{:kind :load-capture-raw :dst 0 :index 0}
                                        {:kind :value-const :dst 1 :value "s"}
                                        {:kind :value-const :dst 2 :value one}
                                        {:kind :value-const :dst 3 :value (get (native "first") :fn)}
                                        {:kind :call :dst 4 :func 3 :args [0]}
                                        {:kind :tail-call :dst 5 :func 2 :args [1]}
                                        {:kind :load-capture-raw :dst 6 :index 1}
                                        {:kind :load-self :dst 7}]
                                     {:kind :return :value 5})] :captures ['a])]
                  [free-facts [["first" 4] [:load-capture-raw 6]]])))

  {:facts-of facts-of :fences [straight resume free]})
```

## Not written yet

- The facts of the error exit of an instruction that raises, such as
  FirstDestructure on a value that is not a pair.
- The Datalog rules that read `release`, which the verifier runs.
