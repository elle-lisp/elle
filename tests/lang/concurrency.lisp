(elle/epoch 14)
# audited: 2026-09-30
# sys/spawn-vm runs a closure on another thread, and the captures and traits it carries survive the crossing.
# docs/threads.md

(defn on-thread [thunk]
  "Run `thunk` on a new thread and answer what it returned."
  (sys/join (sys/spawn-vm thunk)))

# ============================================================================
# A capture crosses to the thread with its value
# ============================================================================

(assert (= (on-thread (fn () 42)) 42) "a closure with no captures")

(assert (= (let [x 42]
             (on-thread (fn () x))) 42) "an int capture")

(assert (= (let [msg "hello from thread"]
             (on-thread (fn () msg))) "hello from thread") "a string capture")

(assert (= (let [v [1 2 3]]
             (on-thread (fn () v))) [1 2 3]) "an array capture")

(assert (= (let [x 10
                 y 20]
             (on-thread (fn () (+ x y)))) 30) "a computation over two captures")

(assert (= (let [a 1
                 b 2
                 c 3]
             (on-thread (fn () (+ a (+ b c))))) 6) "three captures")

(assert (nil? (let [n nil]
                (on-thread (fn () n)))) "a nil capture")

(assert (= (let [f 3.14159]
             (on-thread (fn () f))) 3.14159) "a float capture")

(assert (= (let [lst (list 1 2 3)]
             (on-thread (fn () lst))) (list 1 2 3)) "a list capture")

(assert (= (let [x 10]
             (on-thread (fn () (if (> x 5) "big" "small")))) "big")
        "a conditional over a capture")

# A closure bound to a name first crosses the same way as one written inline.

(assert (= (let [a 10
                 b 20]
             (let [closure (fn () (+ a b))]
               (on-thread closure))) 30) "a named closure over two captures")

(assert (= (let [v [10 20 30]]
             (let [closure (fn () v)]
               (on-thread closure))) [10 20 30]) "a named closure over an array")

(let [[ok? result] (protect (let [t (@struct :a 1)]
                              (on-thread (fn () (t :a)))))]
  (assert ok? "a mutable @struct capture crosses")
  (assert (= result 1) "the crossed @struct keeps its data"))

# ============================================================================
# The thread
# ============================================================================

(assert (int? (sys/thread-id)) "sys/thread-id answers an int")
(assert (= (sys/thread-id) (sys/thread-id))
        "sys/thread-id is the same on every call")
(assert (not (= (on-thread (fn () (sys/thread-id))) (sys/thread-id)))
        "a spawned thread has an id of its own")
(assert (= current-thread-id sys/thread-id)
        "current-thread-id names sys/thread-id")

# ============================================================================
# What spawn and join refuse
# ============================================================================

# `parse-int`, not `abs`: abs is a stdlib closure and spawns legitimately, so
# the subject must be a native fn for the refusal to be the one tested.
(assert (native-fn? parse-int) "the refusal's subject is a native fn")
(let [[ok? _] (protect (sys/spawn-vm parse-int))]
  (assert (not ok?) "sys/spawn-vm refuses a native function"))

(let [[ok? err] (protect (sys/spawn-vm 42))]
  (assert (and (not ok?) (= (get err :error) :type-error))
          "sys/spawn-vm refuses an int"))

