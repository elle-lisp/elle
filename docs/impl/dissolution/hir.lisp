(elle/epoch 15)
# audited: 2026-10-26
# The two helpers the dissolution documents' examples read a fused call's HIR through.
#
# docs/impl/dissolution.md
#
# Each document is a program of its own under `make doctest`, and imports this
# file by a spec relative to the document.

(letrec [fused (fn [expr]
                 "The functionalized HIR of a function whose body is expr, as text."
                 (get (compile/dumps (string "(defn f [] " expr ") (f)") "<doc>")
                      :fhir))
         occurrences (fn [text part]
                       "How many times part occurs in text."
                       (- (length (string/split text part)) 1))]
  {:fused fused :occurrences occurrences})
