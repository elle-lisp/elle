(elle/epoch 1)
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
#   Fiber error handling — mask, terminal vs resumable, cancel vs abort
#   Practical patterns — safe wrappers, validation, error inspection



# ========================================
# 1. Raising and catching errors
# ========================================

# Errors in Elle are values signaled via fibers. By convention, a struct
# {:error :keyword :message "message"} is used, but any value works.
# (error val) is a prelude macro that expands to (emit 1 val).

# try/catch runs the body; if an error occurs, the catch handler runs
# with the error bound to the catch variable.

(def result (try
  (error {:error :demo :message "something went wrong"})
  (catch e :caught)))
(assert (= result :caught) "try/catch: error triggers catch")

# The catch binding holds the error struct {:error :kind :message "message"}
(def err (try
  (error {:error :bad-input :message "expected a number"})
  (catch e e)))
(display "  caught error: ") (print err)
(assert (= (get err :error) :bad-input) "error kind is a keyword")
(assert (= (get err :message) "expected a number") "error message is a string")

# When no error occurs, try returns the body's result
(def ok-result (try
  (+ 10 20)
  (catch e :should-not-reach)))
(assert (= ok-result 30) "try/catch: no error returns body result")

# Multiple expressions in the try body — last value is returned
(def multi (try
  (def x 10)
  (def y 20)
  (+ x y)
  (catch e :error)))
(assert (= multi 30) "try/catch: multiple body expressions")


# ========================================
# 2. Nested try/catch
# ========================================

# Errors can be caught at any level. An inner catch can re-signal.
(def outer-result
  (try
    (try
      (error {:error :inner :message "from inside"})
      (catch e
        # Caught the inner error, now signal a new one
        (error {:error :wrapped :message (string/join (list "wrapped: " (get e :message)) "")})))
    (catch e e)))
(display "  nested re-signal: ") (print outer-result)
(assert (= (get outer-result :error) :wrapped) "nested: outer catches re-signaled error")

# Built-in errors (like division by zero) are also catchable
(def div-err (try (/ 1 0) (catch e e)))
(display "  division by zero: ") (print div-err)
(assert (= (get div-err :error) :division-by-zero) "built-in error: division by zero")


# ========================================
# 3. protect — errors as data
# ========================================

# protect runs its body and returns [success? value] without propagating.
# Success case:
(def [ok1? val1] (protect (+ 100 200)))
(display "  protect ok: [") (display ok1?) (display " ") (display val1) (print "]")
(assert ok1? "protect: success returns true")
(assert (= val1 300) "protect: success value")

# Error case:
(def [ok2? val2] (protect (error {:error :boom :message "exploded"})))
(display "  protect err: [") (display ok2?) (display " ") (display val2) (print "]")
(assert (not ok2?) "protect: error returns false")
(assert (= (get val2 :error) :boom) "protect: error kind preserved")

# protect is useful for "try this, fall back to that" patterns
(defn safe-parse [s]
  "Try to parse a string as an integer, return nil on failure."
  (def [ok? val] (protect (integer s)))
  (if ok? val nil))

(assert (= (safe-parse "42") 42) "safe-parse: valid input")
(assert (= (safe-parse "abc") nil) "safe-parse: invalid input returns nil")
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
(assert (= defer-result 42) "defer: returns body result")
(assert (= log @[:body :cleanup]) "defer: cleanup runs after body")
(display "  defer log: ") (print log)

# Error case: cleanup runs, then error re-propagates
(def err-log @[])
(def defer-err (try
  (defer (push err-log :cleanup)
    (push err-log :body)
    (error {:error :fail :message "oops"})
    (push err-log :unreachable))
  (catch e (push err-log :caught) e)))
(assert (= err-log @[:body :cleanup :caught]) "defer: cleanup before catch")
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

(assert (= with-result 1) "with: returns body result")
(assert (= res-log @[:opened :used :closed]) "with: acquire, use, release")
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
    (error {:error :io :message "write failed"}))
  (catch e :recovered)))

(assert (= with-err :recovered) "with: error caught after cleanup")
(assert (= err-res-log @[:opened :used :closed]) "with: cleanup on error")
(display "  with error log: ") (print err-res-log)


# ========================================
# 6. Error propagation
# ========================================

# Errors bubble up through the call stack until caught.

(defn validate-age [age]
  "Ensure age is a positive integer."
  (when (not (= (type age) :integer))
    (error {:error :type-error :message "age must be an integer"}))
  (when (< age 0)
    (error {:error :value-error :message "age must be non-negative"}))
  (when (> age 150)
    (error {:error :value-error :message "age is unreasonably large"}))
  age)

(defn create-person [name age]
  "Create a person record, validating the age."
  {:name name :age (validate-age age)})

# Valid input works fine
(def alice (create-person "Alice" 30))
(assert (= (get alice :name) "Alice") "propagation: valid input")
(display "  person: ") (print alice)

# Invalid input: error propagates from validate-age through create-person
(def person-err (try
  (create-person "Bob" -5)
  (catch e e)))
(display "  bad age: ") (print person-err)
(assert (= (get person-err :error) :value-error) "propagation: error kind correct")

(def type-err (try
  (create-person "Charlie" "thirty")
  (catch e e)))
(display "  bad type: ") (print type-err)
(assert (= (get type-err :error) :type-error) "propagation: type error caught")


# ========================================
# 7. Fiber error handling — mask and resumption
# ========================================

# A fiber's mask determines whether errors are terminal or resumable.
# mask=0: errors propagate to parent, fiber enters :error (terminal)
# mask=1: parent catches errors, fiber stays :paused (resumable)

# Fiber with mask=0: error propagates to parent
(def f0 (fiber/new (fn [] (error {:error :boom :message "kaboom"})) 0))

