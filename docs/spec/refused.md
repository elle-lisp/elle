# What the verifier refuses

<!-- audited: 2026-11-24 -->

The instructions of the region IR that have no rule yet, and why the verifier refuses each one.

[The region IR](ir.md) refuses each count instruction, each slot instruction,
and the five that store or allocate by a run-time tag. [The facts](facts.md)
gives every other kind a rule, but for the kinds below. A kind whose result
depends on a run-time tag has no rule, and neither has a kind whose facts the
vocabulary cannot state yet. A refused kind gives `next` and `use` alone
([the points](points.md)), and the function records the refusal.

| Kind | Why |
|------|-----|
| LoadCapture, StoreCapture | each checks at run time whether the entry is a cell |
| LoadCaptureRaw at an index past the capture count | it reads a local slot |
| Get | it dispatches on the container's tag |
| Freeze, Thaw | each answers its argument itself where it already has the target mutability, and a copy elsewhere |
| ArrayMutSliceFrom | its result takes the mutability of its input |
| LoadResumeValue | an Emit's resume block takes the resume value as its parameter |
| CallArrayMut, TailCallArrayMut, ArrayMutExtend | a spliced call passes arguments at positions the IR does not hold |
| PushParamFrame, PopParamFrame | no rule says what a parameter frame holds |
| Eval whose environment register no Const of nil writes | [the ownership rules](ownership.md) do not decide an eval that passes an environment |
| Call to a native with no `:spec` | [signatures](signature.md) refuse it |

A refusal is `[what p]`: `what` is the kind, or the native's name for a call to
a native with no `:spec`.

## The module

`refusal` answers what refuses a node of a function, or nil, given a struct
from each register to the node that writes it and the unit's closure
constants:

```lisp
(fn []
  (def {:fence fence} ((import "../spec.md")))
  (def {:native native} ((import "./form.md")))

  (defn refusal [fun writer constants node]
    "The kind or the native's name that refuses NODE of FUN, or nil."
    nil)

  (def refusals (fence 'refusals
    :doc "Each refused kind, a slot read, an eval that passes an environment and an undeclared native are refused, and nothing else."
    :facts [:get nil :load-capture-raw nil :eval "first" nil :decref-region nil]
    :check (fn [want]
             (def writer {0 {:kind :const :value nil} 1 {:kind :param :index 0}
                          2 {:kind :value-const :value (get (native "first") :fn)}
                          3 {:kind :value-const :value (get (native "%push-array-mut") :fn)}})
             (def nodes [{:kind :get :dst 4 :obj 1 :key 0}
                         {:kind :load-capture-raw :dst 4 :index 0}
                         {:kind :load-capture-raw :dst 4 :index 1}
                         {:kind :eval :dst 4 :expr 1 :env 0}
                         {:kind :eval :dst 4 :expr 1 :env 1}
                         {:kind :call :dst 4 :func 2 :args [1]}
                         {:kind :tail-call :dst 4 :func 3 :args [1 0]}
                         {:kind :decref-region :region-id 0}
                         {:kind :list :dst 4 :head 0 :tail 1}])
             (= want (->array (map (fn [n] (refusal {:captures ['a]} writer @[] n)) nodes))))))

  {:refusal refusal :fences [refusals]})
```
