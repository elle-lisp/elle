# Tests for splice (;expr) — spreading indexed types into calls and literals

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Basic splice in function calls
# ============================================================================

(assert-eq (+ ;@[1 2 3]) 6 "splice basic call")
(assert-eq (+ 10 ;@[1 2 3]) 16 "splice mixed args")
(assert-eq (+ ;@[1 2] ;@[3 4]) 10 "splice multiple splices")
(assert-eq (+ 1 ;@[2 3] 4 ;@[5 6]) 21 "splice with normal args between")
(assert-eq (+ 1 ;@[] 2) 3 "splice empty array")
(assert-eq (+ ;[1 2 3]) 6 "splice array")

# ============================================================================
# Splice in data constructors
# ============================================================================

(let ([a @[1 ;@[2 3] 4]])
  (assert-eq (length a) 4 "splice in array literal"))

(let ([t [1 ;@[2 3] 4]])
  (assert-eq (length t) 4 "splice in array literal"))

# ============================================================================
# Splice with closures
# ============================================================================

(begin
  (defn add3 [a b c] (+ a b c))
  (def args @[1 2 3])
  (assert-eq (add3 ;args) 6 "splice with closure"))

(letrec ((apply-helper (fn [nums]
            (if (empty? nums) 0
                (+ (first nums) (apply-helper (rest nums))))))
         (sum (fn [& nums] (apply-helper nums))))
  (assert-eq (sum ;@[1 2 3 4 5]) 15 "splice with variadic fn"))

# ============================================================================
# Long form (splice expr)
# ============================================================================

(assert-eq (+ (splice @[1 2 3])) 6 "splice long form")
(assert-eq (+ 10 (splice @[1 2 3])) 16 "splice long form mixed")

# ============================================================================
# Tail call with splice
# ============================================================================

(begin
  (defn f [a b c] (+ a b c))
  (defn g [] (f ;@[1 2 3]))
  (assert-eq (g) 6 "splice tail call"))

(begin
  (defn sum-to [n acc]
    (if (= n 0) acc
        (sum-to ;@[(- n 1) (+ acc n)])))
  (assert-eq (sum-to 100 0) 5050 "splice recursive tail call"))

# ============================================================================
# Arity mismatch with splice (runtime errors)
# ============================================================================

(begin
  (defn f3 [a b c] (+ a b c))
  (let ([result (protect (f3 ;@[1 2]))])
    (assert-eq (get result 0) false "splice too few args errors")))

(begin
  (defn f2 [a b] (+ a b))
  (let ([result (protect (f2 ;@[1 2 3]))])
    (assert-eq (get result 0) false "splice too many args errors")))

# ============================================================================
# Reader tests
# ============================================================================

(assert-eq (+ ;@[1 2]) 3 "semicolon is splice not comment")
(assert-eq (+ 1 2) 3 "hash is comment") # this is a comment

# ============================================================================
# Yield through splice
# ============================================================================

(begin
  (defn yielding-fn [a b c]
    (yield (+ a b c))
    (* a b c))
  (var co (make-coroutine (fn [] (yielding-fn ;@[2 3 4]))))
  (def first-resume (coro/resume co))
  (def second-resume (coro/resume co))
  (assert-eq first-resume 9 "yield through splice: first resume yields 9")
  (assert-eq second-resume 24 "yield through splice: second resume returns 24"))

# ============================================================================
# Splice with list
# ============================================================================

(assert-eq (+ ;(list 1 2 3)) 6 "splice list into arithmetic")
(assert-eq (+ 10 ;(list 1 2 3)) 16 "splice list into arithmetic with leading arg")
(assert-eq (+ ;(list 1 2) ;(list 3 4)) 10 "splice multiple lists")

(let ([result (list 0 ;(list 1 2 3) 4)])
  (assert-eq result (list 0 1 2 3 4) "splice list into list constructor"))

(begin
  (def xs (list 1 2 3))
  (assert-eq (+ ;xs) 6 "splice list variable into call"))

(begin
  (defn add3 [a b c] (+ a b c))
  (assert-eq (add3 ;(list 10 20 30)) 60 "splice list into closure call"))

(assert-eq (+ ;(list)) 0 "splice empty list")

# ============================================================================
# Compile-time error cases
# ============================================================================

(assert-err (fn () (eval '(;@[1 2 3]))) "splice at top level should error")

(assert-err (fn () (eval '(+ ;42))) "splicing an integer should error")

(assert-err (fn () (eval '(;;@[1 2]))) "nested splice should error")