# The error propagates to us, so we must catch it to survive.
(def [ok? err] (protect (fiber/resume f0)))
(display "  mask=0 fiber errored: ") (print err)
(assert (not ok?) "mask=0: error propagates to parent")
(assert (= (get err :error) :boom) "mask=0: error kind preserved")

(def status0 (fiber/status f0))
(display "  fiber status: ") (print status0)
(assert (= status0 :error) "mask=0: fiber is in :error status")

# Attempting to resume an errored fiber fails
(def [ok2? err2] (protect (fiber/resume f0)))
(display "  resume errored fiber: ") (print err2)
(assert (not ok2?) "resume errored: signals an error")
(assert (= (get err2 :error) :state-error) "resume errored: error kind is :state-error")

# The fiber is still in :error — nothing changed.
(assert (= (fiber/status f0) :error) "fiber still :error after failed resume")


# Fiber with mask=1: errors are caught, fiber stays :paused
(def f1 (fiber/new (fn [] (error {:error :caught-boom :message "handled"})) 1))
(fiber/resume f1 nil)

(def status1 (fiber/status f1))
(def value1 (fiber/value f1))
(display "  mask=1 fiber status: ") (print status1)
(display "  mask=1 fiber value: ") (print value1)
(assert (= status1 :paused) "mask=1: fiber is :paused, not :error")
(assert (= (get value1 :error) :caught-boom) "mask=1: error value accessible")

# A suspended fiber can be resumed (though it has nothing left to do).
(fiber/resume f1 nil)
(def status1b (fiber/status f1))
(display "  mask=1 after second resume: ") (print status1b)
(assert (= status1b :dead) "mask=1: fiber completes on second resume")


# ========================================
# 8. fiber/cancel vs fiber/abort
# ========================================

# fiber/cancel — hard kill, no cleanup
#
# cancel sets the fiber to :error immediately. No defer blocks run,
# no protect handlers execute. The fiber is dead.

(def log1 @[])
(def f-cancel (fiber/new (fn []
  (defer (push log1 :cleanup)
    (yield :waiting)
    (push log1 :body-done)
    :done)) 3))
(fiber/resume f-cancel nil)
(assert (= (fiber/status f-cancel) :paused) "cancel: starts paused")

(fiber/cancel f-cancel {:error :cancelled})
(display "  cancel status: ") (print (fiber/status f-cancel))
(display "  cancel log: ") (print log1)
(assert (= (fiber/status f-cancel) :error) "cancel: fiber is :error")
(assert (= (length log1) 0) "cancel: no defer ran")

# Resuming a cancelled fiber fails.
(def [ok-c? err-c] (protect (fiber/resume f-cancel)))
(display "  resume cancelled: ") (print err-c)
(assert (not ok-c?) "resume cancelled: signals error")

# fiber/abort — inject error and resume
#
# abort injects an error into a paused fiber and resumes it.
# Unlike cancel, the fiber's error handling machinery (mask, protect)
# participates. With mask=1 (catches errors), the fiber suspends
# with the injected error as a caught signal.

(def f-abort (fiber/new (fn []
  (yield :waiting)
  :done) 3))  # mask=3 catches errors+yields
(fiber/resume f-abort nil)
(assert (= (fiber/status f-abort) :paused) "abort: starts paused")
(assert (= (fiber/value f-abort) :waiting) "abort: yielded :waiting")

(fiber/abort f-abort {:error :aborted})
(display "  abort status: ") (print (fiber/status f-abort))
(display "  abort value: ") (print (fiber/value f-abort))
# With mask=3, the injected error is caught — fiber is :paused with the error
(assert (= (fiber/status f-abort) :paused) "abort: error caught by mask")
(assert (= (get (fiber/value f-abort) :error) :aborted) "abort: error value preserved")

# Contrast with cancel: cancel kills immediately, abort goes through mask.
# cancel with mask=3: fiber is dead regardless of mask.
(def log-cmp @[])
(def f-cmp (fiber/new (fn []
  (yield :waiting)
  (push log-cmp :after-yield)
  :done) 3))  # mask=3 — would catch errors, but cancel bypasses mask
(fiber/resume f-cmp nil)
(fiber/cancel f-cmp {:error :killed})
(display "  cancel vs abort - cancel: ") (print (fiber/status f-cmp))
(assert (= (fiber/status f-cmp) :error) "cancel ignores mask")

# fiber/cancel self — a fiber can cancel itself
#
# Self-cancel sends SIG_TERMINAL which is uncatchable — it propagates
# through all mask checks (including protect/defer). This is intentional:
# self-cancel is a hard kill that cannot be intercepted.
# (fiber/cancel (fiber/self) :reason) — terminates the entire fiber chain.


# ========================================
# 9. Practical patterns
# ========================================

# Pattern: safe division with error recovery
(defn safe-divide [a b]
  "Divide a by b, returning [:ok result] or [:err message]."
  (def [ok? val] (protect (/ a b)))
  (if ok?
    [:ok val]
    [:err (get val :message)]))

(assert (= (safe-divide 10 2) [:ok 5]) "safe-divide: success")
(assert (= (get (safe-divide 1 0) 0) :err) "safe-divide: division by zero")
(display "  10 / 2 = ") (print (safe-divide 10 2))
(display "  1 / 0 = ") (print (safe-divide 1 0))

# Pattern: validate multiple fields, collecting all errors
(defn validate-config [cfg]
  "Validate a config @struct. Returns [:ok cfg] or [:err errors]."
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
(assert (= (get (validate-config good-cfg) 0) :ok) "validate: good config")
(assert (= (get (validate-config bad-cfg) 0) :err) "validate: bad config")

(print "")
(print "all errors passed.")
