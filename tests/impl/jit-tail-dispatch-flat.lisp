(elle/epoch 15)
# audited: 2026-11-11
# Mutual tail recursion through compiled functions runs 200,000 steps deep on a flat stack and frees what each step allocates.
# docs/impl/jit.md
#
# `ping` and `pong` tail-call each other, and each step hands the next a fresh
# pair. The trampoline that owns the activation runs every tail callee in
# place of the body, and a callee with compiled code runs compiled there. A
# compiled callee that tail-calls again hands the call back to the same
# trampoline. The counter-factual is a trampoline that nests the compiled
# callee's own tail call, which grows the native stack by several native frames
# per step, and the recursion halts with `:stack-overflow` long before its end.
#
# The gauge reads the other side: each step's pair is moved into the callee as
# its parameter, and the callee releases it. A compiled callee that mints a
# reference of its own to a moved argument, as a callee in call position does,
# keeps one pair per step, and the delta reads about 200,000.
#
# Two faces. In the first both functions are compiled, so the trampoline is
# the one a compiled caller hands its tail call to. In the second `ping-i`
# takes a `&named` parameter, which the JIT refuses, so `ping-i` runs in the
# interpreter and its tail call to `pong-i` is resolved by the interpreter's
# trampoline.
#
# The sidecar compiles on the VM thread at each function's first call, and
# turns the graft off, so each tail call stays a call.

(defn ping [p]
  (let [n (first p)]
    (if (= n 0)
      :done
      (pong (%pair (- n 1) nil)))))

(defn pong [p]
  (ping (%pair (first p) nil)))

(defn ping-i [p &named unused]
  (let [n (first p)]
    (if (= n 0)
      :done
      (pong-i (%pair (- n 1) nil)))))

(defn pong-i [p]
  (ping-i (%pair (first p) nil)))

(def depth 200000)

(defn measure [start]
  "Run the recursion `start` begins, `depth` steps deep, after a warm-up, and
  answer how many objects it left behind."
  (start 3)
  (jit/rejections)
  (def before (arena/count))
  (assert (= :done (start depth)) "the recursion runs to its end")
  (%sub (arena/count) before))

(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(def d-compiled (measure (fn [n] (ping (%pair n nil)))))
(def d-interp (measure (fn [n] (ping-i (%pair n nil)))))

(when (vm/config :jit)
  (assert (and (jit? ping) (jit? pong)) "precondition: ping and pong compile")
  (assert (not (jit? ping-i)) "precondition: ping-i stays interpreted")
  (assert (jit? pong-i) "precondition: pong-i compiles"))

(bounded? d-compiled "compiled to compiled")
(bounded? d-interp "interpreted to compiled")

(println "jit-tail-dispatch-flat: ok")
