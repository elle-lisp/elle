#!/usr/bin/env elle

# Errors — Elle's error handling facilities
#
# Demonstrates:
#   error            — raising errors via fiber signals
#   try / catch      — recovering from errors
#   protect          — capturing errors as data (no propagation)
#   defer            — guaranteed cleanup after body
#   with             — resource management (acquire / use / release)
#   Error propagation — errors bubble through call stacks
#   Practical patterns — safe wrappers, validation, error inspection

(import-file "./examples/assertions.lisp")


# Errors in Elle are values signaled via fibers. By convention, a tuple
# [:keyword "message"] is used, but any value works. (error val) is a
# prelude macro that expands to (fiber/signal 1 val).


# ========================================
# 1. Raising and catching errors
# ========================================

# try/catch runs the body; if an error occurs, the catch handler runs
# with the error bound to the catch variable.

(def result (try
  (error [:demo "something went wrong"])
  (catch e :caught)))
(assert-eq result :caught "try/catch: error triggers catch")

# The catch binding holds the error tuple [:kind "message"]
(def err (try
  (error [:bad-input "expected a number"])
  (catch e e)))
(display "  caught error: ") (print err)
(assert-eq (get err 0) :bad-input "error kind is a keyword")
(assert-eq (get err 1) "expected a number" "error message is a string")

# When no error occurs, try returns the body's result
(def ok-result (try
  (+ 10 20)
  (catch e :should-not-reach)))
(assert-eq ok-result 30 "try/catch: no error returns body result")

# Multiple expressions in the try body — last value is returned
(def multi (try
  (def x 10)
  (def y 20)
  (+ x y)
  (catch e :error)))
(assert-eq multi 30 "try/catch: multiple body expressions")


# ========================================
# 2. Nested try/catch
# ========================================

# Errors can be caught at any level. An inner catch can re-raise.
(def outer-result
  (try
    (try
      (error [:inner "from inside"])
      (catch e
        # Caught the inner error, now raise a new one
        (error [:wrapped (string/join (list "wrapped: " (get e 1)) "")])))
    (catch e e)))
(display "  nested re-raise: ") (print outer-result)
(assert-eq (get outer-result 0) :wrapped "nested: outer catches re-raised error")

# Built-in errors (like division by zero) are also catchable
(def div-err (try (/ 1 0) (catch e e)))
(display "  division by zero: ") (print div-err)
(assert-eq (get div-err 0) :division-by-zero "built-in error: division by zero")


# ========================================
# 3. protect — errors as data
# ========================================

# protect runs its body and returns [success? value] without propagating.
# Success case:
(def [ok1? val1] (protect (+ 100 200)))
(display "  protect ok: [") (display ok1?) (display " ") (display val1) (print "]")
(assert-true ok1? "protect: success returns true")
(assert-eq val1 300 "protect: success value")

# Error case:
(def [ok2? val2] (protect (error [:boom "exploded"])))
(display "  protect err: [") (display ok2?) (display " ") (display val2) (print "]")
(assert-false ok2? "protect: error returns false")
(assert-eq (get val2 0) :boom "protect: error kind preserved")

# protect is useful for "try this, fall back to that" patterns
(defn safe-parse [s]
  "Try to parse a string as an integer, return nil on failure."
  (def [ok? val] (protect (string->integer s)))
  (if ok? val nil))

(assert-eq (safe-parse "42") 42 "safe-parse: valid input")
(assert-eq (safe-parse "abc") nil "safe-parse: invalid input returns nil")
(display "  safe-parse \"42\" = ") (print (safe-parse "42"))
(display "  safe-parse \"abc\" = ") (print (safe-parse "abc"))


# ========================================
# 4. defer — guaranteed cleanup
# ========================================

# defer runs cleanup after body, whether body succeeds or errors.
# Syntax: (defer cleanup-expr body...)

# Success case: cleanup runs, body result returned
(def log @[])
(def defer-result (defer (push log :cleanup) (push log :body) 42))
(assert-eq defer-result 42 "defer: returns body result")
(assert-eq log @[:body :cleanup] "defer: cleanup runs after body")
(display "  defer log: ") (print log)

# Error case: cleanup runs, then error re-propagates
(def err-log @[])
(def defer-err (try
  (defer (push err-log :cleanup)
    (push err-log :body)
    (error [:fail "oops"])
    (push err-log :unreachable))
  (catch e (push err-log :caught) e)))
