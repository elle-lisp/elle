(elle/epoch 15)
# audited: 2026-11-10
# A call through a merged function signals what every function it may stand for shares.
# docs/signals/inference.md
#
# The counter-factual: a call through a merged binding was a call to an
# unknown callee, so a silent function could not make it, and no arity or
# bound was checked.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<def-merge-call>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

(defn compiles? [src]
  (first (protect (compile/whole-module src "<def-merge-call>"))))

# ── A function body ───────────────────────────────────────────────────

(assert (compiles? "(defn pick [c]
                     (if c (defn f [] 1) (defn f [] 2))
                     (defn read [] (silent!) (f))
                     (read))")
        "a silent function calls a function both branches define")
(assert (compiles? "(defn pick [c]
                     (defn f [] 1)
                     (when c (defn f [] 2))
                     (defn read [] (silent!) (f))
                     (read))")
        "a skipped def merges with the function before it")
(assert (not (compiles? "(defn pick [c]
                          (if c (defn f [] 1) (defn f [] (yield 2)))
                          (defn read [] (silent!) (f))
                          (read))"))
        "a silent function may not call a merge that may yield")
(assert (not (compiles? "(defn pick [c]
                          (when c (defn f [] 1))
                          (defn read [] (silent!) (f))
                          (read))"))
        "a merge that may still be nil is an unknown callee")
(assert (string/contains? (compile-error
                            "(defn pick [c] (if c (defn f [] 1) (defn f [] 2)) (f 1))")
                          "arity error: f expects 0 arguments, got 1")
        "a call through a merge checks the arity its functions share")
(assert (string/contains? (compile-error
                            "(defn pick [c]
                               (if c
                                 (defn f [g] (bound g ||) (g))
                                 (defn f [g] (bound g ||) (g)))
                               (f (fn [] (yield 1))))")
                          "may emit {:yield} but parameter g is bounded to {}")
        "a call through a merge checks the bounds its functions share")
(assert (compiles? "(defn pick [c]
                     (if c (defn f [g] (bound g ||) (g)) (defn f [g] (g)))
                     (f (fn [] (yield 1))))")
        "a bound one function lacks is left to the entry check")

(defn two-arities [c]
  (if c
    (defn f []
      :none)
    (defn f [x]
      x))
  (f :one))
(assert (= (two-arities false) :one) "the function that takes one argument runs")
(assert (not (first (protect (two-arities true))))
        "the other raises at the call, which no compile-time count rejected")

(defn yield-either [c]
  (if c
    (defn either []
      (yield :then)
      :then-done)
    (defn either []
      :else))
  (defn call-either []
    (either))
  (call-either))
(def either-fiber (fiber/new (fn [] (yield-either true)) |:yield|))
(assert (= (fiber/resume either-fiber) :then)
        "the merge's yield reaches the resume")
(assert (= (fiber/resume either-fiber) :then-done)
        "and its caller resumes after it")

# ── The file ──────────────────────────────────────────────────────────
#
# At file level a merge's inputs settle in the file's fixpoint. Here
# `merged-early` reads `merged-later`'s seed before `merged-later` is analyzed,
# and a merge that copied that seed would leave `merged-caller` silent: its
# yield would then break the proof.

(defn merged-early []
  (merged-later)
  :early)
(when true
  (defn merged-early []
    (merged-later)
    :again))
(defn merged-caller []
  (merged-early))
(defn merged-later []
  (yield :later))
(def merged-fiber (fiber/new merged-caller |:yield|))
(assert (= (fiber/resume merged-fiber) :later) "the yield through a file merge")
(assert (= (fiber/resume merged-fiber) :again) "the def that ran answers")

(when true
  (defn when-nested []
    (when-later)
    :nested))
(defn when-caller []
  (when-nested))
(defn when-later []
  (yield :later))
(def when-fiber (fiber/new when-caller |:yield|))
(assert (= (fiber/resume when-fiber) :later)
        "the yield through a merge of a lambda nested in a top-level form")
(assert (= (fiber/resume when-fiber) :nested) "the nested lambda answers")
