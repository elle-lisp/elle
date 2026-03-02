#!/usr/bin/env elle

# Functions — a student gradebook
#
# We build a small gradebook application, introducing Elle's function
# features as we need them.  Each section adds a new capability.
#
# Demonstrates:
#   fn / defn          — anonymous and named functions
#   Lexical scope      — let, let*, letrec, shadowing
#   Closures           — capturing environment, stateful graders
#   Higher-order fns   — map, filter, fold via recursion
#   Composition        — compose, ->, ->> pipelines
#   Variadic functions — & rest parameters
#   Mutual recursion   — letrec with mutually-recursive helpers
#   block / break      — named blocks with early exit
#   Mutable captures   — shared state between closures
#   Destructuring      — unpacking tuples and structs in let/fn params

(import-file "./examples/assertions.lisp")


# ========================================
# 1. fn and defn
# ========================================



# fn creates an anonymous function (a "lambda").
# (fn [params] body) — brackets delimit the parameter list.
(def average (fn [a b] (/ (+ a b) 2)))  # bind a lambda to 'average'
(assert-eq (average 80 90) 85 "fn: average")
(display "  (average 80 90) = ") (print (average 80 90))

# A function body can have multiple expressions.
# The last expression is the return value.
(def report (fn [name score]
  (def line (string/join (list name ": " (string score)) ""))  # build string
  (print line)           # side effect: print it
  line))                 # return value: the string
(assert-eq (report "Alice" 95) "Alice: 95" "fn: report returns last expr")

# defn is sugar for (def name (fn [params] body)).
# It also supports a docstring as the first body form.
(defn letter-grade [score]
  "Convert a numeric score to a letter grade."
  (cond                           # multi-branch conditional
    ((>= score 90) "A")
    ((>= score 80) "B")
    ((>= score 70) "C")
    ((>= score 60) "D")
    (true          "F")))         # default branch

(assert-eq (letter-grade 95) "A" "defn: A")
(assert-eq (letter-grade 85) "B" "defn: B")
(assert-eq (letter-grade 55) "F" "defn: F")
(display "  (letter-grade 95) = ") (print (letter-grade 95))

# (doc name) retrieves the docstring — bare symbol, no quoting needed
(assert-eq (doc letter-grade) "Convert a numeric score to a letter grade."
  "doc retrieves docstring")
(display "  (doc letter-grade) = ") (print (doc letter-grade))


# ========================================
# 2. Lexical scope
# ========================================



# A name is visible only in the block of code where it's defined.
# This is "lexical scope" — you can tell where a name is valid by
# reading the source, without running the program.

# let creates temporary bindings that exist only inside the let form.
# Both midterm and final are bound at the same time (neither sees the other).
(let ([midterm 82]
      [final 94])
  (assert-eq (average midterm final) 88
    "let: temporary bindings for grade calc"))
# midterm and final do not exist out here.

# let* binds sequentially — each binding can see the ones before it.
(let* ([raw 78]
       [curved (+ raw 5)]          # curved sees raw
       [grade (letter-grade curved)])  # grade sees curved
  (assert-eq grade "B" "let*: sequential — curved then graded"))

# letrec — bindings can reference themselves (for recursion)
(letrec ([sum-scores (fn [lst]
           (if (empty? lst)        # base case: nothing left
             0
             (+ (first lst)        # + wraps the recursive call, so this is
                (sum-scores (rest lst)))))])  # NOT tail-recursive (stack grows)
  (assert-eq (sum-scores (list 90 80 70)) 240
    "letrec: recursive sum"))

# Shadowing — an inner binding temporarily hides an outer one.
# The outer value is untouched; it reappears when the inner scope ends.
(def passing 60)
(let ([passing 70])                # shadows the outer 'passing'
  (assert-eq passing 70 "shadowing: inner passing"))
(assert-eq passing 60 "shadowing: outer passing unchanged")


# ========================================
# 3. Closures
# ========================================



# A closure captures values from its defining scope.
# make-curver returns a function that remembers the bonus amount.
(defn make-curver [bonus]
  "Return a function that adds bonus points to a score."
  (fn [score] (+ score bonus)))    # bonus is captured from make-curver's scope

