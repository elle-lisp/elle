#!/usr/bin/env elle

# Effects — user-defined signals and effect restrictions
#
# Demonstrates:
#   effect              — declaring user-defined signal keywords
#   restrict            — bounding which signals a function may emit
#   restrict (param)    — bounding which signals a callback may emit
#   Progress reporting  — signaling progress from long-running work
#   Early termination   — signaling :abort to stop a search early
#   Logging             — signaling :log entries the caller can collect
#   Plugin sandboxing   — restricting what signals a plugin may emit

(def {:assert-eq assert-eq :assert-equal assert-equal :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "./examples/assertions.lisp")))


# ========================================
# 1. Declaring user-defined effects
# ========================================

# effect declares a new user-defined effect that functions can signal.
# Effects are flow-control signals: they interrupt execution and let the
# caller decide how to respond. They have nothing to do with side-effects
# (a function can mutate state and still be inert if it emits no signals).

(effect :log)
(effect :progress)
(effect :abort)

# :log — a function signals this to request that a log entry be written;
#        the caller decides whether to handle it or ignore it
# :progress — a long-running function signals progress updates; the caller
#             can display them, collect them, or ignore them entirely
# :abort — a function signals early termination; the caller can catch and
#          recover, or let it propagate


# ========================================
# 2. Progress reporting
# ========================================

# A long-running computation signals :progress at each step.
# The caller intercepts these signals using a fiber and drives the
# computation forward, collecting progress updates.

(defn process-items [items]
  "Process each item, signaling :progress after each one."
  (map (fn [item]
         (def result (* item item))
         (fiber/signal :progress {:item item :result result})
         result)
       items))

# Run in a fiber that catches :progress signals
(def progress-log @[])

(defn run-process []
  (process-items [1 2 3 4 5]))

(def f (fiber/new run-process |:progress|))
(var done false)
(while (not done)
  (fiber/resume f nil)
  (if (= (fiber/status f) :paused)
    (push progress-log (fiber/value f))
    (assign done true)))

(assert-eq (length progress-log) 5 "progress: 5 updates received")
(assert-eq (get (get progress-log 0) :result) 1  "progress: 1^2 = 1")
(assert-eq (get (get progress-log 4) :result) 25 "progress: 5^2 = 25")
(display "  progress log: ") (print progress-log)


# ========================================
# 3. Early termination with :abort
# ========================================

# A linear search that signals :abort the moment it finds a match,
# skipping the rest of the list.

(defn find-first [pred xs]
  "Scan xs, signaling :abort with the first element satisfying pred."
  (each x xs
    (when (pred x)
      (fiber/signal :abort x)))
  nil)

# Catch :abort to get the found value
(defn find-even []
  (find-first even? [1 3 5 4 7 9]))

(def search-fiber (fiber/new find-even |:abort|))

(fiber/resume search-fiber nil)
(def found (if (= (fiber/status search-fiber) :paused)
  (fiber/value search-fiber)
  nil))
(display "  first even in [1 3 5 4 7 9]: ") (print found)
(assert-eq found 4 "abort: found first even number, stopped early")


# ========================================
# 4. Logging — caller decides what to do with log entries
# ========================================

# A computation that signals :log as it works. Show two uses: one that
# collects logs, one that ignores them.

(defn compute-with-log [x]
  "Compute a result, signaling :log at each step."
  (fiber/signal :log {:level :info :msg "starting"})
  (def step1 (* x 2))
  (fiber/signal :log {:level :debug :msg (string "doubled to " step1)})
  (def step2 (+ step1 10))
  (fiber/signal :log {:level :info :msg "done"})
  step2)

# Caller 1: collect log entries
(def log-entries @[])

(defn compute-and-log []
  (compute-with-log 5))

(def log-fiber (fiber/new compute-and-log |:log|))
(var log-done false)
(while (not log-done)
  (fiber/resume log-fiber nil)
  (if (= (fiber/status log-fiber) :paused)
    (push log-entries (fiber/value log-fiber))
    (assign log-done true)))
(display "  log entries: ") (print log-entries)
(assert-eq (length log-entries) 3 "log: 3 entries collected")
(assert-eq (fiber/value log-fiber) 20 "log: result is 20")

# Caller 2: ignore log entries entirely — just run it
(def result (compute-with-log 5))
(assert-eq result 20 "log: result same when logs ignored")


# ========================================
# 5. restrict — protecting a data structure from signaling callbacks
# ========================================

# A map-like function that requires its callback to be inert. The comment
# explains WHY: if the callback could signal mid-iteration, the iteration
# state would be left inconsistent.

(defn safe-map [f xs]
  "Map f over xs. f must be inert — a signaling callback would
   interrupt the iteration and leave results in an inconsistent state."
  (restrict f)
  (map f xs))

# Works fine with an inert callback
(def squares (safe-map (fn [x] (* x x)) [1 2 3 4 5]))
(display "  squares: ") (print squares)
(assert-list-eq squares [1 4 9 16 25] "safe-map: inert callback works")

# Fails at runtime when callback signals
(def [ok? err] (protect
  (safe-map (fn [x] (fiber/signal :log {:msg "oops"}) x) [1 2 3])))
(display "  effect violation caught: ") (print err)
# err is {:error :effect-violation :message "..."} — handle by kind:
# (match (get err :error)
#   :effect-violation (display "callback tried to signal")
#   _ (error err))
(assert-false ok? "safe-map: signaling callback rejected")
(assert-eq (get err :error) :effect-violation "safe-map: effect-violation error")


# ========================================
# 6. restrict — plugin sandboxing
# ========================================

# A plugin runner that allows plugins to emit :log but nothing else.
# A misbehaving plugin that tries to signal :abort is caught.

(defn run-plugin [plugin data]
  "Run a plugin on data. Plugins may emit :log but nothing else.
   This prevents plugins from aborting the host, yielding control,
   or signaling errors that escape the sandbox."
  (restrict plugin :log)
  (plugin data))

# Well-behaved plugin: only logs
(defn good-plugin [data]
  (fiber/signal :log {:msg "plugin running"})
  (* data 2))

(def plugin-result (run-plugin good-plugin 21))
(display "  good plugin result: ") (print plugin-result)
(assert-eq plugin-result 42 "plugin: well-behaved plugin works")

# Misbehaving plugin: tries to signal :abort
(defn bad-plugin [data]
  (fiber/signal :abort :escape-attempt)
  data)

(def [ok? err] (protect (run-plugin bad-plugin 21)))
(display "  effect violation caught: ") (print err)
# err is {:error :effect-violation :message "..."} — handle by kind:
# (match (get err :error)
#   :effect-violation (display "plugin tried to signal :abort")
#   _ (error err))
(assert-false ok? "plugin: misbehaving plugin caught")
(assert-eq (get err :error) :effect-violation "plugin: effect-violation error")


(print "")
(print "all effects tests passed.")
