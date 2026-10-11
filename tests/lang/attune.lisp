(elle/epoch 15)
# audited: 2026-10-11
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
(def step2 (squelch :yield step1))
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

# ── A bad spec does not compile ───────────────────────────────────
(def [bad-ok? _]
  (protect (compile/whole-module "(defn f [] (attune! :nosuch) 1)" "<attune>")))
(assert (not bad-ok?) "attune! names no signal")
(println "attune!-bad-spec: ok")

# ── A ceiling built as data ───────────────────────────────────────
# Inside quoted data a set literal becomes the list (set :yield :io), and
# attune! reads it as the set it spells (#1331). The ceiling then admits a
# body that yields and refuses one that raises. Counter-factual: attune!
# took a set only from source text, so the quoted ceiling did not compile.
(def [quoted-ok? _]
  (protect (eval '(fn []
                    (attune! |:yield :io|)
                    (yield 1)))))
(assert quoted-ok? "a quoted ceiling admits a body that fits it")
(def [excess-ok? _]
  (protect (eval '(fn [x]
                    (attune! |:yield :io|)
                    (+ x 1)))))
(assert (not excess-ok?) "and refuses a body that raises")
(println "attune!-quoted-spec: ok")

# ── squelch takes its spec first, as attune does ──────────────────
# Counter-factual: squelch took the closure first, so the two runtime
# boundaries put the one argument they share in different places.
(def [order-ok? order-err] (protect (squelch (fn [] 1) :yield)))
(assert (not order-ok?) "a closure in the spec position is refused")
(assert (= (get order-err :error) :type-error)
        "as an argument of the wrong type")
(def [spec-ok? spec-err] (protect ((squelch :yield (fn [] (yield 1))))))
(assert (= (get spec-err :error) :signal-violation)
        "the spec-first squelch blocks the yield")
(println "squelch-spec-first: ok")