(def curve5 (make-curver 5))       # remembers bonus=5
(def curve10 (make-curver 10))     # remembers bonus=10
(assert-eq (curve5 80) 85 "closure: curve5")
(assert-eq (curve10 80) 90 "closure: curve10")
(display "  (curve5 80) = ") (print (curve5 80))

# Mutable closure — a grader that tracks how many scores it's seen.
(defn make-grader []
  "Return a function that grades scores and counts how many it has seen."
  (var count 0)                    # mutable binding, captured by the closure
  (fn [score]
    (set count (+ count 1))        # mutate the captured variable
    (letter-grade score)))

(def grader (make-grader))
(assert-eq (grader 95) "A" "grader: first score")
(assert-eq (grader 72) "C" "grader: second score")


# ========================================
# 4. Higher-order functions
# ========================================



# A higher-order function takes or returns a function.
# my-map applies f to every element of a list.
(defn my-map [f lst]
  "Apply f to each element of lst, returning a new list."
  (if (empty? lst)
    (list)                         # base case: empty in, empty out
    (cons (f (first lst))          # cons wraps the recursive call, so
          (my-map f (rest lst))))) # not tail-recursive (stack grows with list length)

(def scores (list 72 85 90 68 95))
(assert-list-eq (my-map letter-grade scores)
  (list "C" "B" "A" "D" "A") "my-map: letter grades")

# my-filter keeps elements where pred returns true.
(defn my-filter [pred lst]
  "Keep elements of lst for which pred returns true."
  (if (empty? lst)
    (list)
    (if (pred (first lst))
      (cons (first lst) (my-filter pred (rest lst)))  # keep it
      (my-filter pred (rest lst)))))                  # skip it

(defn passing? [score]
  "Is this score a passing grade?"
  (>= score 60))

(assert-list-eq (my-filter passing? scores)
  (list 72 85 90 68 95) "my-filter: all pass here")
(assert-list-eq (my-filter (fn [s] (>= s 80)) scores)  # inline lambda
  (list 85 90 95) "my-filter: B or above")

# my-fold reduces a list to a single value.
# Unlike my-map, my-fold IS tail-recursive: the last thing it does is
# call itself.  Elle optimizes tail calls — this runs in constant stack
# space no matter how long the list.  (In C or Python, this would overflow.)
(defn my-fold [f acc lst]
  "Left fold: apply f to acc and each element of lst."
  (if (empty? lst)
    acc                            # base: return accumulator
    (my-fold f (f acc (first lst)) (rest lst))))  # tail call — no stack growth

(assert-eq (my-fold + 0 scores) 410 "my-fold: sum scores")
(assert-eq (my-fold max 0 scores) 95 "my-fold: max score")
(display "  sum of scores = ") (print (my-fold + 0 scores))


# ========================================
# 5. Composition and pipelines
# ========================================



# compose creates a new function from two existing ones.
# (compose f g)(x) = f(g(x))
(defn compose [f g]
  "Return a function that applies g then f."
  (fn [x] (f (g x))))             # g first, then f

(def curved-grade (compose letter-grade curve5))  # curve5 → letter-grade
(assert-eq (curved-grade 78) "B" "compose: curve then grade")
(display "  (curved-grade 78) = ") (print (curved-grade 78))

# -> thread-first: inserts value as the first argument to each form
(assert-eq (-> 75 (+ 10) letter-grade) "B"  # (+ 75 10) → (letter-grade 85)
  "->: curve then grade via threading")

# ->> thread-last: inserts value as the last argument
# Pipeline: take scores, keep B+, convert to letters
(assert-list-eq
  (->> scores
       (my-filter (fn [s] (>= s 80)))       # keep 80+
       (my-map letter-grade))               # convert to letters
  (list "B" "A" "A") "->>: filter then map")

# Full pipeline: class average of passing scores
(def class-avg
  (let* ([passing-scores (my-filter passing? scores)]  # keep ≥60
         [total (my-fold + 0 passing-scores)]          # sum them
         [count (length passing-scores)])              # count them
    (/ total count)))                                  # divide
(assert-eq class-avg 82 "pipeline: class average")


# ========================================
# 6. Variadic functions
# ========================================



# & collects remaining arguments into a list.
(defn grade-all [& student-scores]
  "Grade any number of scores, returning a list of letter grades."
  (my-map letter-grade student-scores))  # student-scores is a list

