(elle/epoch 16)
# audited: 2026-11-16
# A halt crosses a squelch or attune boundary, so neither a binding's narrowed signal nor a bound's entry check drops :halt.
# docs/signals/squelch.md

(defn bits-of [src name]
  (get (compile/signal (compile/analyze src) name) :bits))

(def quit-src "(defn quit [] (sys/exit 0))\n")

# Counter-factual: narrowing removed the :halt a mask names, so a binding of
# the boundary claimed that it could not halt.
(assert (contains? (bits-of (append quit-src
                                    "(def calm (squelch :halt quit)) (defn f [] (calm))")
                            :f) :halt) "a squelch of :halt keeps :halt")
(assert (contains? (bits-of (append quit-src
                                    "(def calm (attune :ambient quit)) (defn f [] (calm))")
                            :f) :halt)
        "an attune whose spec omits :halt keeps :halt")

# A ceiling that omits :halt refuses a call through such a binding.
(def [ceiled? _]
  (protect (eval '(begin
                    (defn quit []
                      (sys/exit 0))
                    (def calm (squelch :halt quit))
                    (fn []
                      (attune! |:error :ambient|)
                      (calm))))))
(assert (not ceiled?) "the ceiling sees the halt")

# A bound's entry check reads the closure's effective signal, which keeps
# :halt. halter halts only when asked to, so the counter-factual fails an
# assertion rather than ending the process.
(defn apply-quiet [f]
  (bound f |:error :ambient|)
  (f false))
(def halter
  (squelch :halt (fn [stop]
                   (when stop (sys/exit 1))
                   :ran)))
(def [ran? err] (protect (apply-quiet (get @[halter] 0))))
(assert (not ran?) "the entry check refuses a closure that may halt")
(assert (= :signal-violation (get err :error)))

(println "squelch-halt: OK")
