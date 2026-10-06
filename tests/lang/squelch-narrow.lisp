(elle/epoch 15)
# audited: 2026-10-06
# A binding of squelch over a literal spec takes the narrowed signal, and a spec built by a shadowed set does not.
# docs/signals/inference.md

(defn bits-of [src name]
  (get (compile/signal (compile/analyze src) name) :bits))

(def p-src "(defn p [] (yield 1))\n")

# A literal keyword and a set literal each narrow the binding: the yield goes,
# and the :error the boundary may raise comes in.
(assert (= |:error|
           (bits-of (append p-src
                            "(def safe (squelch :yield p)) (defn f [] (safe))")
                    :f)) "a keyword spec narrows a def")
(assert (= |:error|
           (bits-of (append p-src
                            "(def safe (squelch |:yield| p)) (defn f [] (safe))")
                    :f)) "a set spec narrows a def")
(assert (= |:error|
           (bits-of (append p-src
                            "(defn f [] (let [safe (squelch |:yield| p)] (safe)))")
                    :f)) "a set spec narrows a let")

# A set literal is a call to `set`. Where a binding shadows `set`, that call
# builds whatever the binding builds, so the spec is computed and the binding
# keeps no signal the compiler knows. Counter-factual: the narrowing read any
# call named `set` as a literal, so `f` inferred |:error| while `safe` could
# raise anything the shadowing function's spec let through.
(assert (contains? (bits-of (append p-src
                                    "(defn f [] (let [set (fn [& ks] :io)
                                                      safe (squelch |:yield| p)]
                                                  (safe)))")
                            :f) :yield)
        "a spec built by a let-shadowed set narrows nothing")
(assert (contains? (bits-of (append p-src
                                    "(def set (fn [& ks] :io))
                                     (def safe (squelch |:yield| p))
                                     (defn f [] (safe))")
                            :f) :yield)
        "a spec built by a def-shadowed set narrows nothing")

(println "squelch-narrow: OK")
