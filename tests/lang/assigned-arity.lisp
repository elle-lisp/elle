(elle/epoch 15)
# audited: 2026-10-25
# A call through a binding an assign reaches checks its argument count when it runs, wherever the assign stands.
#
# docs/pipeline.md
#
# Counter-factual: the pass checked each call's count against the first lambda
# its callee's binding held. A call shaped for the lambda a later assign stored
# did not compile.

(defn run [form]
  "form compiled and run under eval: [ok? value-or-error]."
  (protect (eval form)))

(defn refusal [form]
  "The message eval reports for form, which must not compile."
  (def [ok? err] (run form))
  (assert (not ok?) "the form must not compile")
  (get err :message))

# ── In a function body ───────────────────────────────────────────────

(assert (= [true 2]
           (run '((fn []
                    (defn f []
                      (h 1))
                    (var h (fn [] 0))
                    (assign h (fn [x] (+ x 1)))
                    (f)))))
        "a forward call finds the lambda the assign stored, with its count")

(assert (= [true 11]
           (run '((fn []
                    (var h (fn [] 0))
                    (defn f []
                      (h 1))
                    (assign h (fn [x] (+ x 10)))
                    (f)))))
        "a call before the assign finds the lambda the assign stored")

# The count is checked when the call runs, against the value it finds. The
# call runs outside eval, which reports any error the code it runs raises as an
# :eval-error.
(def [stale-ok? stale]
  (run '(fn []
          (var h (fn [] 0))
          (defn f []
            (h 1))
          (assign h (fn [] 1))
          (f))))
(assert stale-ok? "a call the stored lambda refuses compiles")
(def [stale-ran? stale-err] (protect (stale)))
(assert (not stale-ran?) "the stored lambda takes no argument")
(assert (= :arity-error (get stale-err :error))
        "the call reports the count when it runs")

# Control: a var that no assign reaches keeps its arity.
(assert (string/contains? (refusal '(fn []
                                      (defn f []
                                        (h 1))
                                      (var h (fn [] 0))
                                      f))
                          "arity error: h expects 0 arguments, got 1")
        "a forward call through an unassigned var checks its count")

# ── In a file ────────────────────────────────────────────────────────

(def file-src
  "(var h (fn [] 0))
(defn f [] (h 1))
(assign h (fn [x] (+ x 1)))
(f)")
(def [file-ok? file-err] (protect (compile/whole-module file-src "<assigned>")))
(assert file-ok? "a file's call before the assign compiles")

(def fixed-src "(def h (fn [] 0))
(defn f [] (h 1))
(f)")
(def [fixed-ok? fixed-err] (protect (compile/whole-module fixed-src "<fixed>")))
(assert (not fixed-ok?) "a file's call through a def checks its count")
(assert (string/contains? (get fixed-err :message)
                          "arity error: h expects 0 arguments, got 1")
        "and names the callee and the counts")
