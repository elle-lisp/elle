(elle/epoch 15)
# audited: 2026-11-11
# A callee that compiled code reaches only in tail position still becomes hot and compiles.
# docs/impl/jit.md
#
# `hand-to` is warmed on `decoy` until it compiles, so every later
# `(hand-to callee 1)` runs as native code and reaches `callee` through the
# tail-call helper, `jit_tail_call_inner`. Count only non-tail calls, and
# `callee` is counted nowhere: it never becomes hot, never compiles, and
# `(jit? callee)` reads false. jit-tail-promotes-callee.lisp holds the
# interpreter's count.
#
# The sidecar compiles on the VM thread, so `hand-to` is in the cache before
# `callee` is called at all, and no interpreted window lets the interpreter
# count `callee` instead. It also turns the graft off: a call to a function the
# compiler can see becomes the function's body (docs/impl/graft.md), and a
# caller nothing calls never compiles.
#
# In a build without the JIT `(vm/config :jit)` reads nil and nothing compiles,
# so the assertions need a JIT.

(defn hand-to [f x]
  (f x))

(defn decoy [x]
  x)

(defn callee [x]
  (+ x 1))

(def @i 0)
(while (< i 30)
  (hand-to decoy 1)
  (assign i (+ i 1)))
(jit/rejections)

(assign i 0)
(while (< i 30)
  (assert (= 2 (hand-to callee 1)) "the tail call answers the callee's value")
  (assign i (+ i 1)))
(jit/rejections)

(when (vm/config :jit)
  (assert (jit? hand-to)
          "precondition: the caller must be JIT-compiled, else the interpreter makes the tail call and the assertion below reads its count")
  (assert (jit? callee)
          "a callee compiled code reaches only in tail position must still be promoted"))

(println "ok: a compiled tail call promotes its callee")
