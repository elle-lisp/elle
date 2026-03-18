(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-err assert-err} ((import-file "tests/elle/assert.lisp")))

## === vm/list-primitives returns symbols ===

(assert-eq (type-of (first (vm/list-primitives))) :symbol "list-primitives elements are symbols")

## === vm/primitive-meta accepts symbols ===

(assert-true (struct? (vm/primitive-meta (quote +))) "primitive-meta accepts symbol")

## === vm/primitive-meta still accepts keywords ===

(assert-true (struct? (vm/primitive-meta :+)) "primitive-meta accepts keyword")

## === vm/primitive-meta still accepts strings ===

(assert-true (struct? (vm/primitive-meta "+")) "primitive-meta accepts string")

## === vm/primitive-meta type error on wrong type ===

(assert-err (fn () (vm/primitive-meta 42)) "primitive-meta rejects integer")
