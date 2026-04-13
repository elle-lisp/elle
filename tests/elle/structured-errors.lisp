# ── Structured error tests ─────────────────────────────────────────────
#
# Tests for the structured error surface: multi-error accumulation,
# undefined variable suggestions, signal mismatch accumulation,
# and lint integration.

# ── Bug 3: undefined vars detected by compile/analyze ─────────────────

(def typo-src "(defn greet [name] (println \"hello,\" nam))\n(greet \"world\")")
(def typo-result (compile/analyze typo-src {:file "typo.lisp"}))
(def typo-diags (compile/diagnostics typo-result))

# The undefined variable should appear in diagnostics
(assert (not (empty? typo-diags)) "undefined var produces diagnostics")

(println "1: undefined var in diagnostics ok")

# ── Gap 6: multiple errors accumulated ────────────────────────────────

(def multi-src "(defn one [x] (prntln x))\n(defn two [x] (pirntln x))\n(defn three [x] (println x))\n(three 5)")
(def multi-result (compile/analyze multi-src {:file "multi.lisp"}))
(def multi-diags (compile/diagnostics multi-result))

# Both prntln and pirntln should appear in diagnostics
(assert (>= (length multi-diags) 2) "multiple undefined vars accumulated")

(println "2: multi-error accumulation ok")

# ── Gap 9: did you mean? ──────────────────────────────────────────────

# prntln is close to println — should get a suggestion
(def first-diag (first multi-diags))
(def first-msg (get first-diag :message))
(assert (string? first-msg) "diagnostic has message")

(println "3: diagnostics have messages ok")

# ── Bug 2: signal mismatch appears in diagnostics ─────────────────────

(def signal-src "(defn yields [] (fiber/emit :yield 42))\n(defn q [] (silence) (yields))")
(def signal-result (compile/analyze signal-src {:file "signal.lisp"}))
(def signal-diags (compile/diagnostics signal-result))

# Signal mismatch should appear in diagnostics, not just as a raised error
(assert (not (empty? signal-diags)) "signal mismatch in diagnostics")

(println "4: signal mismatch in diagnostics ok")

# ── Clean code produces no diagnostics ────────────────────────────────

(def clean-src "(defn add [a b] (+ a b))\n(add 1 2)")
(def clean-result (compile/analyze clean-src {:file "clean.lisp"}))
(def clean-diags (compile/diagnostics clean-result))

# Filter to just errors (not warnings)
(def clean-errors (filter (fn [d] (= (get d :severity) :error)) clean-diags))
(assert (empty? clean-errors) "clean code has no error diagnostics")

(println "5: clean code has no errors ok")

# ──────────────────────────────────────────────────────────────────────

(println "all structured-error tests passed")
