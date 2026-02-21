;; Debugging Toolkit Examples
;; Demonstrates introspection, time, and benchmarking primitives.

(import-file "./examples/assertions.lisp")

(display "=== Debugging Toolkit ===\n\n")

;; --- Introspection ---

(display "--- Introspection ---\n")

(define (add x y) (+ x y))
(define (identity x) x)

;; closure? predicate
(assert-true (closure? add) "add is a closure")
(assert-true (closure? identity) "identity is a closure")
(assert-false (closure? 42) "42 is not a closure")
(assert-false (closure? +) "+ is a native fn, not a closure")
(display "  ✓ closure?\n")

;; pure? predicate
(assert-true (pure? add) "add is pure")
(assert-true (pure? identity) "identity is pure")
(display "  ✓ pure?\n")

;; arity
(assert-eq (arity add) 2 "add has arity 2")
(assert-eq (arity identity) 1 "identity has arity 1")
(assert-eq (arity 42) nil "non-closure has nil arity")
(display "  ✓ arity\n")

;; captures
(define x 10)
(define (make-adder n) (lambda (x) (+ x n)))
(define add5 (make-adder 5))
(assert-eq (captures add) 0 "add captures nothing")
(assert-eq (captures add5) 1 "add5 captures one variable")
(display "  ✓ captures\n")

;; bytecode-size
(assert-true (> (bytecode-size add) 0) "add has bytecode")
(assert-eq (bytecode-size 42) nil "non-closure has nil bytecode-size")
(display "  ✓ bytecode-size\n")

;; disbit - bytecode disassembly
(define disasm-result (disbit add))
(assert-true (> (length disasm-result) 0) "disbit returns non-empty vector")
(assert-true (string? (vector-ref disasm-result 0)) "disbit elements are strings")
(display "  ✓ disbit\n")

;; disjit - Cranelift IR (may be nil if no LIR stored)
(define jit-result (disjit add))
;; disjit returns nil or a vector of strings
(assert-true (or (nil? jit-result)
                 (and (> (length jit-result) 0)
                      (string? (vector-ref jit-result 0))))
             "disjit returns nil or vector of strings")
(display "  ✓ disjit\n")

(display "\n=== All debugging toolkit tests passed ===\n")
