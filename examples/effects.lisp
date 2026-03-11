#!/usr/bin/env elle

# Effects — user-defined signals and effect restrictions
#
# Demonstrates:
#   effect              — declaring user-defined signal keywords
#   restrict            — bounding which signals a function may emit
#   restrict (param)    — bounding which signals a callback may emit
#   Early termination   — :abort stops a search the moment a match is found
#   Progress reporting  — :progress lets callers observe long-running work
#   Logging             — :log entries the caller can collect or ignore
#   Plugin sandboxing   — restrict limits what signals a plugin may emit
#
# Effects are flow-control signals. They interrupt execution and let the
# caller decide how to respond. Restricting signals has nothing to do with
# side-effects — a function can mutate state and still be inert.

# ========================================
# 1. Declaring user-defined effects
# ========================================

# Declare three user-defined signal keywords.
# Each becomes a named flow-control interrupt the caller can intercept.
(effect :abort)    # signals early termination — caller catches and uses the value
(effect :progress) # signals a progress update — caller can display or collect
(effect :log)      # signals a log entry — caller decides whether to record it

# ========================================
# 2. Early termination with :abort
# ========================================

# find-first scans a list and signals :abort the moment pred is satisfied.
# The rest of the list is never visited — no wasted work.
(defn find-first [pred xs]
  "Scan xs linearly; signal :abort with the first element satisfying pred."
  (each x xs              # visit each element in order
    (when (pred x)        # stop as soon as pred is true
      # signal :abort with the found value — interrupts the fiber
      (fiber/signal :abort x))))

# drive-search runs find-first in a fiber that catches :abort signals
(defn drive-search []
  "Search [1 3 5 4 7 9] for the first even number."
  (find-first even? [1 3 5 4 7 9]))

# create a fiber whose mask allows :abort to surface
(def search-fiber (fiber/new drive-search |:abort|))

# resume once — the fiber runs until it hits fiber/signal :abort
(fiber/resume search-fiber)

# if the fiber paused, it caught an :abort signal; fiber/value holds the payload
(def found (-> search-fiber fiber/value))
(display "  first even in [1 3 5 4 7 9]: ") # display prompt
(print found)                                # print the found value
(assert (= found 4) "abort: found first even, stopped early")

# ========================================
# 3. Progress reporting
# ========================================

# process-item squares each element and signals :progress after each one.
# The caller drives the fiber and collects the updates.
(defn process-item [item]
  "Square item and signal :progress with the result."
  (def result (* item item))  # compute the square
  # signal :progress — suspends until the caller resumes
  (fiber/signal :progress {:item item :result result})
  result)                     # return the square as the map result

(defn process-items [items]
  "Map process-item over items, signaling :progress after each."
  (map process-item items))   # map drives process-item over the list

# run-progress is the fiber's entry point — named so fiber/new stays readable
(defn run-progress []
  "Entry point: process the demo list."
  (process-items [1 2 3 4 5]))

(def progress-log @[])        # accumulate :progress payloads here

# create a fiber whose mask allows :progress signals to surface
(def pf (fiber/new run-progress |:progress|))

# drive the fiber until it finishes, collecting each :progress signal
(forever
  # resume the fiber — runs until next fiber/signal or completion
  (fiber/resume pf)
  (if (= (fiber/status pf) :paused)
    # fiber paused on a :progress signal — collect the payload
    (push progress-log (fiber/value pf))
    # fiber finished — stop driving
    (break)))

(assert (= (length progress-log) 5) "progress: 5 updates received")
# use -> to drill into the first and last progress entries
(assert (= (-> progress-log (get 0) (get :result)) 1)  "progress: 1^2 = 1")
(assert (= (-> progress-log (get 4) (get :result)) 25) "progress: 5^2 = 25")
(display "  progress log: ") # display prompt
(print progress-log)         # print the log

# ========================================
# 4. Logging — caller decides what to do with log entries
# ========================================

# compute-with-log does a two-step calculation, signaling :log at each step.
# The caller chooses whether to collect, display, or ignore the entries.
(defn compute-with-log [x]
  "Double x, add 10, signaling :log at each step. Returns the result."
  # signal the start — caller may record this
  (fiber/signal :log {:level :info :msg "starting"})
  (def step1 (* x 2))   # double the input
  # signal the intermediate value
  (fiber/signal :log {:level :debug :msg (-> "doubled to " (append (string step1)))})
  (def step2 (+ step1 10))  # add ten
  # signal completion
  (fiber/signal :log {:level :info :msg "done"})
  step2)                # return the final result

