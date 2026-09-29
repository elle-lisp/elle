(elle/epoch 13)
# audited: 2026-09-29
# The primitive triples std/rdf/elle emits carry this implementation's JIT-eligibility predicate.
# docs/impl/jit.md
#
# `jit-eligible` says whether this implementation's JIT admits a primitive,
# so it is a fact about this implementation rather than about the language.
# tests/lang/rdf.lisp holds the rest of the module's contract.

(def rdf ((import "std/rdf/elle")))

(def prim-triples (rdf:primitives))
(assert (string/contains? prim-triples "jit-eligible")
        "primitives contain jit-eligible predicate")

(println "rdf-jit-eligible: ok")
