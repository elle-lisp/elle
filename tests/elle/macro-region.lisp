(elle/epoch 11)
# Macro expansion + FreeRegion interaction tests
#
# These tests verify that FreeRegion instructions in macro transformer
# bytecode do not free objects that are part of the macro's return value.
# The region solver must widen return value allocations past scope regions
# so FreeRegion doesn't corrupt them.

# Basic syntax-rules macro — the template builds a list via quasiquote,
# which expands to calls like (list ...) and (cons ...). The return
# value must survive FreeRegion.
(define-syntax my-when (syntax-rules () [(_ test body) (if test body nil)]))

(assert (= (my-when true 42) 42) "my-when true branch")
(assert (= (my-when false 42) nil) "my-when false branch")

# Multi-element template — more allocations in the return value.
(define-syntax my-unless (syntax-rules () [(_ test body) (if test nil body)]))

(assert (= (my-unless false 99) 99) "my-unless false branch")
(assert (= (my-unless true 99) nil) "my-unless true branch")

# Template with begin — the expanded list (begin ...) must survive.
(define-syntax my-when2
               (syntax-rules ()
                             [(_ test body ...)
                              (if test
                                (begin
                                  body
                                  ...)
                                nil)]))

(assert (= (my-when2 true 1 2 3) 3) "my-when2 multi-body")
(assert (= (my-when2 false 1 2 3) nil) "my-when2 false")

# Nested macro calls — the outer macro's expansion includes the inner
# macro's already-expanded result. Both must survive FreeRegion.
(assert (= (my-when true (my-when true 77)) 77) "nested macro calls")

# Macro producing a let form
(define-syntax my-let1
               (syntax-rules ()
                             [(_ var val body)
                              (let [var val]
                                body)]))

(assert (= (my-let1 x 10 (+ x 5)) 15) "macro producing let")

# Macro producing nested structure
(define-syntax my-swap
               (syntax-rules ()
                             [(_ a b)
                              (let [tmp a]
                                (list b tmp))]))

(assert (= (my-swap 1 2) (list 2 1)) "macro producing list")