# run-logged is the fiber entry point for the collecting caller
(defn run-logged []
  "Entry point: run compute-with-log with input 5."
  (compute-with-log 5))

(def log-entries @[])   # accumulate :log payloads here

# create a fiber whose mask allows :log signals to surface
(def lf (fiber/new run-logged |:log|))

# drive the fiber, collecting each :log signal
(forever
  # resume — runs until next fiber/signal :log or completion
  (fiber/resume lf)
  (if (= (fiber/status lf) :paused)
    # fiber paused on a :log signal — record the entry
    (push log-entries (fiber/value lf))
    # fiber finished — stop
    (break)))

(assert (= (length log-entries) 3) "log: 3 entries collected")
# the fiber's final return value is accessible via fiber/value after completion
(assert (= (fiber/value lf) 20) "log: result is 20")
(display "  log entries: ") # display prompt
(print log-entries)         # print the entries

# caller 2: ignore :log entirely — just call the function directly
# signals that aren't caught propagate up and are dropped at the top level
(def direct-result (compute-with-log 5))  # :log signals go nowhere
(assert (= direct-result 20) "log: same result when logs ignored")
(display "  direct result (logs ignored): ") # display prompt
(print direct-result)                        # print the result

# ========================================
# 5. restrict — protecting iteration from signaling callbacks
# ========================================

# safe-map requires its callback to be inert.
# If f could signal mid-iteration, the partially-built result list
# would be abandoned — restrict makes that a runtime error instead.
(defn safe-map [f xs]
  "Map f over xs. f must be inert — signals mid-iteration corrupt the result."
  # restrict f to emit no signals — enforced at call time
  (restrict f)
  (map f xs))  # map f over xs normally

# square is an inert callback — no signals, just arithmetic
(defn square [x]
  "Return x squared."
  (* x x))  # pure arithmetic, no signals

(def squares (safe-map square [1 2 3 4 5]))  # works fine
(assert (= squares [1 4 9 16 25]) "safe-map: inert callback works")
(display "  squares: ") # display prompt
(print squares)         # print the squares

# noisy-double signals :log mid-iteration — violates the restrict bound
(defn noisy-double [x]
  "Double x, but also signal :log — not inert."
  # this signal will be caught by restrict and turned into :effect-violation
  (fiber/signal :log {:msg "doubling"})
  (* x 2))  # the multiplication never completes

# protect catches the :effect-violation error as data
(def [ok? err] (protect (safe-map noisy-double [1 2 3])))
(display "  effect violation: ") # display prompt
(print err)                       # print the error
# err is {:error :effect-violation :message "..."} — inspect with ->
(assert (not ok?) "safe-map: signaling callback rejected")
(assert (= (-> err (get :error)) :effect-violation) "safe-map: effect-violation error")

# ========================================
# 6. restrict — plugin sandboxing
# ========================================

# run-plugin allows plugins to emit :log but nothing else.
# A plugin that tries to signal :abort cannot escape the sandbox.
(defn run-plugin [plugin data]
  "Run plugin on data. plugin may emit :log only — all other signals are violations."
  # restrict plugin to :log — any other signal becomes :effect-violation
  (restrict plugin :log)
  (plugin data))  # call the plugin with its input

# good-plugin only logs — stays within its allowed signal set
(defn good-plugin [data]
  "A well-behaved plugin: logs its activity, returns a result."
  # :log is allowed by run-plugin's restrict bound
  (fiber/signal :log {:msg "plugin running"})
  (* data 2))  # return a result

(def plugin-result (run-plugin good-plugin 21))  # 21 * 2 = 42
(assert (= plugin-result 42) "plugin: well-behaved plugin works")
(display "  good plugin result: ") # display prompt
(print plugin-result)              # print the result

# bad-plugin tries to signal :abort — outside its allowed set
(defn bad-plugin [data]
  "A misbehaving plugin: attempts to signal :abort to escape the sandbox."
  # :abort is not in the allowed set — this becomes :effect-violation
  (fiber/signal :abort :escape-attempt)
  data)  # never reached

# protect catches the sandbox violation as data
(def [ok? err] (protect (run-plugin bad-plugin 21)))
(display "  sandbox violation: ") # display prompt
(print err)                        # print the error
# err is {:error :effect-violation :message "..."} — inspect with ->
(assert (not ok?) "plugin: misbehaving plugin caught")
(assert (= (-> err (get :error)) :effect-violation) "plugin: effect-violation error")

(print "")                                    # blank line
(print "all effects tests passed.")           # final message