(assert-eq err-log @[:body :cleanup :caught] "defer: cleanup before catch")
(display "  defer error log: ") (print err-log)


# ========================================
# 5. with — resource management
# ========================================

# with acquires a resource, runs body, then releases via destructor.
# Syntax: (with binding constructor destructor body...)
# The destructor is called with the binding value, even on error.

(def res-log @[])

(defn open-connection []
  "Simulate opening a connection."
  (push res-log :opened)
  {:type :connection :id 1})

(defn close-connection [conn]
  "Simulate closing a connection."
  (push res-log :closed))

(def with-result
  (with conn (open-connection) close-connection
    (push res-log :used)
    (get conn :id)))

(assert-eq with-result 1 "with: returns body result")
(assert-eq res-log @[:opened :used :closed] "with: acquire, use, release")
(display "  with log: ") (print res-log)

# Cleanup happens even when body errors
(def err-res-log @[])

(defn open-file []
  (push err-res-log :opened)
  :file-handle)

(defn close-file [f]
  (push err-res-log :closed))

(def with-err (try
  (with f (open-file) close-file
    (push err-res-log :used)
    (error [:io "write failed"]))
  (catch e :recovered)))

(assert-eq with-err :recovered "with: error caught after cleanup")
(assert-eq err-res-log @[:opened :used :closed] "with: cleanup on error")
(display "  with error log: ") (print err-res-log)


# ========================================
# 6. Error propagation
# ========================================

# Errors bubble up through the call stack until caught.

(defn validate-age [age]
  "Ensure age is a positive integer."
  (when (not (= (type age) :integer))
    (error [:type-error "age must be an integer"]))
  (when (< age 0)
    (error [:value-error "age must be non-negative"]))
  (when (> age 150)
    (error [:value-error "age is unreasonably large"]))
  age)

(defn create-person [name age]
  "Create a person record, validating the age."
  {:name name :age (validate-age age)})

# Valid input works fine
(def alice (create-person "Alice" 30))
(assert-eq (get alice :name) "Alice" "propagation: valid input")
(display "  person: ") (print alice)

# Invalid input: error propagates from validate-age through create-person
(def person-err (try
  (create-person "Bob" -5)
  (catch e e)))
(display "  bad age: ") (print person-err)
(assert-eq (get person-err 0) :value-error "propagation: error kind correct")

(def type-err (try
  (create-person "Charlie" "thirty")
  (catch e e)))
(display "  bad type: ") (print type-err)
(assert-eq (get type-err 0) :type-error "propagation: type error caught")


# ========================================
# 7. Practical patterns
# ========================================

# Pattern: safe division with error recovery
(defn safe-divide [a b]
  "Divide a by b, returning [:ok result] or [:err message]."
  (def [ok? val] (protect (/ a b)))
  (if ok?
    [:ok val]
    [:err (get val 1)]))

(assert-eq (safe-divide 10 2) [:ok 5] "safe-divide: success")
(assert-eq (get (safe-divide 1 0) 0) :err "safe-divide: division by zero")
(display "  10 / 2 = ") (print (safe-divide 10 2))
(display "  1 / 0 = ") (print (safe-divide 1 0))

# Pattern: validate multiple fields, collecting all errors
(defn validate-config [cfg]
  "Validate a config table. Returns [:ok cfg] or [:err errors]."
  (def errors @[])
  (when (nil? (get cfg :host))
    (push errors "missing :host"))
  (when (nil? (get cfg :port))
    (push errors "missing :port"))
  (when-let ((port (get cfg :port)))
    (when (not (= (type port) :integer))
      (push errors "port must be an integer"))
    (when (and (= (type port) :integer) (or (< port 1) (> port 65535)))
      (push errors "port out of range")))
  (if (= (length errors) 0)
    [:ok cfg]
    [:err errors]))

(def good-cfg @{:host "localhost" :port 8080})
(def bad-cfg @{:port -1})

(display "  valid config: ") (print (validate-config good-cfg))
(display "  bad config:   ") (print (validate-config bad-cfg))
(assert-eq (get (validate-config good-cfg) 0) :ok "validate: good config")
(assert-eq (get (validate-config bad-cfg) 0) :err "validate: bad config")

(print "")
(print "all errors passed.")
