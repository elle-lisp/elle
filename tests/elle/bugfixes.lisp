## Bug Regression Tests
##
## Migrated from tests/property/bugfixes.rs
## These bugs were structural, not data-dependent. Representative examples suffice.
##
## Covers:
## - StoreCapture stack mismatch (let bindings inside lambdas)
## - defn shorthand equivalence
## - List display (no `. ()` terminator)
## - or expression return value corruption in recursive calls

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Bug 1: StoreCapture stack mismatch (let bindings inside lambdas)
# ============================================================================

# let binding inside lambda preserves value
(begin
  (def f (fn (x) (let ((y x)) y)))
  (assert-eq (f 42) 42 "let binding preserves positive value")
  (assert-eq (f -7) -7 "let binding preserves negative value"))

# let binding with arithmetic
(begin
  (def f (fn (a b) (let ((x a) (y b)) (+ x y))))
  (assert-eq (f 10 -3) 7 "let binding with arithmetic"))

# recursive function with let inside
(begin
  (def f (fn (x)
    (if (= x 0)
        (list)
        (let ((y x))
          (cons y (f (- x 1)))))))
  (assert-eq (length (f 5)) 5 "recursive function with let inside"))

# append inside let inside lambda
(begin
  (def f (fn (x)
    (if (= x 0)
        (list)
        (let ((y x))
          (append (list y) (f (- x 1)))))))
  (assert-eq (length (f 5)) 5 "append inside let inside lambda"))

# multiple let bindings
(begin
  (def f (fn (a b c)
    (let ((x a) (y b) (z c))
      (+ x (+ y z)))))
  (assert-eq (f 1 2 3) 6 "multiple let bindings"))

# nested let bindings
(begin
  (def f (fn (a b)
    (let ((x a))
      (let ((y b))
        (+ x y)))))
  (assert-eq (f 10 20) 30 "nested let bindings"))

# let with computation
(begin
  (def f (fn (x)
    (let ((y (* x 2)) (z (+ x 1)))
      (+ y z))))
  (assert-eq (f 5) 16 "let with computation (y=10, z=6, result=16)"))

# ============================================================================
# Bug 2: defn shorthand equivalence
# ============================================================================

# defn ≡ def+fn
(begin
  (defn f (x) (+ x 1))
  (assert-eq (f 41) 42 "defn shorthand"))

# defn multi-param
(begin
  (defn add (a b) (+ a b))
  (assert-eq (add 10 -3) 7 "defn multi-param"))

# defn recursive (factorial)
(begin
  (defn fact (n)
    (if (= n 0)
        1
        (* n (fact (- n 1)))))
  (assert-eq (fact 10) 3628800 "defn recursive factorial"))

# defn with let body
(begin
  (defn double (x)
    (let ((y x))
      (+ y y)))
  (assert-eq (double 21) 42 "defn with let body"))

# ============================================================================
# Bug 3: List display (no `. ()` terminator)
# ============================================================================

# list display no dot terminator
(begin
  (var list-str (string (list 1 2 3)))
  (assert-false (string/contains? list-str ". ()") "list display no dot terminator"))

# cons chain display
(begin
  (var cons-str (string (cons 1 (cons 2 (cons 3 (list))))))
  (assert-false (string/contains? cons-str ". ()") "cons chain display"))

# list length matches
(begin
  (assert-eq (length (list 1 2 3 4 5)) 5 "list length 5")
  (assert-eq (length (list)) 0 "empty list length"))

# nested list display
(begin
  (var nested-str (string (list (list 1) (list 2))))
  (assert-false (string/contains? nested-str ". ()") "nested list display"))

# append result display
(begin
  (var append-str (string (append (list 1 2) (list 3 4))))
  (assert-false (string/contains? append-str ". ()") "append result display"))

# ============================================================================
# Bug 4: or expression corrupts return value in recursive calls
# ============================================================================

# or expression in recursive predicate
(begin
  (var check
    (fn (x remaining)
      (if (empty? remaining)
          true
          (if (or (= x 1) (= x 2))
              false
              (check x (rest remaining))))))
  (var foo
    (fn (n seen)
      (if (= n 0)
          (list)
          (if (check n seen)
              (append (list n) (foo (- n 1) (cons n seen)))
              (foo (- n 1) seen)))))
  (assert-eq (length (foo 5 (list 0))) 3 "or in recursive predicate (n=5,4,3 safe)"))

# ============================================================================
# Combined: shorthand + let + list display
# ============================================================================

# defn + let + list display
(begin
  (defn make-list (x)
    (if (= x 0)
        (list)
        (let ((y x))
          (cons y (make-list (- x 1))))))
  (var result-str (string (make-list 5)))
  (assert-false (string/contains? result-str ". ()") "defn + let + list display"))

# defn + recursive + list display
(begin
  (defn build (n)
    (if (= n 0)
        (list)
        (let ((rest-list (build (- n 1))))
          (cons n rest-list))))
  (assert-eq (length (build 10)) 10 "defn + recursive + list display"))
