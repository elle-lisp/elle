(elle/epoch 13)
# audited: 2026-09-29
# Where this implementation's name hash orders :a against :b.
# docs/impl/symbol.md
#
# Keywords order by the hash of their names, so the order is total and the
# same in every run, but carries no alphabetical meaning. Another
# implementation may order these two the other way and be just as correct;
# tests/lang/comparison.lisp holds the claims that every order meets.

(assert (= (compare :a :b) -1) "compare: keyword less")

(println "keyword-order: ok")
