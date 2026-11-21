(elle/epoch 18)
# audited: 2026-11-20
# std/portrait reads each function's signal and each callee's, and sorts no function but the silent ones.
# docs/analysis/portrait.md

(def portrait ((import "std/portrait")))
(def p1 (portrait:function (compile/analyze "(defn add [a b] (+ a b))") :add))

# ── A portrait sorts no function but the silent ones ────────────────────
#
# The counter-factual sorted each call into a phase and each function into
# I/O, delegating or yielding, by rules that called port/read pure and an
# error-only function yielding. The signal says what a function does.

(assert (nil? (get p1 :phases)) "a function portrait has no phases")

(def a10
  (compile/analyze "(defn ident [x] x)
(defn inc1 [x] (+ x 1))
(defn rd [p] (port/read p 1))"))
(def mp10 (portrait:module a10))
(assert (= ["ident"] (get mp10 :pure)) "pure lists the silent functions")
(assert (nil? (get mp10 :io-boundary)) "no I/O list")
(assert (nil? (get mp10 :delegating)) "no delegating list")
(assert (nil? (get mp10 :yielding)) "no yielding list")

# ── Failure modes and observations read each callee's signal ────────────
#
# The counter-factual asked compile/signal for each callee by name, which
# knows no primitive, so port/read raised nothing and did no I/O.

(def p-rd (portrait:function a10 :rd))
(assert (any? (fn [f] (= (get f :source) "port/read")) (get p-rd :failures))
        "port/read may raise an error")
(assert (any? (fn [o] (= (get o :kind) :almost-pure)) (get p-rd :observations))
        "port/read is rd's one source of I/O")

# ── A render keeps the order of each list ───────────────────────────────

(def mp11
  (portrait:module (compile/analyze "(defn alpha [] 1)\n(defn beta [] 2)")))
(assert (string/contains? (portrait:render-module mp11) "alpha, beta")
        "a list renders in its own order, not reversed")

(println "portrait-signals: all tests passed")
