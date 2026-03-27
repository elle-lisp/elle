#!/usr/bin/env elle

# Meta-programming — macros, quasiquote, hygiene, and introspection
#
# Demonstrates:
#   defmacro, quasiquote   — compile-time code transformation
#   Macro composition      — macros expanding into other macros
#   expand-macro, macro?   — introspection tools
#   gensym                 — unique symbol generation
#   Quasiquote as data     — building data structures with computed parts
#   Macro hygiene          — automatic capture avoidance
#   datum->syntax          — hygiene escape hatch (anaphoric macros)
#   syntax->datum          — stripping scope information
## ── Basic macro with quasiquote ────────────────────────────────────

# The simplest pattern: quasiquote a template, unquote the parameters.

(defmacro double (expr)
  `(* ,expr 2))

(assert (= (double 21) 42) "double(21) equals 42")
(assert (= (double 10) 20) "double(10) equals 20")
(println "  (double 21) => " (double 21))
## ── Macros receive code, not values ────────────────────────────────

# The argument (+ 1 2) is spliced as code into the template,
# not evaluated first. The expansion is (* (+ 1 2) 2), which
# evaluates to 6.

(assert (= (double (+ 1 2)) 6) "double((+ 1 2)) expands to (* (+ 1 2) 2)")
(println "  (double (+ 1 2)) => " (double (+ 1 2)))
## ── Conditional in macro template ──────────────────────────────────

# Macro templates can contain any form, including if/cond.

(defmacro abs-value (expr)
  `(if (< ,expr 0) (- ,expr) ,expr))

(assert (= (abs-value -42) 42) "abs-value(-42) equals 42")
(assert (= (abs-value 42) 42) "abs-value(42) equals 42")
(println "  (abs-value -42) => " (abs-value -42))
## ── Code generation: macro producing a function ────────────────────

(defmacro make-adder (n)
  `(fn [x] (+ x ,n)))

(var add-10 (make-adder 10))
(var add-20 (make-adder 20))

(assert (= (add-10 5) 15) "make-adder(10) generates working function")
(assert (= (add-20 5) 25) "make-adder(20) generates working function")
(println "  (add-10 5) => " (add-10 5))
(println "  (add-20 5) => " (add-20 5))
## ── Macro composition ──────────────────────────────────────────────

# A macro can expand into code that uses another macro.
# quad expands to (square (square x)), then square expands twice.

(defmacro square (expr)
  `(* ,expr ,expr))

(defmacro quad (x)
  `(square (square ,x)))

(assert (= (square 5) 25) "square(5) equals 25")
(assert (= (quad 2) 16) "quad(2) = square(square(2)) = 16")
(println "  (quad 2) => " (quad 2))
## ── Macro introspection ────────────────────────────────────────────

# macro? checks whether a name is a defined macro.
# expand-macro expands a quoted macro call one step.

(assert (macro? double) "double is a macro")
(assert (not (macro? +)) "+ is not a macro")

(var expanded (expand-macro '(double 5)))
(assert (= expanded (list '* 5 2)) "expand-macro shows expansion")
(println "  (expand-macro '(double 5)) => " expanded)
## ── gensym: unique symbol generation ───────────────────────────────

# gensym creates symbols guaranteed to be unique, useful for
# avoiding name collisions in macro-generated code.

(var sym1 (gensym))
(var sym2 (gensym))
(assert (not (identical? sym1 sym2)) "gensym symbols are unique")
(println "  gensym => " sym1)

# With prefix for readability
(var tmp1 (gensym "tmp"))
(var tmp2 (gensym "tmp"))
(assert (not (identical? tmp1 tmp2)) "prefixed gensym symbols are unique")
(println "  (gensym \"tmp\") => " tmp1)
## ── Quasiquote as data templating ──────────────────────────────────

# Quasiquote works outside macros too, for building data structures
# with computed parts.

(var x 42)
(var result `(the answer is ,x))
(assert (= (length result) 4) "quasiquote builds a 4-element list")
(assert (= (last result) 42) "unquoted value is spliced in")
(println "  `(the answer is ,x) => " result)

# Quasiquote without unquote is like quote
(assert (= `(a b c) (list 'a 'b 'c)) "quasiquote without unquote is like quote")
## ── Macro hygiene: no accidental capture ───────────────────────────

# The swap macro introduces a `tmp` binding internally. If the caller
# also has a variable named `tmp`, the macro's `tmp` must not shadow it.
# This is automatic — no gensym needed.

(defmacro my-swap (a b)
  `(let ((tmp ,a)) (assign ,a ,b) (assign ,b tmp)))

(let ([tmp 100] [x 1] [y 2])
  (my-swap x y)
  (assert (= tmp 100) "swap: caller's tmp is not captured")
  (assert (= x 2) "swap: x is now 2")
  (assert (= y 1) "swap: y is now 1"))

(println "  swap hygiene: caller's tmp preserved after swap")
## ── Hygiene with nested macros ─────────────────────────────────────

# Two macros both introduce `tmp`. They don't interfere with each
# other or with the caller.

(defmacro add-one (x)
  `(let ((tmp ,x)) (+ tmp 1)))

(defmacro add-two (x)
  `(let ((tmp ,x)) (+ tmp 2)))

(assert (= (+ (add-one 10) (add-two 20)) 33) "nested macros with same-named tmp don't interfere")
(println "  (+ (add-one 10) (add-two 20)) => " (+ (add-one 10) (add-two 20)))
## ── gensym in macro templates ──────────────────────────────────────

# gensym creates unique symbols at macro expansion time. Useful when
# you need a temporary binding that won't collide with anything.

(defmacro with-temp (val body)
  (let ([g (gensym "tmp")])
    `(let ((,g ,val)) ,body)))

(with-temp 42 (assert true "gensym macro expanded without error"))

# Two expansions get different gensyms, so they don't collide.
(with-temp 1
  (with-temp 2
    (assert true "nested gensym macros don't collide")))
## ── datum->syntax: anaphoric macros ────────────────────────────────

# datum->syntax is the hygiene escape hatch. It creates a binding
# that IS visible at the call site — the opposite of normal hygiene.
# This enables "anaphoric" macros that intentionally introduce names.

# aif: anaphoric if — binds the test result to `it`
(defmacro aif (test then else)
  `(let ((,(datum->syntax test 'it) ,test))
     (if ,(datum->syntax test 'it) ,then ,else)))

(assert (= (aif 42 it 0) 42) "aif: `it` is bound to the test value")

(assert (= (aif false 42 0) 0) "aif: false test takes else branch")

(assert (= (aif (+ 1 2) (+ it 10) 0) 13) "aif: `it` works with compound test expressions")

(println "  (aif (+ 1 2) (+ it 10) 0) => " (aif (+ 1 2) (+ it 10) 0))
## ── datum->syntax with existing bindings ───────────────────────────

# The macro-introduced `it` correctly shadows an outer `it` because
# the let binding is closer in scope.

(let ([it 999])
  (assert (= (aif 42 it 0) 42) "aif: macro's `it` shadows outer `it` inside then-branch"))
## ── syntax->datum: stripping scopes ────────────────────────────────

# syntax->datum converts a syntax object back to a plain value,
# stripping all scope information. Useful for inspecting macro
# arguments as data.

(assert (= (syntax->datum 42) 42) "syntax->datum: plain values pass through unchanged")
(println "  (syntax->datum 42) => " (syntax->datum 42))


(println "")
(println "all meta-programming passed.")
