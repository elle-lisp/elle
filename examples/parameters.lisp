#!/usr/bin/env elle

# Parameters — dynamic binding with lexical scope
#
# Parameters are Racket-style dynamic bindings. Unlike lexical bindings
# (let, fn params), parameters are looked up at runtime from a dynamic
# context stack. This allows functions to read/write shared state without
# passing it as arguments.
#
# Demonstrates:
#   parameter      — create a parameter with a default value
#   parameter?          — test if a value is a parameter
#   Calling a parameter — read its current value
#   parameterize        — override a parameter's value in a scope
#   Nested parameterize — shadowing and revert
#   Fiber inheritance   — child fibers inherit parent's parameter bindings

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false} ((import-file "./examples/assertions.lisp")))


# ========================================
# 1. Creating and reading parameters
# ========================================

# parameter creates a parameter with a default value.
# Calling the parameter (with no args) reads its current value.
(def output-port (parameter "stdout"))
(assert-eq (output-port) "stdout" "parameter: read default")
(display "  (output-port) = ") (print (output-port))

# parameter? tests if a value is a parameter
(assert-true (parameter? output-port) "parameter?: true for parameter")
(assert-false (parameter? "stdout") "parameter?: false for string")
(assert-false (parameter? 42) "parameter?: false for number")
(display "  (parameter? output-port) = ") (print (parameter? output-port))

# Parameters can hold any value
(def current-user (parameter nil))
(assert-true (nil? (current-user)) "parameter: nil default")

(def max-retries (parameter 3))
(assert-eq (max-retries) 3 "parameter: int default")


# ========================================
# 2. parameterize — override in a scope
# ========================================

# parameterize temporarily overrides a parameter's value.
# The override is active only within the parameterize body.
# After the body, the parameter reverts to its previous value.

(def p (parameter 10))

# Inside parameterize, p reads the new value
(parameterize ((p 20))
  (assert-eq (p) 20 "parameterize: override active"))
(display "  Inside parameterize: (p) = 20")
(newline)

# Outside parameterize, p reverts to the default
(assert-eq (p) 10 "parameterize: reverts after")
(display "  After parameterize: (p) = ") (print (p))


# ========================================
# 3. Nested parameterize — shadowing
# ========================================

# Nested parameterize creates a stack of overrides.
# Inner overrides shadow outer ones.

(def level (parameter 1))

(parameterize ((level 2))
  (assert-eq (level) 2 "nested: outer override")
  
  (parameterize ((level 3))
    (assert-eq (level) 3 "nested: inner shadows outer"))
  
  # After inner parameterize, outer override is visible again
  (assert-eq (level) 2 "nested: outer visible after inner"))

# After all parameterize, default is visible
(assert-eq (level) 1 "nested: default after all")
(display "  Nested parameterize: level reverts correctly")
(newline)


# ========================================
# 4. Multiple parameters in one parameterize
# ========================================

# parameterize can override multiple parameters at once.
(def x (parameter 1))
(def y (parameter 10))

(parameterize ((x 2) (y 20))
  (assert-eq (x) 2 "multi: x overridden")
  (assert-eq (y) 20 "multi: y overridden")
  (assert-eq (+ (x) (y)) 22 "multi: both used in expression"))

(assert-eq (x) 1 "multi: x reverted")
(assert-eq (y) 10 "multi: y reverted")
(display "  Multiple parameters: both override and revert correctly")
(newline)


# ========================================
# 5. parameterize body is begin
# ========================================

# The body of parameterize is a begin — multiple expressions,
# last one is the return value.

(def config (parameter "default"))

(def result (parameterize ((config "test"))
  (def msg (string/join (list "Config: " (config)) ""))
  (print msg)
  (+ 1 2)))

(assert-eq result 3 "parameterize: body returns last expr")
(assert-eq (config) "default" "parameterize: reverted after multi-expr body")


# ========================================
# 6. Use case: simulating I/O ports
# ========================================

# A common use of parameters is to simulate I/O ports.
# Functions can read the current output port without it being passed as an argument.

(def current-output (parameter "stdout"))

(defn write-line [msg]
  "Write a message to the current output port."
  (let ((port (current-output)))
    (string/join (list "[" port "] " msg) "")))

# Default output port
(assert-eq (write-line "hello") "[stdout] hello"
  "io-port: default output")

# Override output port
(parameterize ((current-output "stderr"))
  (assert-eq (write-line "error") "[stderr] error"
    "io-port: overridden output"))

# Reverted to default
(assert-eq (write-line "done") "[stdout] done"
  "io-port: reverted output")

(display "  I/O port simulation: ")
(print (write-line "success"))


# ========================================
# 7. Fiber inheritance
# ========================================

# Child fibers inherit the parent's parameter bindings.
# When a child fiber is created inside a parameterize,
# the child sees the overridden values.

(def shared-param (parameter 100))

(parameterize ((shared-param 200))
  # Create a child fiber inside the parameterize
  (let ((f (fiber/new (fn [] (shared-param)) 1)))
    (fiber/resume f nil)
    (let ((child-value (fiber/value f)))
      (assert-eq child-value 200 "fiber: inherits parent parameterize"))))

(display "  Fiber inheritance: child sees parent's parameter override")
(newline)

# Outside parameterize, child would see the default
(let ((f (fiber/new (fn [] (shared-param)) 1)))
  (fiber/resume f nil)
  (let ((child-value (fiber/value f)))
    (assert-eq child-value 100 "fiber: inherits default outside parameterize")))

(display "  Fiber inheritance: child sees default outside parameterize")
(newline)
