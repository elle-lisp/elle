(elle/epoch 15)
# audited: 2026-10-05
# Every construct that can raise carries :error in its inferred signal, so a
# function that infers silent cannot raise, and a raise nothing catches
# reports as an ordinary error. Counter-factual: each construct below
# inferred `||`, and an uncaught raise from such a function aborted the
# process before `protect` could answer.

(defn bits-of [src name]
  (get (compile/signal (compile/analyze src) name) :bits))

# ── Strict destructuring ─────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn f [x] (def [a b] x) a)" :f)) "def pattern")
(assert (= |:error| (bits-of "(defn f [x] (var [a b] x) a)" :f)) "var pattern")
(assert (= |:error| (bits-of "(defn f [x] (let [[a] x] a))" :f)) "let pattern")
(assert (= |:error| (bits-of "(defn f [x] (letrec [[a] x] a))" :f))
        "letrec pattern")
(assert (= |:error| (bits-of "(defn f [[a b]] a)" :f))
        "required parameter pattern")
(assert (= |:error| (bits-of "(defn f [&keys {:k k}] k)" :f)) "&keys pattern")

# A pattern that binds nil instead of raising adds nothing.
(assert (= || (bits-of "(defn f [&opt [a b]] a)" :f)) "&opt pattern binds nil")
(assert (= || (bits-of "(defn f [&named a] a)" :f)) "&named binds nil")
(assert (= || (bits-of "(defn f [x] (def a x) a)" :f))
        "a plain binding cannot fail")

# ── Qualified access ─────────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn f [m] m:k)" :f)) "qualified access is a get")
(assert (= |:error| (bits-of "(defn f [m] m:a:b)" :f)) "nested qualified access")

# ── A call whose argument count was not checked ──────────────────────

(assert (= |:error| (bits-of "(defn g [a] a) (defn f [xs] (g ;xs))" :f))
        "a spliced call checks its count at run time")
(assert (= || (bits-of "(defn g [a] a) (defn f [x] (g x))" :f))
        "a plain call to a silent function stays silent")

# Counter-factual: a call to a parameter added nothing, so `(call identity)`
# inferred silent and raised :arity-error.
(assert (= |:error| (bits-of "(defn call [f] (f))" :call))
        "a call to a parameter may raise")
(def [param-ok? param-err]
  (protect (eval '(begin
                    (defn call [f]
                      (f))
                    (defn g []
                      (silent!)
                      (call identity))))))
(assert (not param-ok?) "a silent function cannot pass identity to call")

# Counter-factual: q took p's signal and no arity, so a call to it inferred
# silent.
(assert (= |:error|
           (bits-of "(defn p [] 1) (def q (squelch :yield p)) (defn f [] (q))"
                    :f)) "a squelch binding's arity is not known")

# A stdlib function's arity is known, so a call with the right count adds
# nothing and a call with the wrong count does not compile. Counter-factual:
# the stdlib exports carried no arity, and (identity 1 2) raised at run time
# from a function that inferred silent.
(assert (= || (bits-of "(defn f [x] (identity x))" :f))
        "a call to identity with one argument is checked")
(def [arity-ok? _]
  (protect (compile/whole-module "(defn g [] (identity 1 2))" "<arity>")))
(assert (not arity-ok?) "a wrong count to a stdlib function does not compile")

# ── A call to a polymorphic function ─────────────────────────────────
#
# Counter-factual: passing a parameter on to a polymorphic callee recorded
# the parameter and dropped the callee's own bits, so twice inferred `||`
# while call yields.

(assert (= |:yield :error|
           (bits-of "(defn call [f x] (yield x) (f x)) (defn twice [k] (call k 1))"
                    :twice)) "twice keeps what call raises itself")

# ── A collection literal with a splice ───────────────────────────────
#
# Counter-factual: `[1 ;x]` inferred `||`, and `(f 5)` raised :type-error.

(assert (= |:error| (bits-of "(defn f [x] [1 ;x])" :f))
        "an array literal checks its spliced value")
(assert (= |:error| (bits-of "(defn f [x] @[;x 1])" :f)) "so does a mutable one")
(assert (= || (bits-of "(defn f [x] [1 x])" :f))
        "a literal without a splice adds nothing")

