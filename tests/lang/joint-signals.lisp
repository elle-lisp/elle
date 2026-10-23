(elle/epoch 15)
# audited: 2026-10-23
# A unit's definitions settle their signals jointly, whatever their order, in a function body as in a file.
#
# docs/pipeline.md
#
# Counter-factual: a body seeded a definition only as it reached it, so a call
# to a later sibling took every bit. A call through a var that a later assign
# replaced read the first lambda's signal.

(defn outer-bits [src]
  "The signal bits of `outer`, defined in src."
  (get (compile/signal (compile/analyze src) :outer) :bits))

(defn compiles? [form]
  "Whether form compiles under eval. A form here returns its lambdas uncalled."
  (first (protect (eval form))))

(defn refusal [form]
  "The message eval reports for form, which must not compile."
  (def [ok? err] (protect (eval form)))
  (assert (not ok?) "the form must not compile")
  (get err :message))

# ── A body reads a later sibling's signal ────────────────────────────

(assert (= (outer-bits "(defn outer [] (defn f [] (g)) (defn g [] (yield 1)) (f))")
           |:yield|) "a call to a later sibling reads its yield")
(assert (= (outer-bits "(defn outer [] (defn g [] (yield 1)) (defn f [] (g)) (f))")
           |:yield|) "a call to an earlier sibling reads its yield")
(assert (= (outer-bits "(defn outer [] (defn f [] (g)) (defn g [] 1) (f))") ||)
        "a call to a later silent sibling is silent")
(assert (= (outer-bits "(defn outer [] (defn f [n] (if n (g false) 0)) (defn g [n] (yield n) (f n)) (f true))")
           |:yield|) "mutual recursion between siblings settles on the yield")
(assert (= (outer-bits "(defn outer [] (let [k 1] (defn f [] (g)) (defn g [] (yield k)) (f)))")
           |:yield|) "a let body reads its later sibling's yield too")

# A ceiling reads the settled signal, so it neither admits a raise nor refuses
# a silent body.
(assert (compiles? '(fn []
                      (defn f []
                        (silent!)
                        (g))
                      (defn g []
                        1)
                      f)) "silent! accepts a call to a later silent sibling")
(assert (compiles? '(fn []
                      (defn f [n]
                        (silent!)
                        (if n (g false) 0))
                      (defn g [n]
                        (if n (f n) 1))
                      f)) "silent! accepts a silent mutual pair")
(assert (string/contains? (refusal '(fn []
                                      (defn f []
                                        (silent!)
                                        (g))
                                      (defn g []
                                        (yield 1))
                                      f)) "body may emit {:yield}")
        "silent! refuses a call to a later yielding sibling, naming the yield")

# The calling convention follows: a forward call that may yield keeps its frame.
(def forward
  (fiber/new (fn []
               ((fn []
                  (defn f []
                    (+ 1 (g)))
                  (defn g []
                    (yield :up)
                    10)
                  (f)))) |:yield|))
(assert (= :up (fiber/resume forward))
        "the later sibling's yield reaches the fiber")
(assert (= 11 (fiber/resume forward)) "the forward call returns into f")

# ── A forward call checks its argument count ─────────────────────────

(assert (string/contains? (refusal '(fn []
                                      (defn f []
                                        (g 1 2))
                                      (defn g [x]
                                        x)
                                      f))
                          "arity error: g expects 1 argument, got 2")
        "a forward call with too many arguments does not compile")
(assert (compiles? '(fn []
                      (defn f []
                        (g 1))
                      (defn g [x &opt y]
                        x)
                      f))
        "a forward call that leaves an optional parameter out compiles")
(assert (compiles? '(fn []
                      (defn f []
                        (g :a 1))
                      (defn g [&keys opts]
                        opts)
                      f))
        "a forward call with keywords to a keyword collector compiles")

# ── A lambda in a form outside every lambda meets its ceiling ────────
#
# The pass analyzes the silent lambda in k before it reaches g. Counter-factual:
# the ceiling was checked during the pass, against g's silent seed, and the
# yield left the silent lambda at run time.

(def hole
  "(def f (fn [] (g)))
(def k (fiber/new (fn [] (silent!) (f)) |:yield|))
(def g (fn [] (yield 1) 2))")
(def [file-ok? file-err] (protect (compile/whole-module hole "<hole>")))
(assert (not file-ok?) "the file's silent lambda reaches a yield")
(assert (string/contains? (get file-err :message) "body may emit {:yield}")
        "the file refusal names the yield alone")
(assert (string/contains? (refusal '(fn []
                                      (def f (fn [] (g)))
                                      (def k
                                        (fiber/new (fn []
                                          (silent!)
                                          (f)) |:yield|))
                                      (def g
                                        (fn []
                                          (yield 1)
                                          2))
                                      k)) "body may emit {:yield}")
        "the same definitions in a body are refused for the yield alone")

# ── A call through a var that an assign reaches ──────────────────────
#
# The pass reads h's first lambda. Counter-factual: f inferred silent, and its
# call to the reassigned h broke the proof at run time.

(assert (string/contains? (refusal '(fn []
                                      (var h (fn [] 1))
                                      (defn f []
                                        (silent!)
                                        (h))
                                      (assign h (fn [] (yield 1)))
                                      f))
                          "function restricted to {} but body may emit")
        "silent! refuses a call through a reassigned var")

(defn through-var []
  (var h (fn [] 1))
  (defn f []
    (h))
  (assign
    h
    (fn []
      (yield :swapped)
      2))
  (f))

(def swapped (fiber/new through-var |:yield|))
(assert (= :swapped (fiber/resume swapped))
        "the call finds the lambda the assign stored")
(assert (= 2 (fiber/resume swapped)) "and returns its value")
