(elle/epoch 18)
# audited: 2026-11-19
# A letrec's bindings settle their signals jointly, whatever their order, in a function body as in a file.
#
# docs/pipeline.md
#
# Counter-factual: the pass seeded a binding only as it reached it, so a call
# to a binding initialized later took every bit. A call through a var that a
# later assign replaced read the first lambda's signal.

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

# ── A binding reads a later binding's signal ─────────────────────────

(assert (= (outer-bits "(defn outer [] (letrec [f (fn [] (g)) g (fn [] (yield 1))] (f)))")
           |:yield|) "a call to a later binding reads its yield")
(assert (= (outer-bits "(defn outer [] (letrec [g (fn [] (yield 1)) f (fn [] (g))] (f)))")
           |:yield|) "a call to an earlier binding reads its yield")
(assert (= (outer-bits "(defn outer [] (letrec [f (fn [] (g)) g (fn [] 1)] (f)))")
           ||) "a call to a later silent binding is silent")
(assert (= (outer-bits "(defn outer [] (letrec [f (fn [n] (if n (g false) 0)) g (fn [n] (yield n) (f n))] (f true)))")
           |:yield|) "mutual recursion between bindings settles on the yield")
(assert (= (outer-bits "(defn outer [] (let [k 1] (defn g [] (yield k)) (defn f [] (g)) (f)))")
           |:yield|) "a def in a let body reads its earlier sibling's yield")
(assert (string/contains? (refusal '(fn []
                                      (defn f []
                                        (g))
                                      (defn g []
                                        (yield 1))
                                      f)) "undefined variable: g")
        "a def cannot call a sibling defined after it")

# A ceiling reads the settled signal, so it neither admits a raise nor refuses
# a silent body.
(assert (compiles? '(fn []
                      (letrec [f (fn []
                                   (silent!)
                                   (g))
                               g (fn [] 1)]
                        f))) "silent! accepts a call to a later silent binding")
(assert (compiles? '(fn []
                      (letrec [f (fn [n]
                                   (silent!)
                                   (if n (g false) 0))
                               g (fn [n] (if n (f n) 1))]
                        f))) "silent! accepts a silent mutual pair")
(assert (string/contains? (refusal '(fn []
                                      (letrec [f (fn []
                                                   (silent!)
                                                   (g))
                                               g (fn [] (yield 1))]
                                        f))) "body may emit {:yield}")
        "silent! refuses a call to a later yielding binding, naming the yield")

# The calling convention follows: a forward call that may yield keeps its frame.
(def forward
  (fiber/new (fn []
               ((fn []
                  (letrec [f (fn [] (+ 1 (g)))
                           g (fn []
                               (yield :up)
                               10)]
                    (f))))) |:yield|))
(assert (= :up (fiber/resume forward))
        "the later binding's yield reaches the fiber")
(assert (= 11 (fiber/resume forward)) "the forward call returns into f")

# ── A forward call checks its argument count ─────────────────────────

(assert (string/contains? (refusal '(fn []
                                      (letrec [f (fn [] (g 1 2))
                                               g (fn [x] x)]
                                        f)))
                          "arity error: g expects 1 argument, got 2")
        "a forward call with too many arguments does not compile")
(assert (compiles? '(fn []
                      (letrec [f (fn [] (g 1))
                               g (fn [x &opt y] x)]
                        f)))
        "a forward call that leaves an optional parameter out compiles")
(assert (compiles? '(fn []
                      (letrec [f (fn [] (g :a 1))
                               g (fn [&keys opts] opts)]
                        f)))
        "a forward call with keywords to a keyword collector compiles")

# ── A lambda in a form outside every lambda meets its ceiling ────────
#
# The pass analyzes the silent lambda in k before it reaches g. Counter-factual:
# the ceiling was checked during the pass, against g's silent seed, and the
# yield left the silent lambda at run time.

(def hole
  "(letrec [f (fn [] (g))
         k (fiber/new (fn [] (silent!) (f)) |:yield|)
         g (fn [] (yield 1) 2)]
  k)")
(def [file-ok? file-err] (protect (compile/whole-module hole "<hole>")))
(assert (not file-ok?) "the file's silent lambda reaches a yield")
(assert (string/contains? (get file-err :message) "body may emit {:yield}")
        "the file refusal names the yield alone")
(assert (string/contains? (refusal '(fn []
                                      (letrec [f (fn [] (g))
                                               k (fiber/new
                                                   (fn []
                                                     (silent!)
                                                     (f)) |:yield|)
                                               g (fn []
                                                   (yield 1)
                                                   2)]
                                        k))) "body may emit {:yield}")
        "the same bindings in a body are refused for the yield alone")

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
