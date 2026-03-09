(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## fn/cfg integration tests - Part 2
## Tests the control flow graph visualization (Mermaid format)

## ── Mermaid format tests ────────────────────────────────────────────

(def r9 (fn/cfg (fn (f) f)))
(assert-true (string/starts-with? r9 "flowchart") "fn/cfg default is mermaid")

(def r10 (fn/cfg (fn (g) g) :mermaid))
(assert-true (string/starts-with? r10 "flowchart") "fn/cfg mermaid explicit")

(def r11 (fn/cfg (fn (h) h) :mermaid))
(assert-true (string/contains? r11 "block0") "fn/cfg mermaid contains block")

(def r12 (fn/cfg (fn (i) (if i 1 2)) :mermaid))
(assert-true (string/contains? r12 "-->") "fn/cfg mermaid branching has edges")

(def f (fn (j) (if j 1 2)))
(def r13a (fn/cfg f))
(def r13b (fn/cfg f :mermaid))
(assert-eq r13a r13b "fn/cfg mermaid default equals explicit")
