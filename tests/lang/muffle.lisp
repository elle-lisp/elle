(elle/epoch 14)
# audited: 2026-10-04
# (muffle spec) is squelch applied to the function itself when a closure is
# made from it: the inferred signal loses spec and gains :error, and the
# boundary turns a muffled signal into :signal-violation. Counter-factual:
# muffle removed bits from the inferred signal and enforced nothing, so a
# muffled function lied to its callers, and `(silence) (muffle :error)` made
# a function that raises read as silent.

(defn bits-of [src name]
  (get (compile/signal (compile/analyze src) name) :bits))

(defn compile-error [form]
  "The message eval reports when form does not compile."
  (let [[ok? err] (protect (eval form))]
    (assert (not ok?) "the form compiles")
    (get err :message)))

# ── The inferred signal ──────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn f [] (muffle :yield) (yield 1) 2)" :f))
        "a muffled yield comes back as :error")
(assert (= || (bits-of "(defn f [x] (muffle :yield) x)" :f))
        "a muffle of a signal the body never raises changes nothing")
(assert (= |:error|
           (bits-of "(defn f [] (muffle |:yield :io|) (println 1) (yield 1) 2)"
                    :f)) "a set muffles each member")
(assert (= |:io :error|
           (bits-of "(defn f [] (muffle :yield) (println 1) (yield 1) 2)" :f))
        "a signal outside the spec still leaves")

# ── The boundary ─────────────────────────────────────────────────────

(defn muffled []
  (muffle :yield)
  (yield 1)
  2)

(def [ok? err]
  (protect (let [r (muffled)]
             r)))
(assert (not ok?) "a muffled yield does not leave the function")
(assert (= :signal-violation (get err :error)))
(assert (= "squelch: signal {:yield} caught at boundary" (get err :message)))

(def [tail-ok? tail-err] (protect (muffled)))
(assert (not tail-ok?) "the boundary holds in tail position")
(assert (= :signal-violation (get tail-err :error)))

(defn quiet [x]
  (muffle :yield)
  x)
(assert (= 3 (quiet 3)) "a muffle that catches nothing costs nothing")

(defn muffled-io []
  (muffle :io)
  (println "never printed")
  2)
(def [io-ok? io-err]
  (protect (let [r (muffled-io)]
             r)))
(assert (not io-ok?) "a muffled I/O request does not reach the scheduler")
(assert (= :signal-violation (get io-err :error)))

# ── Beside a ceiling ─────────────────────────────────────────────────

(defn bounded []
  (attune! :error)
  (muffle :yield)
  (yield 1)
  2)
(def [bounded-ok? bounded-err]
  (protect (let [r (bounded)]
             r)))
(assert (not bounded-ok?) "attune! :error admits the muffled form")
(assert (= :signal-violation (get bounded-err :error)))

(assert (string/contains? (compile-error '(fn []
                            (silence)
                            (muffle :yield)
                            (yield 1)))
                          "function restricted to {} but body may emit {:error}")
        "silence rejects a muffled body: the violation it raises is a raise")

# ── What cannot be muffled ───────────────────────────────────────────

(assert (string/contains? (compile-error '(fn [x]
                            (muffle :error)
                            (+ x 1)))
                          "{:error} passes every boundary and cannot be muffled")
        ":error cannot be muffled")
(assert (string/contains? (compile-error '(fn [x]
                            (muffle |:yield :halt|)
                            x))
                          "{:halt} passes every boundary and cannot be muffled")
        ":halt cannot be muffled")
(assert (string/contains? (compile-error '(muffle :yield))
                          "muffle must appear inside a function body")
        "muffle outside a function is an error")

(println "all muffle tests passed")
