(elle/epoch 15)
# audited: 2026-11-11
# A function the interpreter reaches only in tail position still becomes hot and compiles.
# docs/impl/jit.md
#
# `ping` and `pong` tail-call each other. The top level calls `ping` once, in
# a non-tail position, so the whole recursion runs on one trampoline and every
# later call is a tail call. `pong` is reached by nothing else. Count only
# non-tail calls, and `pong` is never counted: it never becomes hot, never
# compiles, and `(jit? pong)` reads false.
#
# `ping` builds a closure, which the JIT refuses to compile, so `ping` runs as
# bytecode throughout and every tail call to `pong` is one the interpreter
# resolves in `tail_call_inner`. A `ping` that compiled would make those tail
# calls from compiled code, whose count would promote `pong` on its own.
# jit-compiled-tail-promotes-callee.lisp holds the compiled tier's count.
#
# The sidecar compiles on the VM thread and turns the graft off: a call to a
# function the compiler can see becomes the function's body
# (docs/impl/graft.md), and a tail call grafted away counts nothing.
#
# In a build without the JIT `(vm/config :jit)` reads nil and nothing compiles,
# so the assertions need a JIT.

(defn ping [n]
  (if (= n 0)
    :done
    (pong (fn [] (- n 1)))))

(defn pong [next]
  (ping (next)))

(assert (= :done (ping 60)) "the mutual recursion runs to its end")
(jit/rejections)

(when (vm/config :jit)
  (assert (not (jit? ping))
          "precondition: ping must stay interpreted, else its compiled tail calls count pong")
  (assert (jit? pong)
          "a function the interpreter reaches only in tail position must still be promoted"))

(println "ok: an interpreted tail call promotes its callee")
