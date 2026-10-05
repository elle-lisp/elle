(elle/epoch 14)
# audited: 2026-10-05
# attune: positive signal permit (dual of squelch)

# ── Permitted signals pass through ────────────────────────────────
(def yielder (fn [] (yield 42)))
(def attuned (attune |:yield :error| yielder))
(def coro (fiber/new attuned |:yield|))
(fiber/resume coro nil)
(assert (= (fiber/value coro) 42))
(println "attune-permitted-passes: ok")

# ── Non-permitted signals are converted to :error ─────────────────
(def io-fn
  (fn []
    (println "side effect")
    :done))
(def attuned-no-io (attune |:error| io-fn))
(try
  (attuned-no-io)
  (catch e (assert (= (get e :error) :signal-violation))
         (println "attune-blocks-unpermitted: ok")))

# ── attune composes with squelch ──────────────────────────────────
(def multi (fn [] (yield 1)))
(def step1 (attune |:yield :error| multi))
(def step2 (squelch step1 :yield))
# step2: attune allows only yield+error, then squelch removes yield
# net effect: only error is possible
(try
  (step2)
  (catch e (assert (= (get e :error) :signal-violation))
         (println "attune-composes-with-squelch: ok")))

# ── Compile-time signal inference ─────────────────────────────────
# attune narrows the signal for interprocedural tracking
(def narrow (attune |:yield| (fn [] (yield 1))))
(def coro2 (fiber/new narrow |:yield|))
(fiber/resume coro2 nil)
(assert (= (fiber/value coro2) 1))
(println "attune-inference: ok")

# ── attune! preamble: compile-time signal ceiling ─────────────────
# Function declares it emits at most :yield — compiler verifies
(defn yielding-only []
  (attune! :yield)
  (yield 99))

(def coro3 (fiber/new yielding-only |:yield|))
(fiber/resume coro3 nil)
(assert (= (fiber/value coro3) 99))
(println "attune!-ceiling: ok")

# ── attune! rejects functions that exceed the ceiling ─────────────
# This should fail at compile time: function does IO but ceiling is :yield
(def failed
  (try
    (eval '(fn []
             (attune! :yield)
             (println "oops")))
    (catch e e)))
(assert (get failed :error))
(println "attune!-rejects-excess: ok")

# ── attune! checks and does not replace ───────────────────────────
# The function's signal stays the inferred one, so a caller sees what the
# body does. Counter-factual: the signal became the ceiling, so a body that
# only raises reported a yield it never raises.
(def attuned-sig
  (compile/signal (compile/analyze "(defn f [x] (attune! |:yield :error|) (+ x 1))")
                  :f))
(assert (= |:error| (get attuned-sig :bits))
        "the signal is what the body raises")
(assert (empty? (get attuned-sig :propagates)))

(def quiet-sig
  (compile/signal (compile/analyze "(defn f [] (attune! :error) 1)") :f))
(assert (get quiet-sig :silent)
        "a ceiling with room to spare leaves the body silent")
(println "attune!-checks-only: ok")

# ── A bad spec names attune! ──────────────────────────────────────
# Counter-factual: the message began with emit:, a form the program never wrote.
(def [bad-ok? bad-err]
  (protect (compile/whole-module "(defn f [] (attune! :nosuch) 1)" "<attune>")))
(assert (not bad-ok?))
(assert (string/contains? (get bad-err :message)
                          "attune!: unknown signal keyword :nosuch"))
(println "attune!-bad-spec: ok")
