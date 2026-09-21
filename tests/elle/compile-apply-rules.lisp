(elle/epoch 12)
# audited: 2026-09-21
# compile/apply-rules — the rewrite edit engine driven by rules as data.
# docs/analysis/portrait.md carries the worked example; this file pins
# the contract edge by edge.
#
# The counter-factual: template interpolation copies argument source
# text verbatim, so a single pass leaves a renamed symbol inside a
# replaced call's arguments unrenamed. The fixpoint rows catch that.

(def rename-old [{:kind :rename :from "old" :to "new"}])

# ── rename ─────────────────────────────────────────────────────────
(let [r (compile/apply-rules "(old 1) (older 2) (old 3)" rename-old)]
  (assert (= (r :source) "(new 1) (older 2) (new 3)") "exact token match only")
  (assert (= (r :count) 2) "one edit per occurrence")
  (assert (empty? (->list (r :reports))) "no report rules, no reports"))

(let [r (compile/apply-rules "# keep old comments\n(old)" rename-old)]
  (assert (= (r :source) "# keep old comments\n(new)") "comments are not code"))

(let [r (compile/apply-rules "(f 'old)" rename-old)]
  (assert (= (r :source) "(f 'new)") "a quoted symbol is still a token"))

# ── replace ────────────────────────────────────────────────────────
(def swap
  [{:kind :replace :name "m:bump" :arity 2 :template "(m:increment $2 $1)"}])

(let [r (compile/apply-rules "(m:bump a (g b))" swap)]
  (assert (= (r :source) "(m:increment (g b) a)")
          "arguments interpolate as source text"))
(assert (= ((compile/apply-rules "(m:bump a)" swap) :source) "(m:bump a)")
        "a different arity is left alone")
(assert (= ((compile/apply-rules "(m:bump (m:bump a b) c)" swap) :source)
           "(m:increment c (m:increment b a))") "nested calls converge")

# ── rename inside a replaced call's arguments ──────────────────────
(let [rules [{:kind :rename :from "old" :to "new"}
             {:kind :replace :name "wrap" :arity 1 :template "(w $1)"}]
      r (compile/apply-rules "(wrap old)" rules)]
  (assert (= (r :source) "(w new)") "the fixpoint renames inside the span"))

# ── report ─────────────────────────────────────────────────────────
(let [rules [{:kind :report :name "gone" :message "use new"}
             {:kind :rename :from "old" :to "new"}]
      r (compile/apply-rules "(gone 1)\n(old)\n(gone 2)" rules)]
  (assert (= (r :source) "(gone 1)\n(new)\n(gone 2)")
          "a report rule edits nothing")
  (let [reps (->list (r :reports))]
    (assert (= (length reps) 2) "one report per occurrence, one pass")
    (assert (= ((first reps) :name) "gone") "the symbol")
    (assert (= ((first reps) :line) 1) "the first occurrence's line")
    (assert (= ((first reps) :message) "use new") "the shipped message")
    (assert (= ((first (rest reps)) :line) 3) "the second occurrence's line")))

# ── edges ──────────────────────────────────────────────────────────
(let [r (compile/apply-rules "(f 1)" [])]
  (assert (= (r :source) "(f 1)") "no rules, no change")
  (assert (= (r :count) 0) "and no edits"))
(assert (not (first (protect (compile/apply-rules "(f)" [{:kind :frob}]))))
        "an unknown rule kind refuses")
(assert (not (first (protect (compile/apply-rules "\"unterminated" rename-old))))
        "unlexable source refuses")

(println "compile-apply-rules: all tests passed")
