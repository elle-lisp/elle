#!/usr/bin/env elle

# Control flow — building an expression evaluator
#
# We build a small calculator that evaluates arithmetic expression trees
# represented as tagged tuples: [:lit 42], [:add a b], [:mul a b], etc.
# Each section introduces a control flow form by using it to build or
# extend the evaluator.
#
# Demonstrates:
#   if              — conditional, optional else, truthiness rules
#   cond            — multi-branch conditional
#   case            — equality dispatch (prelude macro)
#   when / unless   — one-armed conditional execution
#   if-let / when-let — conditional binding
#   while           — loop with mutation, implicit :while block
#   forever / break — infinite loop with early exit
#   block / break   — named scopes with values, early return
#   match           — pattern matching: literals, wildcards, binding,
#                     list/tuple/struct patterns, nested, guards
#   each            — iteration macro (brief — see collections.lisp)
#   -> / ->>        — threading macros

(import-file "./examples/assertions.lisp")


# ========================================
# Expression language
# ========================================
#
# Arithmetic expressions are tagged tuples:
#   [:lit n]     — literal number
#   [:add a b]   — addition
#   [:sub a b]   — subtraction
#   [:mul a b]   — multiplication
#   [:div a b]   — division (with safe-division support)
#   [:neg a]     — negation


# ========================================
# 1. if — is this expression a literal?
# ========================================
#
# (if test then else) — else is optional (returns nil when omitted).
# Only nil and false are falsy; everything else is truthy.

(defn literal? [expr]
  "Check if an expression is a literal number."
  (if (tuple? expr)
    (if (= (get expr 0) :lit)
      true
      false)
    false))

(assert-true (literal? [:lit 42]) "literal?: yes")
(assert-false (literal? [:add [:lit 1] [:lit 2]]) "literal?: compound")
(assert-false (literal? 42) "literal?: bare number is not an expr")
(display "  (literal? [:lit 42]) = ") (print (literal? [:lit 42]))

# Only nil and false are falsy — everything else is truthy
(assert-eq (if true :yes :no) :yes "if: true branch")
(assert-eq (if false :yes :no) :no "if: false branch")
(assert-eq (if nil :yes :no) :no "if: nil is falsy")
(assert-eq (if true :yes) :yes "if: no else, true")
(assert-eq (if false :yes) nil "if: no else, false → nil")

# Truthiness: 0, empty string, empty list are all truthy (unlike C/Python)
(assert-eq (if 0 :yes :no) :yes "if: 0 is truthy")
(assert-eq (if "" :yes :no) :yes "if: empty string is truthy")
(assert-eq (if (list) :yes :no) :yes "if: empty list is truthy")


# ========================================
# 2. cond — classify expression nodes
# ========================================
#
# (cond (test1 body1) (test2 body2) ... (true default))
# Evaluates tests in order, returns the body of the first truthy one.

(defn expr-type [expr]
  "Return a keyword describing an expression's node type."
  (cond
    ((not (tuple? expr)) :invalid)
    ((= (get expr 0) :lit)  :literal)
    ((= (get expr 0) :neg)  :unary)
    ((= (get expr 0) :add)  :binary)
    ((= (get expr 0) :sub)  :binary)
    ((= (get expr 0) :mul)  :binary)
    ((= (get expr 0) :div)  :binary)
    (true                    :unknown)))

(assert-eq (expr-type [:lit 5]) :literal "cond: literal")
(assert-eq (expr-type [:neg [:lit 3]]) :unary "cond: unary")
(assert-eq (expr-type [:add [:lit 1] [:lit 2]]) :binary "cond: binary")
(assert-eq (expr-type 42) :invalid "cond: not a tuple")
(display "  expr-type([:add ...]) = ") (print (expr-type [:add [:lit 1] [:lit 2]]))

# The operator symbol for pretty-printing
(defn op-symbol [op]
  "Return the printable symbol for an operator keyword."
  (cond
    ((= op :add) "+")
    ((= op :sub) "-")
    ((= op :mul) "*")
    ((= op :div) "/")
    ((= op :neg) "neg")
    (true "?")))

(assert-eq (op-symbol :add) "+" "cond dispatch: add")
(assert-eq (op-symbol :unknown) "?" "cond dispatch: default")


