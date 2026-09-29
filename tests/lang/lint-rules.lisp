(elle/epoch 12)
# ── Lint rules, end to end through compile/diagnostics ──────────────────
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
