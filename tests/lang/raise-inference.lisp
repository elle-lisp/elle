(elle/epoch 14)
# audited: 2026-10-04
# Every construct that can raise carries :error in its inferred signal, so a
# function that infers silent cannot raise, and a raise nothing catches
# reports as an ordinary error. Counter-factual: each construct below
# inferred `||`, and an uncaught raise from such a function aborted the
# process as a silence violation before `protect` could answer.

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

# ── A spliced call ───────────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn g [a] a) (defn f [xs] (g ;xs))" :f))
        "a spliced call checks its count at run time")
(assert (= || (bits-of "(defn g [a] a) (defn f [x] (g x))" :f))
        "a plain call to a silent function stays silent")

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

(def [silent-ok? silent-err]
  (protect (eval '(defn g []
                   (silence)
                   (eval '(+ 1 2))))))
(assert (not silent-ok?) "a silent function cannot call eval")
(assert (string/contains? (get silent-err :message) "body may emit {:error}"))

(def [assert-ok? _]
  (protect (eval '(defn g []
                   (silent!)
                   (eval '(+ 1 2))))))
(assert (not assert-ok?) "silent! rejects eval")

# ── A silence bound ──────────────────────────────────────────────────

(assert (= |:error| (bits-of "(defn f [g x] (silence g) (g x))" :f))
        "the entry check may raise")
(assert (= |:error| (bits-of "(defn f [g x] (silence g) x)" :f))
        "the check runs whether or not the parameter is called")

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
  (silence h)
  (h x))
(defn call-aps []
  (def r (aps + 42))
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
(def [native-ok? native-err]
  (protect (let [r (aps length [1 2])]
             r)))
(assert (not native-ok?) "a native that may raise fails the bound")
(assert (= :signal-violation (get native-err :error)))
(assert (string/contains? (get native-err :message)
                          "length may emit {:error} but parameter is restricted to {}"))

(println "all raise-inference tests passed")