# ========================================
# 3. case — dispatch on operator
# ========================================
#
# (case expr val1 body1 val2 body2 ... default)
# Sugar for chained (if (= g val) ...). Flat pairs, optional default.

(defn binary-op [op a b]
  "Apply a binary arithmetic operator to two numbers."
  (case op
    :add (+ a b)
    :sub (- a b)
    :mul (* a b)
    :div (/ a b)
    (error [:unknown-op (string/join (list "unknown: " (string op)) "")])))

(assert-eq (binary-op :add 3 4) 7 "case: add")
(assert-eq (binary-op :sub 10 3) 7 "case: sub")
(assert-eq (binary-op :mul 6 7) 42 "case: mul")
(assert-eq (binary-op :div 15 3) 5 "case: div")
(display "  (binary-op :mul 6 7) = ") (print (binary-op :mul 6 7))


# ========================================
# 4. when / unless — validation warnings
# ========================================
#
# (when test body...) — runs body if truthy, returns nil otherwise.
# (unless test body...) — runs body if falsy.
# Useful for side effects without an else branch.

(var warnings @[])

(defn check-expr [expr]
  "Push warnings about suspicious expressions."
  (when (not (tuple? expr))
    (push warnings :not-a-tuple))
  (when (and (tuple? expr) (= (get expr 0) :div))
    (push warnings :has-division))
  (unless (tuple? expr)
    (push warnings :also-not-a-tuple)))

(check-expr [:add [:lit 1] [:lit 2]])
(assert-eq (length warnings) 0 "when: no warnings for valid add")

(check-expr [:div [:lit 10] [:lit 0]])
(assert-eq (pop warnings) :has-division "when: division warning")

(check-expr 42)
(assert-eq (length warnings) 2 "when: two warnings for bare number")

# when/unless return nil when the test doesn't fire
(assert-eq (when false :never) nil "when: nil on false")
(assert-eq (unless true :never) nil "unless: nil on true")


# ========================================
# 5. if-let / when-let — safe division
# ========================================
#
# (if-let ((x expr)) then else)
# Binds x to expr; if falsy, takes the else branch.

(defn safe-div [a b]
  "Divide a by b, returning nil if b is zero."
  (if (= b 0)
    nil
    (/ a b)))

(defn describe-result [a b]
  "Describe a division result, handling zero divisor."
  (if-let ((q (safe-div a b)))
    (string/join (list (string a) "/" (string b) " = " (string q)) "")
    (string/join (list (string a) "/0 is undefined") "")))

(assert-eq (describe-result 10 2) "10/2 = 5" "if-let: truthy")
(assert-eq (describe-result 10 0) "10/0 is undefined" "if-let: falsy")
(display "  ") (print (describe-result 10 2))
(display "  ") (print (describe-result 10 0))

# when-let: collect safe quotients, skip failures
(var quotients @[])

(defn try-divide [a b]
  "Push quotient if division succeeds."
  (when-let ((q (safe-div a b)))
    (push quotients q)))

(try-divide 20 4)
(try-divide 10 0)
(try-divide 15 3)
(assert-eq (length quotients) 2 "when-let: only truthy pushes")
(assert-eq (get quotients 0) 5 "when-let: first quotient")


# ========================================
# 6. while — evaluate a batch of expressions
# ========================================
#
# (while test body...) — loops while test is truthy.
# while always returns nil. Use block/break to return a value.

# First, a simple evaluator for literals and negation
(defn eval-simple [expr]
  "Evaluate a literal or negation expression."
  (if (= (get expr 0) :lit)
    (get expr 1)
    (if (= (get expr 0) :neg)
      (- 0 (eval-simple (get expr 1)))
      nil)))

# Evaluate a batch of expressions with a while loop
(def batch @[[:lit 10] [:lit 20] [:neg [:lit 5]] [:lit 7]])
(var results @[])
(var i 0)
(while (< i (length batch))
  (push results (eval-simple (get batch i)))
  (set i (+ i 1)))
(assert-eq results @[10 20 -5 7] "while: evaluated batch")
(display "  batch results: ") (print results)

# while always returns nil
(var j 0)
(def while-result
  (while (< j 3)
    (set j (+ j 1))))
(assert-eq while-result nil "while: always returns nil")

