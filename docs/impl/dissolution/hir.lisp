(elle/epoch 13)
# audited: 2026-09-28
# The two helpers the dissolution documents' examples read a fused call's HIR
# through.
#
# docs/impl/dissolution.md
#
# Each document is a program of its own under `make doctest`, which runs from the
# repository root, so each imports this file by that path.

(defn fused [expr]
  "The functionalized HIR of a function whose body is expr, as text."
  (get (compile/dumps (string "(defn f [] " expr ") (f)") "<doc>") :fhir))

(defn occurrences [text part]
  "How many times part occurs in text."
  (- (length (string/split text part)) 1))

{:fused fused :occurrences occurrences}
