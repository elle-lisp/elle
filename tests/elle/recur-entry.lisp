(elle/epoch 12)
## tests/elle/recur-entry.lisp
##
## A self-recursive closure's body can be ENTERED through many doors, not just
## the interpreter's own call path: a JIT-compiled caller falling back to the
## interpreter for an uncompiled callee, a forced-tier dispatch
## (compile/run-on), a fiber's first resume, or a measured-thunk entry
## (arena/allocs). Whichever door the body enters through, its self-reference
## must resolve to the closure itself — the executing-closure register is
## handed across EVERY entry boundary, not only the nested-call one
## (docs/impl/vm.md § The executing-closure register).
##
## The hazard is a nil (or stale) self-reference: the recursion dispatches to
## nil ("Cannot call nil") or to the wrong closure. No region leaks and no
## freed page is read, so only value assertions catch it. Siblings cover the
## other boundaries: recur-after-yield (suspend/resume), recur-after-tail-call
## (frame replacement), recur-as-value (value handoff).

## 1. JIT→interpreter boundary. compile/run-on :jit force-compiles ONLY the
## caller; the self-recursive callee stays uncompiled, so the compiled caller's
## call falls back to the interpreter — the callee's body enters through the
## JIT helper's fallback door. `go` is called ONLY from inside the compiled
## caller, so the fallback (not a JIT-to-JIT dispatch) is what runs.
# Gate on JIT availability: a build with no JIT tier compiled in
# (--no-default-features, e.g. the aarch64 no-features job) rejects
# (compile/run-on :jit …) with :error :tier-rejected. This file's first case
# forces the :jit tier, so re-raise as a loud :gated — `elle test` records a
# file-level SKIP and a direct run prints "SKIP (gated)" (exit 0), matching
# compress.lisp.
(def _jit-available
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "JIT tier not compiled in"))
      true)))

(def go
  (letrec [g (fn [ls]
               (if (empty? ls)
                 0
                 (%add 1 (g (rest ls)))))]
    g))
## `go` is bound to a letrec EXPRESSION's escaping lambda, so calls through it
## type as unknown; the allocation-free coerce-guard on the result discharges
## the %add operand contract (docs/intrinsics.md § The contract).
(defn caller [xs]
  (let [r (go xs)]
    (if (%int? r) (%add 100 r) -1)))
(assert (= (compile/run-on :jit caller (list 1 2 3)) 103)
        "non-tail: a JIT caller's interpreter-fallback callee recurses as itself")

## 2. JIT tail-call resolution. The compiled caller ends in a tail call to the
## uncompiled callee; the JIT returns a tail-call sentinel and the runtime
## resolves it by entering the callee's body — another entry door.
(defn tail-caller [xs]
  (go xs))
(assert (= (compile/run-on :jit tail-caller (list 1 2 3 4)) 4)
        "a JIT tail-call resolution into the interpreter must recurse as itself")

## 3. Forced bytecode tier. compile/run-on :bytecode enters the target
## closure's body directly — the target itself is the self-recursive closure.
(assert (= (compile/run-on :bytecode go (list 1 2)) 2)
        "a self-recursive closure forced onto the bytecode tier must recurse as itself")

## 4. Fiber body. The fiber's first resume enters the body of the closure the
## fiber was created from; here that closure is itself self-recursive.
## `count-down` is passed to fiber/new as a value, so call-site forwarding
## cannot prove `m`; the diverging guard does (the fiber mask admits :error).
(def f
  (letrec [count-down (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1)
                          0
                          (%add 1 (count-down (%sub m 1)))))]
    (fiber/new count-down |:error|)))
(assert (= (fiber/resume f 3) 3)
        "a self-recursive fiber body must recurse as itself on first resume")

## 5. Measured thunk. arena/allocs runs a user thunk on the current fiber;
## here the thunk itself is self-recursive (terminating via a mutable counter,
## since a zero-argument recursion carries no decreasing argument).
(def @steps 0)
(def sharp
  (letrec [tick (fn []
                  (if (%lt steps 3)
                    (begin
                      (assign steps (%add steps 1))
                      (%add 1 (tick)))
                    0))]
    tick))
(assert (= (first (arena/allocs sharp)) 3)
        "a self-recursive thunk measured by arena/allocs must recurse as itself")

## 6. The stdlib-HOF integration shape: non-tail self-recursion whose body
## calls stdlib higher-order functions (map first / map rest), which drag in
## the stdlib's own self-recursive helpers under region churn. Exercises the
## adaptive-JIT compile window (a hot caller compiled while its callee is
## still interpreted) when run under an adaptive policy.
(def myrec
  (fn [lst]
    (letrec [walk (fn (ls)
                    (if (any? empty? ls)
                      ()
                      (list (map first ls) (walk (map rest ls)))))]
      (walk lst))))
(def @j 0)
(while (%lt j 300)
  (myrec (list (list 1 2) (list 3 4)))
  (assign j (%add j 1)))
(assert (= j 300)
        "300 iterations of nested self-recursive HOF walking completed")

(println "recur-entry: ok")
