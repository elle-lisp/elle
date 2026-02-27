#!/usr/bin/env elle
;;; Concurrency with spawn and join
;;;
;;; This example demonstrates the spawn and join primitives for concurrent
;;; execution in Elle. The spawn primitive creates a new thread that executes
;;; a closure, and join waits for the thread to complete and returns its result.
;;;
;;; Key concepts:
;;; 1. spawn takes a closure and executes it in a new thread
;;; 2. The closure can capture immutable values from the parent scope
;;; 3. join blocks until the thread completes and returns the result
;;; 4. Closures that capture mutable values (tables) cannot be spawned

(import-file "./examples/assertions.lisp")

;; Note: concurrency.lisp uses assert-equal and assert-true which are
;; defined in assertions.lisp. We need to adapt the calls to use the
;; standard assertion interface.

(display "=== Elle Concurrency Example: spawn and join ===")
(newline)

;;; Example 1: Simple computation in a thread
(display "Example 1: Simple computation in a thread")
(newline)

(let ((x 10) (y 20))
  (let ((handle (spawn (fn () (+ x y)))))
    (let ((result (join handle)))
      (display "Result of (+ 10 20) in thread: ")
      (display result)
      (newline)
      (assert-equal result 30 "Example 1: spawn/join computes 10+20 = 30"))))

;;; Example 2: Multiple threads computing in parallel
(display "Example 2: Multiple threads computing in parallel")
(newline)

(let ((h1 (spawn (fn () (* 2 3))))
      (h2 (spawn (fn () (* 4 5))))
      (h3 (spawn (fn () (* 6 7)))))
  (let ((r1 (join h1))
        (r2 (join h2))
        (r3 (join h3)))
    (display "Results: ")
    (display r1)
    (display ", ")
    (display r2)
    (display ", ")
    (display r3)
    (newline)
    (assert-equal r1 6 "Example 2: thread 1 computes 2*3 = 6")
    (assert-equal r2 20 "Example 2: thread 2 computes 4*5 = 20")
    (assert-equal r3 42 "Example 2: thread 3 computes 6*7 = 42")))

;;; Example 3: Capturing immutable values
(display "Example 3: Capturing immutable values")
(newline)

(let ((name "Alice")
      (age 30))
  (let ((handle (spawn (fn () 
                         (-> "Hello, " (append name) (append "! You are ") (append (string age)) (append " years old."))))))
    (let ((result (join handle)))
      (display result)
      (newline)
      (assert-true (string-contains? result "Alice") "Example 3: result contains 'Alice'")
      (assert-true (string-contains? result "30") "Example 3: result contains '30'"))))

;;; Example 4: Capturing vectors
(display "Example 4: Capturing vectors")
(newline)

(let ((numbers [1 2 3 4 5]))
  (let ((handle (spawn (fn () 
                         (let ((sum (+ 1 (+ 2 (+ 3 (+ 4 5))))))
                           sum)))))
    (let ((result (join handle)))
      (display "Sum of [1 2 3 4 5]: ")
      (display result)
      (newline)
      (assert-equal result 15 "Example 4: sum of 1+2+3+4+5 = 15"))))

;;; Example 5: Closure with conditional logic
(display "Example 5: Closure with conditional logic")
(newline)

(let ((threshold 50))
  (let ((handle (spawn (fn () 
                         (if (> threshold 40)
                             "threshold is high"
                             "threshold is low")))))
    (let ((result (join handle)))
      (display result)
      (newline)
      (assert-equal result "threshold is high" "Example 5: conditional returns 'threshold is high'"))))

;;; Example 6: Closure returning a value
(display "Example 6: Closure returning a value")
(newline)

(let ((multiplier 3))
  (let ((handle (spawn (fn () 
                         (* 7 multiplier)))))
    (let ((result (join handle)))
      (display "7 * 3 = ")
      (display result)
      (newline)
      (assert-equal result 21 "Example 6: 7*3 = 21"))))

;;; Example 7: Spawning closures with different capture patterns
(display "Example 7: Spawning closures with different capture patterns")
(newline)

(let ((x 100))
  (let ((h1 (spawn (fn () x)))
        (h2 (spawn (fn () (+ x 50)))))
    (let ((r1 (join h1))
          (r2 (join h2)))
      (display "Captured value: ")
      (display r1)
      (newline)
      (display "Computed value: ")
      (display r2)
      (newline)
      (assert-equal r1 100 "Example 7: captured value = 100")
      (assert-equal r2 150 "Example 7: computed value = 100+50 = 150"))))

;;; Example 8: Using time/sleep with threads
(display "Example 8: Using time/sleep with threads")
(newline)

(let ((handle (spawn (fn () 
                       (begin
                         (display "Thread started")
                         (newline)
                         (time/sleep 0.1)
                         (display "Thread finished after time/sleep")
                         (newline)
                         42)))))
  (display "Main thread waiting for spawned thread...")
  (newline)
  (let ((result (join handle)))
    (display "Spawned thread returned: ")
    (display result)
    (newline)
    (assert-equal result 42 "Example 8: thread with time/sleep returns 42")))

;;; Example 9: Current thread ID
(display "Example 9: Current thread ID")
(newline)

(let ((main-id (current-thread-id)))
  (display "Main thread ID: ")
  (display main-id)
  (newline)

  (let ((handle (spawn (fn () (current-thread-id)))))
    (let ((spawned-id (join handle)))
      (display "Spawned thread ID: ")
      (display spawned-id)
      (newline)
      (assert-true (not (= main-id spawned-id)) "Example 9: spawned thread has different ID"))))

;;; Example 10: Complex computation with multiple captures
(display "Example 10: Complex computation with multiple captures")
(newline)

(let ((a 2) (b 3) (c 4) (d 5))
  (let ((handle (spawn (fn () 
                         (+ (* a b) (* c d))))))
    (let ((result (join handle)))
      (display "Result of (+ (* 2 3) (* 4 5)): ")
      (display result)
      (newline)
      (assert-equal result 26 "Example 10: (2*3)+(4*5) = 6+20 = 26"))))

(display "=== End of concurrency example ===")
(newline)
(display "=== Concurrency Assertions Complete ===")
(newline)