# Factorial via while — the classic accumulator pattern
(defn factorial [n]
  "Compute n! iteratively."
  (var acc 1)
  (var k n)
  (while (> k 1)
    (set acc (* acc k))
    (set k (- k 1)))
  acc)

(assert-eq (factorial 0) 1 "factorial: 0")
(assert-eq (factorial 5) 120 "factorial: 5")
(assert-eq (factorial 10) 3628800 "factorial: 10")
(display "  10! = ") (print (factorial 10))

# while has an implicit block named :while — break overrides the nil return
(var k 0)
(def found
  (while (< k 100)
    (set k (+ k 1))
    (when (= k 5)
      (break :while k))))
(assert-eq found 5 "while: break :while returns a value")


# ========================================
# 7. forever / break — simplify until stable
# ========================================
#
# (forever body...) is sugar for (while true body...).
# You MUST use break to exit. The break value is the return value.

# Repeatedly negate double-negations: [:neg [:neg x]] → x
(defn simplify [expr]
  "Remove double-negations from an expression."
  (forever
    (match expr
      ([:neg [:neg inner]]
        (set expr inner))           # strip one layer, loop again
      (_
        (break :while expr)))))     # stable — return it

(assert-eq (simplify [:lit 5]) [:lit 5] "simplify: already simple")
(assert-eq (simplify [:neg [:neg [:lit 5]]]) [:lit 5] "simplify: double neg")
(assert-eq (simplify [:neg [:neg [:neg [:neg [:lit 3]]]]]) [:lit 3]
  "simplify: four negations")
(display "  simplify(--5) = ") (print (simplify [:neg [:neg [:lit 5]]]))

# Bare (break) exits without a value — returns nil
(var x 0)
(def bare-result
  (forever
    (set x (+ x 1))
    (when (= x 3) (break))))
(assert-eq bare-result nil "forever: bare break returns nil")
(assert-eq x 3 "forever: ran 3 times")

# Collatz sequence: count steps to reach 1
(defn collatz-steps [n]
  "Count steps in the Collatz sequence from n to 1."
  (var x n)
  (var steps 0)
  (forever
    (if (= x 1)
      (break :while steps))
    (set x
      (if (= (% x 2) 0)
        (/ x 2)
        (+ (* 3 x) 1)))
    (set steps (+ steps 1))))

(assert-eq (collatz-steps 1) 0 "collatz: 1 → 0 steps")
(assert-eq (collatz-steps 6) 8 "collatz: 6 → 8 steps")
(assert-eq (collatz-steps 27) 111 "collatz: 27 → 111 steps")
(display "  collatz(27) = ") (display (collatz-steps 27)) (print " steps")


# ========================================
# 8. block / break — validate before evaluating
# ========================================
#
# (block :name body...) creates a named scope.
# Without break, returns the last expression's value.
# (break :name value) exits early with value. (break :name) returns nil.
# break is compile-time validated and cannot cross function boundaries.

# Validate an expression tree, returning :ok or an error keyword
(defn validate-expr [expr]
  "Check that an expression tree is well-formed."
  (block :check
    (when (not (tuple? expr))
      (break :check :not-a-tuple))
    (when (empty? expr)
      (break :check :empty-tuple))
    (def tag (get expr 0))
    (when (not (keyword? tag))
      (break :check :bad-tag))
    (cond
      ((= tag :lit)
        (when (not (number? (get expr 1)))
          (break :check :bad-literal)))
      ((= tag :neg)
        (when (not (= (length expr) 2))
          (break :check :wrong-arity)))
      (true
        (when (not (= (length expr) 3))
          (break :check :wrong-arity))))
    :ok))

(assert-eq (validate-expr [:lit 42]) :ok "validate: good literal")
(assert-eq (validate-expr [:add [:lit 1] [:lit 2]]) :ok "validate: good binary")
(assert-eq (validate-expr 42) :not-a-tuple "validate: bare number")
(assert-eq (validate-expr [:lit "nope"]) :bad-literal "validate: bad literal")
(display "  validate([:lit 42]) = ") (print (validate-expr [:lit 42]))
(display "  validate(42) = ") (print (validate-expr 42))

# Block without break returns the last expression
(def no-break
  (block :compute
    (+ 10 20)
    (* 6 7)))
