(elle/epoch 14)
# audited: 2026-10-06
# The two helpers the dissolution documents' examples read a fused call's HIR
# through.
#
# docs/impl/dissolution.md
#
# Each document is a program of its own under `make doctest`, and imports this
# file by a path relative to the document.

(defn fused [expr]
  "The functionalized HIR of a function whose body is expr, as text."
  (get (compile/dumps (string "(defn f [] " expr ") (f)") "<doc>") :fhir))

(defn occurrences [text part]
  "How many times part occurs in text."
  (- (length (string/split text part)) 1))

{:fused fused :occurrences occurrences}
