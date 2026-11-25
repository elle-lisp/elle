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
[datalog](datalog.md) says. A step that reads the elements of a container as
one value names them `[:elem p]`.

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
| StructRest | `alloc(d, p)`, `immutable(d)`, `read([:elem p], s, p)` and `operand([:elem p], d, p)` |
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
  (def {:same? same? :function function :make-block make-block} ((import "./points.md")))

  (defn facts-of [unit]
    "One struct per function of UNIT: its :id, :name, :facts and :refused."
    (map (fn [fun] {:id (get fun :id) :name (get fun :name) :facts {} :refused []}) unit))

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
    {:param [[0 0]] :immediate [1] :alloc [[2 2]] :immutable [2] :operand [[0 2 2] [1 2 2]]
     :use [[0 2] [1 2] [2 4] [1 4] [2 5]] :release [[0 3]] :return [[2 5]] :next (exits 5)
     :call [[3 "%push-array-mut" 4]] :arg [[2 0 4] [1 1 4]] :store [[1 2 4]]})
  (def straight (fence 'straight
    :doc "A Param names its parameter, a List makes an immutable value of its operands, a Release uses nothing, and an ArrayMutPush is a call."
    :facts straight-facts
    :check (gives [(function [(make-block 0 [{:kind :param :dst 0 :index 0}
                                        {:kind :const :dst 1 :value nil}
                                        {:kind :list :dst 2 :head 0 :tail 1}
                                        {:kind :release :value 0}
                                        {:kind :array-mut-push :dst 3 :array 2 :value 1}]
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
