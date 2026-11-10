(elle/epoch 16)
# audited: 2026-11-10
# eval and a module load compile a form as a file does, so a %-intrinsic they cannot prove does not compile.
# docs/intrinsics.md
# docs/modules.md
#
# The loaders compile a module's form through eval. The counter-factual: eval
# skipped the operand proofs, so an unproven site compiled, and a load answered
# a function that ran its raw instruction on whatever reached it.

(defn refusal [thunk]
  "The message of the error THUNK raises, or the empty string when it returns."
  (let [[ok? err] (protect (thunk))]
    (if ok? "" (get err :message))))

(assert (string/contains? (refusal (fn [] (eval '(%add "a" 3))))
                          "%add: operand 1 is not a proven number")
        "eval refuses a provably wrong operand")
(assert (string/contains? (refusal (fn [] (eval '(fn [x y] (%add x y)))))
                          "not a proven number")
        "eval refuses an operand nothing proves")
(assert (string/contains? (refusal (fn []
                                     (import/load-source "<m>"
                                       "(fn [x] (%add x 1))")))
                          "not a proven number")
        "a load refuses a module whose form holds an unproven site")

(def guarded
  (import/load-source "<g>"
                      "(fn [x] (when (%not (%int? x)) (error :int)) (%add x 1))"))
(assert (= 3 (guarded 2)) "a load accepts a site a guard proves")
(def wrapped (import/load-source "<w>" "(fn [x] (+ x 1))"))
(assert (= 3.5 (wrapped 2.5)) "and a wrapper where nothing proves the operand")
