; Macros and Meta-programming in Elle
;
; Macros are compile-time code transformations. The macro body is normal
; Elle code that runs in the VM during expansion. It receives its arguments
; as quoted syntax and must return syntax (typically via quasiquote).

(import-file "./examples/assertions.lisp")

; ========================================
; 1. Basic macro with quasiquote
; ========================================
; The simplest pattern: quasiquote a template, unquote the parameters.

(defmacro double (expr)
  `(* ,expr 2))

(assert-eq (double 21) 42 "double(21) equals 42")
(assert-eq (double 10) 20 "double(10) equals 20")

; ========================================
; 2. Macros receive code, not values
; ========================================
; The argument (+ 1 2) is spliced as code into the template,
; not evaluated first. The expansion is (* (+ 1 2) 2), which
; evaluates to 6.

(assert-eq (double (+ 1 2)) 6 "double((+ 1 2)) expands to (* (+ 1 2) 2)")

; ========================================
; 3. Conditional in macro template
; ========================================
; Macro templates can contain any form, including if/cond.

(defmacro abs-value (expr)
  `(if (< ,expr 0) (- ,expr) ,expr))

(assert-eq (abs-value -42) 42 "abs-value(-42) equals 42")
(assert-eq (abs-value 42) 42 "abs-value(42) equals 42")

; ========================================
; 4. Code generation: macro producing a function
; ========================================

(defmacro make-adder (n)
  `(fn (x) (+ x ,n)))

(define add-10 (make-adder 10))
(define add-20 (make-adder 20))

(assert-eq (add-10 5) 15 "make-adder(10) generates working function")
(assert-eq (add-20 5) 25 "make-adder(20) generates working function")

; ========================================
; 5. Macro composition
; ========================================
; A macro can expand into code that uses another macro.
; quad expands to (square (square x)), then square expands twice.

(defmacro square (expr)
  `(* ,expr ,expr))

(defmacro quad (x)
  `(square (square ,x)))

(assert-eq (square 5) 25 "square(5) equals 25")
(assert-eq (quad 2) 16 "quad(2) = square(square(2)) = 16")

; ========================================
; 6. Macro introspection
; ========================================
; macro? checks whether a name is a defined macro.
; expand-macro expands a quoted macro call one step.

(assert-true (macro? double) "double is a macro")
(assert-false (macro? +) "+ is not a macro")

(define expanded (expand-macro '(double 5)))
(assert-list-eq expanded (list '* 5 2) "expand-macro shows expansion")

; ========================================
; 7. gensym: unique symbol generation
; ========================================
; gensym creates symbols guaranteed to be unique, useful for
; avoiding name collisions in macro-generated code.

(define sym1 (gensym))
(define sym2 (gensym))
(assert-false (eq? sym1 sym2) "gensym symbols are unique")

; With prefix for readability
(define tmp1 (gensym "tmp"))
(define tmp2 (gensym "tmp"))
(assert-false (eq? tmp1 tmp2) "prefixed gensym symbols are unique")

; ========================================
; 8. Quasiquote as data templating
; ========================================
; Quasiquote works outside macros too, for building data structures
; with computed parts.

(define x 42)
(define result `(the answer is ,x))
(assert-eq (length result) 4 "quasiquote builds a 4-element list")
(assert-eq (last result) 42 "unquoted value is spliced in")

; Quasiquote without unquote is like quote
(assert-list-eq `(a b c) (list 'a 'b 'c) "quasiquote without unquote is like quote")

(display "All meta-programming tests passed.")
(newline)
