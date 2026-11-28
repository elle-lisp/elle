(elle/epoch 18)
# audited: 2026-11-23
# The lint rules end to end, from source text to the rule name compile/diagnostics reports.
# src/lint/README.md
#
# The rules run in `elle lint`, in the LSP, and here, because all three read
# the same `HirLinter`. Asserting through `compile/diagnostics` pins the whole
# path from source text to a rule name a consumer can filter on.

(defn rule-hits [src rule]
  "Every diagnostic `src` produces under `rule`."
  (filter (fn [d] (= (get d :rule) rule))
          (compile/diagnostics (compile/analyze src))))

(defn names-in [diags]
  "The quoted binding name each diagnostic message carries."
  (map (fn [d] (get (string/split (get d :message) "'") 1)) diags))

# ── W004 unused-binding ─────────────────────────────────────────────────

(def unused (rule-hits "(defn f [] (let [x 1] 2))\n(f)" "unused-binding"))
(assert (= (length unused) 1) "one unused binding in the let")
(assert (= (first (names-in unused)) "x") "the unused binding is x")
(assert (= (get (first unused) :code) "W004") "unused-binding is W004")
(assert (= (get (first unused) :severity) :warning) "unused-binding warns")
(assert (= (get (first unused) :function) "f")
        "the finding is attributed to its enclosing function")

(assert (empty? (rule-hits "(defn f [] (let [x 1] x))\n(f)" "unused-binding"))
        "a binding that is read does not warn")

(assert (empty? (rule-hits "(defn f [] (let [_scratch 1] 2))\n(f)"
                           "unused-binding"))
        "an _-prefixed name is a deliberate throwaway")

# A misspelled reference leaves the definition unused and the use undefined.
# The lint is the half that names the definition.
(def typo (rule-hits "(defn f [] (let [reuslt 1] 2))\n(f)" "unused-binding"))
(assert (= (first (names-in typo)) "reuslt") "the typo'd definition is flagged")

# ── W006 argument-placed-twice ──────────────────────────────────────────

(def twice "(defmacro twice [x] `(begin ,x ,x))\n")

(def placed
  (rule-hits (string twice "(defn f []\n  (twice\n    (println 1)))\n(f)")
             "argument-placed-twice"))
(assert (= (length placed) 1) "one argument placed twice is one finding")
(assert (= (get (first placed) :code) "W006") "argument-placed-twice is W006")
(assert (= (get (first placed) :severity) :warning)
        "argument-placed-twice warns")
(assert (string/contains? (get (first placed) :message) "'twice'")
        "the finding names the macro")
(assert (= (get (first placed) :line) 4) "the finding sits at the argument")

(assert (= 1
           (length (rule-hits (string twice "(twice [1 (println 2)])")
                              "argument-placed-twice")))
        "an array argument is compound")

(each src in ["(def y 1) (twice y)" "(twice 1)" "(twice \"s\")" "(twice :k)"]
  (assert (empty? (rule-hits (string twice src) "argument-placed-twice"))
          "a name or a literal copies freely"))

(assert (empty? (rule-hits "(defmacro show [x] `(begin (quote ,x) ,x))\n(show (println 1))"
                           "argument-placed-twice")) "a quote does not run")

(assert (= 1
           (length (rule-hits "(defmacro hand-on [x] `(begin ,x (when true ,x)))\n(hand-on (println 1))"
                              "argument-placed-twice")))
        "a position inside another macro call counts")

## The prelude places every argument once. The counter-factual: each placed
## its body once for each kind of collection, and if-let its else form once
## for each binding.
(def prelude-uses
  "(defn f [xs s]
     (each x in xs (println x))
     (if-let [a (first xs) b (rest xs)] (println a b) (println :none))
     (when-let [a (first xs) b (rest xs)] (println a b))
     (assert (= (length xs) 2) \"two\")
     (case (length xs) 1 (println :one) (println :other))
     (-> xs (first) (println))
     (some-> s (string/trim) (println))
     (defer (println :done) (println :body))
     (try (println :try) (catch e (println e)))
     (unless (empty? xs) (println :some))
     (repeat 2 (println :again)))
   (f [1 2] \" s \")")
(assert (empty? (rule-hits prelude-uses "argument-placed-twice"))
        "no prelude macro places an argument twice")

# ── A macro's copies of a body raise one finding ────────────────────────
#
# A macro that places a body twice makes an unused `x` two bindings. The
# counter-factual warned twice at one place.

(def copies
  (rule-hits (string twice "(defn f [] (twice (let [x 1] 2)))\n(f)")
             "unused-binding"))
(assert (= 1 (length copies)) "one finding for one binding the source writes")
(assert (= 1
           (length (rule-hits (string twice "(defn f [] (twice (nope 1)))\n(f)")
                              "undefined-variable")))
        "one undefined name in a body a macro copies is one error")

# ── A binding a macro introduced is the macro's ─────────────────────────

(assert (empty? (rule-hits "(defmacro quietly (e) `(let [tmp 1] ,e))\n(defn f [] (quietly 2))\n(f)"
                           "unused-binding"))
        "the user of a macro did not write its tmp")

# ── W005 non-tail-self-recursion ────────────────────────────────────────

(def deep
  (rule-hits "(defn f [n] (if (= n 0) 0 (+ 1 (f (- n 1)))))"
             "non-tail-self-recursion"))
(assert (= (length deep) 1) "the self-call under + is flagged")
(assert (= (get (first deep) :code) "W005") "non-tail-self-recursion is W005")
(assert (= (get (first deep) :function) "f") "the finding names f")

(assert (empty? (rule-hits "(defn f [n acc] (if (= n 0) acc (f (- n 1) (+ acc 1))))"
                           "non-tail-self-recursion"))
        "the accumulator form recurses in tail position")

# ── The tail flag the rule reads is the one callees report ──────────────
#
# Both read `is_tail` off the analyzed tree. Analysis marks it, so neither
# reads a default that would call every call non-tail.

(def a
  (compile/analyze "(defn f [n acc] (if (= n 0) acc (f (- n 1) (+ acc 1))))"))
(def self-calls (filter (fn [c] (= (get c :name) "f")) (compile/callees a :f)))
(assert (= (length self-calls) 1) "f calls itself once")
(assert (get (first self-calls) :tail)
        "the self-call is reported in tail position")

(println "all lint rule tests passed")