(let [[ok? _] (protect ((fn () (eval '(sys/spawn-vm)))))]
  (assert (not ok?) "sys/spawn-vm with no arguments fails"))

(let [[ok? _] (protect ((fn () (eval '(sys/spawn-vm (fn () 1) 2)))))]
  (assert (not ok?) "sys/spawn-vm with two arguments fails"))

(let [[ok? _] (protect ((fn () (eval '(sys/join)))))]
  (assert (not ok?) "sys/join with no arguments fails"))

(let [[ok? err] (protect (sys/join (sys/spawn-vm (fn () 1)) 2))]
  (assert (and (not ok?) (= (get err :error) :argument-error))
          "sys/join takes its bound only as named arguments"))

(let [[ok? err] (protect (sys/join 42))]
  (assert (and (not ok?) (= (get err :error) :type-error))
          "sys/join refuses a value that is not a thread handle"))

# ============================================================================
# A closure capture crosses as a closure
# ============================================================================

(assert (= (let [add1 (fn (x) (+ x 1))]
             (on-thread (fn () (add1 41)))) 42) "a closure capturing a closure")

(assert (= (let [add1 (fn (x) (+ x 1))]
             (let [add2 (fn (x) (add1 (add1 x)))]
               (on-thread (fn () (add2 40))))) 42)
        "a closure capturing nested closures")

(assert (= (let [f (on-thread (fn () (fn (x) (* x 2))))]
             (f 21)) 42) "a thread's closure result crosses back")

(assert (= (let [offset 10]
             (let [add-offset (fn (x) (+ x offset))]
               (on-thread (fn () (add-offset 32))))) 42)
        "a closure capturing a closure and data")

(let [[ok? result] (protect (let [t (@struct :x 42)]
                              (let [f (fn () (t :x))]
                                (on-thread (fn () (f))))))]
  (assert ok? "a closure over a closure over an @struct crosses")
  (assert (= result 42) "the @struct keeps its data through the closure"))

# ============================================================================
# Traits
# ============================================================================

(begin
  (def v (with-traits [1 2 3] {:tag :my-type}))
  (def result (on-thread (fn [] (traits v))))
  (assert (not (nil? result)) "user traits survive the crossing")
  (assert (= (result :tag) :my-type) "user trait data survives the crossing"))

(begin
  (def v [10 20 30])
  (def result (on-thread (fn [] (first v))))
  (assert (= result 10) "a value's default traits work on the receiving thread"))

# ============================================================================
# Recursive closures
# ============================================================================

(assert (= (letrec [fact (fn (n)
                           (if (= n 0)
                             1
                             (* n (fact (- n 1)))))]
             (on-thread (fn () (fact 6)))) 720) "a self-recursive closure")

(assert (= (letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1))))
                    odd? (fn (n) (if (= n 0) false (even? (- n 1))))]
             (on-thread (fn () (even? 10)))) true) "mutually recursive closures")

(assert (= (letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1))))
                    odd? (fn (n) (if (= n 0) false (even? (- n 1))))]
             (on-thread (fn () (odd? 99)))) true) "mutual recursion 99 deep")

# ============================================================================
# Captured closures called in a hot loop
#
# Each spawned closure calls a captured closure often enough to be compiled on
# the worker thread. A captured closure must therefore arrive whole in compiled
# code as well: a constant that did not survive the crossing would leave the
# worker to run it in the interpreter or to fail the call.
# ============================================================================

(assert (= (let [double (fn (x) (* x 2))]
             (letrec [loop (fn (n acc)
                             (if (= n 0)
                               acc
                               (loop (- n 1) (+ acc (double n)))))]
               (on-thread (fn () (loop 100 0))))) 10100)
        "a hot loop over a captured closure")

(assert (= (let [inc (fn (x) (+ x 1))
                 sq (fn (x) (* x x))]
             (letrec [loop (fn (n acc)
                             (if (= n 0)
                               acc
                               (loop (- n 1) (+ acc (sq (inc n))))))]
               (on-thread (fn () (loop 50 0))))) 45525)
        "a hot loop over two captured closures")

(assert (= (let [compose (fn (f g) (fn (x) (f (g x))))]
             (let [inc (fn (x) (+ x 1))
                   dbl (fn (x) (* x 2))]
               (let [f (compose dbl inc)]
                 (letrec [loop (fn (n acc)
                                 (if (= n 0)
                                   acc
                                   (loop (- n 1) (+ acc (f n)))))]
                   (on-thread (fn () (loop 100 0))))))) 10300)
        "a hot loop over composed closures")
