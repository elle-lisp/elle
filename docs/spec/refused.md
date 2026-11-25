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
| Thaw | its fresh mutable copy holds what its operand holds, and [the ownership rules](ownership.md) do not decide a mutable value made over an operand the fiber does not own |
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
  (def {:callee-of callee-of} ((import "./callees.md")))

  # The kinds the region IR refuses, and the kinds this document refuses whatever their fields.
  (def kinds
    |:incref-region :decref-region :incref-value-region :decref-value-region :decref-cell-region
     :decref-postponed :adopt-region :adopt-cell-region :adopt-into-activation :free-region-group
     :assert-region-matches :load-local :store-local :put :del :intr-push :intr-string-push
     :intr-bytes-push :load-capture :store-capture :get :thaw :array-mut-slice-from
     :load-resume-value :call-array-mut :tail-call-array-mut :array-mut-extend :push-param-frame
     :pop-param-frame|)

  (defn refusal [fun writer constants node]
    "The kind or the native's name that refuses NODE of FUN, or nil."
    (def kind (get node :kind))
    (def env (get writer (get node :env)))
    (cond
      (has? kinds kind) kind
      (= kind :load-capture-raw) (when (>= (get node :index) (length (get fun :captures))) kind)
      (= kind :eval) (unless (and (= (get env :kind) :const) (nil? (get env :value))) kind)
      (has? |:call :suspending-call :tail-call| kind)
        (let [f (callee-of fun writer constants (get node :func))]
          (when (and (string? f) (nil? (get (native f) :spec))) f))
      true nil))

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