# ── A call to a function with a keyword collector ────────────────────

(assert (= |:error| (bits-of "(defn g [&named a] a) (defn f [] (g :a 1))" :f))
        "&named checks the keys at the call")
(assert (= |:error| (bits-of "(defn g [&keys k] k) (defn f [] (g :a 1))" :f))
        "&keys checks the pairs at the call")
(assert (= || (bits-of "(defn g [& r] r) (defn f [] (g 1 2))" :f))
        "a rest collector takes anything")

# ── parameterize ─────────────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn f [p] (parameterize ((p 1)) 2))" :f))
        "parameterize checks that p is a parameter")

# ── eval ─────────────────────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn f [] (eval '(+ 1 2)))" :f))
        "eval raises, and raises nothing else")

# eval holds no park of the code it runs: a yield, an I/O request or a halt
# inside it comes back as an :eval-error.
(def [yielded? yield-err] (protect (eval '(yield 7))))
(assert (not yielded?) "a yield inside eval does not leave it")
(assert (= :eval-error (get yield-err :error)))

(def [silent-ok? _]
  (protect (eval '(defn g []
                   (silent!)
                   (eval '(+ 1 2))))))
(assert (not silent-ok?) "a silent function cannot call eval")

# ── A bound ──────────────────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn f [g x] (bound g ||) (g x))" :f))
        "the entry check may raise")
(assert (= |:error| (bits-of "(defn f [g x] (bound g ||) x)" :f))
        "the check runs whether or not the parameter is called")

# A bound wider than silence contributes its bits to a call of the
# parameter, and the parameter stops passing its signal on. Counter-factual:
# bound took no spec, and a callback that may yield had to stay polymorphic.
(def each-yield
  (compile/signal (compile/analyze "(defn f [g x] (bound g :yield) (g x))") :f))
(assert (= |:yield :error| (get each-yield :bits)) "a call to g adds :yield")
(assert (empty? (get each-yield :propagates)) "and f takes no signal from g")

(defn compiles? [src]
  "Whether src compiles. Nothing in it runs, so a refusal is the compiler's."
  (first (protect (compile/whole-module src "<bound>"))))

# A well-formed bound compiles, so each refusal below is the bound's own.
(assert (compiles? "(defn f [h] (bound h ||) h)") "one bound on a parameter")
(assert (not (compiles? "(bound g ||)")) "a bound outside a function")
(assert (not (compiles? "(defn f [x] (bound z ||) x)"))
        "a bound on a name that is not a parameter")
(assert (not (compiles? "(defn f [h] (bound h ||) (bound h :yield) h)"))
        "a second bound on one parameter")
(assert (not (compiles? "(defn f [h] (bound h :nosuch) h)"))
        "a bound naming no signal")
(assert (not (compiles? "(defn f [h] (bound h) h)")) "a bound with no spec")

# ── Literals that check their elements at run time ───────────────────

(assert (= |:error| (bits-of "(defn f [x] b[x])" :f))
        "a bytes literal checks its range")
(assert (= || (bits-of "(defn f [] b[1 2 255])" :f))
        "literal bytes in range raise nothing")
(assert (= |:error| (bits-of "(defn f [] b[256])" :f))
        "a literal byte out of range raises")
(assert (= |:error| (bits-of "(defn f [k] {k 1})" :f))
        "a computed key may be mutable")
(assert (= || (bits-of "(defn f [v] {:a v 'b v \"c\" v 1 v})" :f))
        "a literal key is immutable")
(assert (= |:error| (bits-of "(defn f [k] @{k 1})" :f))
        "a computed key of a table too")

# ── An uncaught raise is an ordinary error ───────────────────────────
#
# `g` calls `f` in non-tail position, so the call completes inside `g` and the
# boundary of `f` is checked there, under `protect`.

(defn f [x]
  (def [a b] x)
  a)
(defn g []
  (def r (f 5))
  r)
(def [ok? err] (protect (g)))
(assert (not ok?) "the raise reaches protect")
(assert (= :type-error (get err :error)) "as the destructuring error it is")

(defn aps [h x]
  (bound h ||)
  (h x))
(defn call-aps []
  (def r (aps (get @[+] 0) 42))
  r)
(def [bound-ok? bound-err] (protect (call-aps)))
(assert (not bound-ok?) "a bound violation reaches protect")
(assert (= :signal-violation (get bound-err :error)))

(def [eval-ok? eval-err]
  (protect (eval '(begin
                    (defn f [x]
                      (def [a b] x)
                      a)
                    (defn g []
                      (def r (f 5))
                      r)
                    (g)))))
(assert (not eval-ok?) "the raise reaches protect through eval")
(assert (= :eval-error (get eval-err :error)))

# ── A native passed for a bounded parameter ──────────────────────────
#
# The check reads a native's declared signal, as it reads a closure's
# inferred one. Counter-factual: every non-closure passed, so `println`
# raised :io out of a function whose signal held no :io.

(assert (= false (aps callable? 42)) "a silent native passes the bound")
(assert (= false (aps (get @[callable?] 0) 42)) "and passes the entry check")
(def [native-ok? native-err]
  (protect (let [r (aps (get @[length] 0) [1 2])]
             r)))
(assert (not native-ok?) "a native that may raise fails the bound")
(assert (= :signal-violation (get native-err :error)))

# ── A polymorphic closure passed for a bounded parameter ─────────────
#
# A closure that passes on its arguments' signals has bits that are only a
# lower bound, so the check rejects it whatever its bits say. `call-it`
# raises only :error itself, which a bound of :error admits. Counter-factual:
# the check read the bits alone, and call-it passed a bound its argument's
# signal can break.

(defn call-it [k]
  (k))
(defn ape [h x]
  (bound h :error)
  (h x))
(assert (= 1 (ape (get @[first] 0) [1])) "a function that only raises fits")
(def [poly-ok? poly-err]
  (protect (let [r (ape (get @[call-it] 0) 42)]
             r)))
(assert (not poly-ok?) "a polymorphic closure fails the bound")
(assert (= :signal-violation (get poly-err :error)))

# ── A bound checked at the call ──────────────────────────────────────
#
# Where the compiler knows the argument's signal, it checks the bound at the
# call, and a violation does not compile. Counter-factual: every argument
# waited for the entry check, so a program that passes a yielding lambda to
# a silence-bounded parameter compiled and failed when it ran.

(def aps-src "(defn aps [h x] (bound h ||) (h x))\n")
(def each-yield-src "(defn each-yield [h x] (bound h :yield) (h x))\n")

(assert (not (compiles? (append aps-src "(aps (fn [x] (yield x)) 1)")))
        "a lambda literal is checked at the call")
(assert (not (compiles? (append aps-src "(aps + 42)")))
        "a stdlib function's signal is known")
(assert (not (compiles? (append aps-src "(aps length [1 2])")))
        "a primitive's signal is known")
(assert (not (compiles? (append aps-src
                                "(defn loud [x] (yield x))\n(aps loud 1)")))
        "a binding's inferred signal is known")
(assert (not (compiles? (append aps-src
                                "(defn call-it [k] (k))\n(aps call-it 1)")))
        "a polymorphic binding cannot fit the bound")
(assert (not (compiles? (append each-yield-src
                                "(each-yield (fn [x] (port/write (port/stdout) x)) 1)")))
        "an argument that does I/O exceeds a bound of :yield")
(assert (compiles? (append each-yield-src "(each-yield (fn [x] (yield x)) 1)"))
        "an argument that only yields fits it")

# An argument that fits compiles, and so does one whose signal is unknown.
(def fits
  (eval '(begin
           (defn each-yield [h x]
             (bound h :yield)
             (h x))
           (defn opaque-yield []
             (get @[(fn [x] (yield x))] 0))
           [(fiber/new (fn [] (each-yield (fn [x] (yield x)) 1)) |:yield|)
            (fiber/new (fn [] (each-yield (opaque-yield) 2)) |:yield|)])))
(assert (= 1 (fiber/resume (get fits 0))) "a lambda that fits the bound runs")
(assert (= 2 (fiber/resume (get fits 1)))
        "an unknown argument waits for the entry")

(println "all raise-inference tests passed")