(assert-list-eq (grade-all 95 82 71) (list "A" "B" "C")
  "variadic: grade three scores")
(assert-list-eq (grade-all) (list)   # zero args → empty list
  "variadic: no scores")

# Fixed head + rest: first arg is required, rest collected.
(defn best-of [first-score & more]
  "Return the highest of all scores."
  (my-fold max first-score more))   # fold max starting from first

(assert-eq (best-of 72 85 90 68) 90 "variadic: best of four")
(assert-eq (best-of 100) 100 "variadic: single score")
(display "  (best-of 72 85 90 68) = ") (print (best-of 72 85 90 68))


# ========================================
# 7. Mutual recursion
# ========================================



# letrec lets functions call each other.
# Here: determine if a score list has an alternating pass/fail pattern.
# Both functions are mutually tail-recursive — the last call in each is
# to the other.  Elle optimizes mutual tail calls too, so this runs in
# constant stack space.
(letrec ([expect-pass (fn [lst]
           (if (empty? lst)
             true                  # ran out of scores — pattern holds
             (if (passing? (first lst))
               (expect-fail (rest lst))   # tail call to expect-fail
               false)))]                  # got fail, pattern broken
         [expect-fail (fn [lst]
           (if (empty? lst)
             true
             (if (not (passing? (first lst)))
               (expect-pass (rest lst))   # tail call to expect-pass
               false)))])                 # got pass, pattern broken
  (assert-true (expect-pass (list 80 50 90 40))
    "mutual: alternating pass/fail")
  (assert-false (expect-pass (list 80 90 50))
    "mutual: not alternating"))


# ========================================
# 8. block and break
# ========================================



# (block :name body...) creates a named scope.
# (break :name value) exits it early, returning value.
# Useful for "find first" patterns.

(def first-failing
  (block :search
    (each s in scores              # iterate the scores list
      (when (< s 70)               # when condition is truthy...
        (break :search s)))        # ...exit the block with this value
    nil))                          # fell through — nobody failed
(assert-eq first-failing 68 "block: found first failing score")

# Nested blocks — break targets the named block, crossing inner ones.
(def result
  (block :outer
    (block :inner
      (break :outer "escaped"))    # jumps past :inner AND :outer
    "never reached"))
(assert-eq result "escaped" "block: break crosses inner block")


# ========================================
# 9. Mutable captures and destructuring
# ========================================



# Two closures sharing the same mutable cell — a getter/setter pair.
(defn make-tracker []
  "Return [record!, summary] — closures sharing a running total and count."
  (var total 0)
  (var count 0)
  [                                # return a tuple of two closures
    (fn [score]
      (set total (+ total score))  # mutate shared 'total'
      (set count (+ count 1)))     # mutate shared 'count'
    (fn []
      (if (= count 0)
        0
        (/ total count)))])        # compute average

# Destructure the tuple directly into bindings
(def [record! avg] (make-tracker))
(record! 90)
(record! 80)
(record! 70)
(assert-eq (avg) 80 "mutable capture: running average")
(display "  running avg after 90,80,70 = ") (print (avg))

# Accumulator — another shared-cell pattern
(defn make-accumulator [initial]
  "Return a function that adds to a running total."
  (var total initial)
  (fn [amount]
    (set total (+ total amount))   # mutate captured 'total'
    total))                        # return new total

(def bonus-points (make-accumulator 0))
(assert-eq (bonus-points 5) 5 "accumulator: +5")
(assert-eq (bonus-points 3) 8 "accumulator: +3")
(assert-eq (bonus-points 2) 10 "accumulator: +2")

# Destructuring in function parameters — grade a student record
(defn grade-student [{:name name :score score}]
  "Grade a student record, returning a result struct."
  {:name name
   :score score
   :grade (letter-grade score)})

(def {:name rname :grade rgrade}   # destructure the result
  (grade-student {:name "Bob" :score 87}))
(assert-eq rname "Bob" "param destructure: name")
(assert-eq rgrade "B" "param destructure: grade")
(display "  grade-student({Bob, 87}) → ") (display rname) (display " ") (print rgrade)


(print "")
(print "all functions passed.")
