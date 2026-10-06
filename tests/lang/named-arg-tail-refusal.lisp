(elle/epoch 14)
# audited: 2026-10-06
# A tail call refused for an unknown named argument leaves its callee alive for the next call.
# docs/impl/region/owner.md
#
# The callee is a closure a module function returns, fetched from the
# module's export struct, and `protect`'s thunk calls it in tail position.
# The fetch hands the calling frame a reference it must release. A tail
# call that replaces the frame passes that release to the new activation;
# a tail call refused while it binds the callee's `&named` arguments
# replaces nothing, and the frame's own error exit releases the reference.
#
# The counter-factual is a refusal that releases it twice: once at the
# error exit and once more where the activation ends. The module's
# closure is then freed while the export struct still names it, and the
# next call through the struct reads whatever reused its slot. On this
# implementation that is a panic on the second refusal, which a run
# under one fiber does not reliably reach, so each refusal runs on a
# fiber of its own.

(def module
  ((fn []
     (defn serve [listener handler &named tls-config on-error]
       [listener handler])
     {:serve serve})))

(defn refused []
  "Call the module's closure with a named argument it does not take."
  (protect (module:serve 1 2 :bogus 3)))

(println "a refused named argument leaves the callee alive...")

(each i in (range 0 12)
  (let [[ok? err] (ev/join (ev/spawn refused))]
    (assert (not ok?) (string "refusal " i " must fail"))
    (assert (= :argument-error (get err :error))
            (string "refusal " i " must be an argument error, got " (string err)))))

(assert (= [1 2] (module:serve 1 2))
        "the closure must still answer after twelve refusals")

(println "named-arg-tail-refusal: a refused tail call frees nothing it does not own")
