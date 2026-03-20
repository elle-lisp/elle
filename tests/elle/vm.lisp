(elle/epoch 1)

## === vm/list-primitives returns symbols ===

(assert (= (type-of (first (vm/list-primitives))) :symbol) "list-primitives elements are symbols")

## === vm/primitive-meta accepts symbols ===

(assert (struct? (vm/primitive-meta (quote +))) "primitive-meta accepts symbol")

## === vm/primitive-meta still accepts keywords ===

(assert (struct? (vm/primitive-meta :+)) "primitive-meta accepts keyword")

## === vm/primitive-meta still accepts strings ===

(assert (struct? (vm/primitive-meta "+")) "primitive-meta accepts string")

## === vm/primitive-meta type error on wrong type ===

(let (([ok? _] (protect ((fn () (vm/primitive-meta 42)))))) (assert (not ok?) "primitive-meta rejects integer"))