(assert-eq no-break 42 "block: no break → last expr")

# Named break targets a specific block — useful for nesting
(def nested-result
  (block :outer
    (block :inner
      (break :outer :escaped))
    :never-reached))
(assert-eq nested-result :escaped "block: named break targets outer")

# Inner block completes normally, outer continues
(def inner-runs
  (block :outer
    (def inner-val
      (block :inner
        (+ 1 2)))
    (+ inner-val 10)))
(assert-eq inner-runs 13 "block: inner completes, outer continues")

# find-first: block + each for early exit from iteration
(defn find-first [arr pred]
  "Return the first element where (pred elem) is truthy, or nil."
  (block :search
    (each elem in arr
      (when (pred elem)
        (break :search elem)))
    nil))

(assert-eq (find-first @[1 4 9 16 25] (fn [x] (> x 10))) 16
  "find-first: found 16")
(assert-eq (find-first @[1 2 3] (fn [x] (> x 100))) nil
  "find-first: not found")
(display "  find-first(>10) = ")
(print (find-first @[1 4 9 16 25] (fn [x] (> x 10))))


# ========================================
# 9. match — the full evaluator
# ========================================
#
# (match value (pattern body) ...)
#
# Patterns: literals (42, "hi", :kw, true, nil), _ (wildcard),
# x (binding), (a b) (list), (h . t) (cons), [a b] (tuple),
# @[a b] (array), {:k v} (struct), @{:k v} (table),
# (pat when guard body) (guarded arm).

# --- Pattern basics ---
(var m-lit (match 42
  (42 :found)
  (_ :nope)))
(assert-eq m-lit :found "match: literal int")

(var m-bind (match 7
  (x (* x x))))
(assert-eq m-bind 49 "match: variable binding")

(var m-nil (match nil
  (nil :got-nil)
  (_ :other)))
(assert-eq m-nil :got-nil "match: nil pattern")

# --- List patterns ---
(var m-list (match (list 1 2 3)
  ((1 2 3) :exact)
  (_ :no)))
(assert-eq m-list :exact "match: exact list")

(var m-cons (match (list 1 2 3)
  ((h . t) h)))
(assert-eq m-cons 1 "match: cons head")

# --- Tuple, struct, guard ---
(var m-tup (match [10 20]
  ([a b] (+ a b))))
(assert-eq m-tup 30 "match: tuple binding")

(var m-struct (match {:x 1 :y 2}
  ({:x x :y y} (+ x y))))
(assert-eq m-struct 3 "match: struct binding")

(defn abs-val [n]
  "Absolute value via guarded match."
  (match n
    (x when (< x 0) (- 0 x))
    (x x)))

(assert-eq (abs-val -7) 7 "match guard: negative")
(assert-eq (abs-val 0) 0 "match guard: zero")

# --- The full recursive evaluator ---
#
# eval-expr is NOT tail-recursive — arithmetic wraps the recursive calls,
# so the stack grows with expression depth. For flat data this is fine.

(defn eval-expr [expr]
  "Evaluate an arithmetic expression tree."
  (match expr
    ([:lit n]   n)
    ([:neg a]   (- 0 (eval-expr a)))
    ([:add a b] (+ (eval-expr a) (eval-expr b)))
    ([:sub a b] (- (eval-expr a) (eval-expr b)))
    ([:mul a b] (* (eval-expr a) (eval-expr b)))
    ([:div a b]
      (let* ([divisor (eval-expr b)]
             [dividend (eval-expr a)])
        (if (= divisor 0)
          (error [:division-by-zero "division by zero in expression"])
          (/ dividend divisor))))))

# Simple expressions
(assert-eq (eval-expr [:lit 42]) 42 "eval: literal")
(assert-eq (eval-expr [:neg [:lit 7]]) -7 "eval: negation")
(assert-eq (eval-expr [:add [:lit 3] [:lit 4]]) 7 "eval: addition")

# Nested: (3 + 4) * (10 - 3) = 49
(def complex-expr
  [:mul
    [:add [:lit 3] [:lit 4]]
    [:sub [:lit 10] [:lit 3]]])
(assert-eq (eval-expr complex-expr) 49 "eval: (3+4)*(10-3)")
(display "  (3+4)*(10-3) = ") (print (eval-expr complex-expr))

