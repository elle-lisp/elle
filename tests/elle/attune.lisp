# attune: positive signal permit (dual of squelch)

# ── Permitted signals pass through ────────────────────────────────
(def yielder (fn [] (yield 42)))
(def attuned (attune |:yield :error| yielder))
(def coro (fiber/new attuned |:yield|))
(fiber/resume coro nil)
(assert (= (fiber/value coro) 42))
(println "attune-permitted-passes: ok")

# ── Non-permitted signals are converted to :error ─────────────────
(def io-fn (fn [] (println "side effect") :done))
(def attuned-no-io (attune |:error| io-fn))
(try (attuned-no-io)
  (catch e
    (assert (= (get e :error) :signal-violation))
    (println "attune-blocks-unpermitted: ok")))

# ── attune composes with squelch ──────────────────────────────────
(def multi (fn [] (yield 1)))
(def step1 (attune |:yield :error| multi))
(def step2 (squelch step1 :yield))
# step2: attune allows only yield+error, then squelch removes yield
# net effect: only error is possible
(try (step2)
  (catch e
    (assert (= (get e :error) :signal-violation))
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
(def failed (try
  (eval '(fn []
    (attune! :yield)
    (println "oops")))
  (catch e e)))
(assert (get failed :error))
(println "attune!-rejects-excess: ok")
