(elle/epoch 18)
# audited: 2026-11-23
# A direct self-call whose callee ends in a tail call to another closure runs that call and answers its value.
# docs/impl/jit-selfcall.md
#
# `walk` calls itself directly: its argument is proven an integer. At the
# bottom it tail-calls `climb`, a closure, so the compiled callee returns the
# tail-call sentinel and leaves the call pending on the VM. The direct call
# runs it. `climb` is recursive, so the compiler cannot splice its body into
# `walk`, and the call stays a tail call.
#
# The counter-factual: a direct call that read its callee's answer as a value.
# The level above the bottom would add one to the sentinel, which is not a
# number, and no level would see `climb`'s answer.

(defn climb [x]
  (if (< x 40) (climb (+ x 1)) x))

(defn walk [n]
  (if (= n 0)
    (climb 35)
    (+ 1 (walk (- n 1)))))

(assert (= (walk 0) 40) "the bottom alone answers climb's value")
(assert (= (walk 3) 43) "each level above the bottom adds one to it")
(assert (= (walk 3) 43) "a second run answers the same")

(println "jit-selfcall-tail: ok")