# Deeper: -(2 * (5 + 3)) = -16
(def deep-expr
  [:neg
    [:mul [:lit 2]
          [:add [:lit 5] [:lit 3]]]])
(assert-eq (eval-expr deep-expr) -16 "eval: -(2*(5+3))")
(display "  -(2*(5+3)) = ") (print (eval-expr deep-expr))

# Simplify + eval: double-neg removal feeds into the evaluator
(def tricky [:neg [:neg [:mul [:lit 3] [:lit 4]]]])
(display "  simplify then eval: ")
(display (eval-expr (simplify tricky))) (print "")
(assert-eq (eval-expr (simplify tricky)) 12 "simplify → eval")

# --- Nested and rest patterns in match ---
(var m-nested
  (match (list (list 1 2) (list 3 4))
    (((a b) (c d))
      (+ a (* b (+ c d))))))
(assert-eq m-nested 15 "match: nested list")

(var m-rest
  (match (list 1 2 3 4 5)
    ((a b & rest)
      (+ a b (length rest)))))
(assert-eq m-rest 6 "match: rest pattern")

# Struct dispatch — tagged shapes
(var m-dispatch
  (match {:type :circle :r 5}
    ({:type :square :s s} (* s s))
    ({:type :circle :r r} (* r r))
    (_ 0)))
(assert-eq m-dispatch 25 "match: struct tag dispatch")


# ========================================
# 10. each — evaluate a list of expressions
# ========================================
#
# (each var in collection body...)
# See collections.lisp for full coverage.

(def program
  (list
    [:lit 10]
    [:add [:lit 3] [:lit 4]]
    [:mul [:lit 6] [:lit 7]]
    [:neg [:lit 5]]
    [:div [:lit 15] [:lit 3]]))

(var outputs @[])
(each expr in program
  (push outputs (eval-expr expr)))

(assert-eq outputs @[10 7 42 -5 5] "each: evaluated program")
(display "  program outputs: ") (print outputs)

# each over a tuple
(var tuple-sum 0)
(each x in [10 20 30]
  (set tuple-sum (+ tuple-sum x)))
(assert-eq tuple-sum 60 "each: tuple sum")


# ========================================
# 11. -> / ->> — pretty-print an expression
# ========================================
#
# (-> val (f a) (g b))  = (g (f val a) b)   — thread-first
# (->> val (f a) (g b)) = (g b (f a val))   — thread-last

(defn format-expr [expr]
  "Pretty-print an expression tree as a string."
  (match expr
    ([:lit n]   (string n))
    ([:neg a]   (-> "(- " (append (format-expr a)) (append ")")))
    ([:add a b] (-> "(" (append (format-expr a)) (append " + ")
                        (append (format-expr b)) (append ")")))
    ([:sub a b] (-> "(" (append (format-expr a)) (append " - ")
                        (append (format-expr b)) (append ")")))
    ([:mul a b] (-> "(" (append (format-expr a)) (append " * ")
                        (append (format-expr b)) (append ")")))
    ([:div a b] (-> "(" (append (format-expr a)) (append " / ")
                        (append (format-expr b)) (append ")")))))

(assert-eq (format-expr [:lit 42]) "42" "format: literal")
(assert-eq (format-expr [:add [:lit 3] [:lit 4]]) "(3 + 4)" "format: add")
(display "  format complex: ") (print (format-expr complex-expr))
(display "  format deep:    ") (print (format-expr deep-expr))

# Thread-first for nested access
(def config {:db {:host "localhost" :port 5432}})
(assert-eq (-> config (get :db) (get :port)) 5432 "->: nested struct access")

# Thread-last for pipeline
(assert-eq (->> "  hello  " string/trim string/upcase) "HELLO"
  "->>: trim then upcase")
(display "  (->> \"  hello  \" trim upcase) = ")
(print (->> "  hello  " string/trim string/upcase))

# Bare symbol threading: (-> x f g) = (g (f x))
(assert-eq (-> -7 abs-val string) "7" "->: bare symbol threading")

# The full pipeline: format, eval, display
(display "  ") (display (format-expr complex-expr))
(display " = ") (print (eval-expr complex-expr))


(print "")
(print "all control flow passed.")
