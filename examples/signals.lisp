#!/usr/bin/env elle

# Signals — user-defined signals and signal restrictions
#
# Demonstrates:
#   signal              — declaring user-defined signal keywords
#   silence             — bounding which signals a function may emit
#   silence (param)     — bounding which signals a callback may emit
#   Early termination   — :abort stops a search the moment a match is found
#   Progress reporting  — :progress lets callers observe long-running work
#   Logging             — :log entries the caller can collect or ignore
#   Plugin sandboxing   — silence limits what signals a plugin may emit
#
# Signals are flow-control interrupts. They suspend execution and let the
# caller decide how to respond. Restricting signals has nothing to do with
# side-effects — a function can mutate state and still be inert.

# ========================================
# 1. Declaring user-defined signals
# ========================================

# Declare four user-defined signal keywords.
# Each becomes a named flow-control interrupt the caller can intercept.
(signal :abort)    # signals early termination — caller catches and uses the value
(signal :progress) # signals a progress update — caller can display or collect
(signal :log)      # signals a log entry — caller decides whether to record it
(signal :audit)    # signals an audit event — caller can log for compliance

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
      (emit :abort x))))

# drive-search runs find-first in a fiber that catches :abort signals
(defn drive-search []
  "Search [1 3 5 4 7 9] for the first even number."
  (find-first even? [1 3 5 4 7 9]))

# create a fiber whose mask allows :abort to surface
(def search-fiber (fiber/new drive-search |:abort|))

# resume once — the fiber runs until it hits emit :abort
(fiber/resume search-fiber)

# if the fiber paused, it caught an :abort signal; fiber/value holds the payload
(def found (fiber/value search-fiber))
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
  (emit :progress {:item item :result result})
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
  # resume the fiber — runs until next emit or completion
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
  (emit :log {:level :info :msg "starting"})
  (def step1 (* x 2))   # double the input
  # signal the intermediate value
  (emit :log {:level :debug :msg (-> "doubled to " (append (string step1)))})
  (def step2 (+ step1 10))  # add ten
  # signal completion
  (emit :log {:level :info :msg "done"})
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
  # resume — runs until next emit :log or completion
  (fiber/resume lf)
  (match (fiber/status lf)
         # fiber paused on :log — record the entry
         (:paused (push log-entries (fiber/value lf)))
         (_ (break))))  # fiber finished — stop driving

(assert (= (length log-entries) 3) "log: 3 entries collected")
# the fiber's final return value is accessible via fiber/value after completion
(assert (= (fiber/value lf) 20) "log: result is 20")
(display "  log entries: ") # display prompt
(print log-entries)         # print the entries

# caller 2: ignore :log entirely — run in a fiber that catches :log but
# discards the payloads, driving to completion for the return value
(defn run-logged-ignore []
  "Entry point: run compute-with-log, ignoring :log signals."
  (compute-with-log 5))

(def lf2 (fiber/new run-logged-ignore |:log|))
(forever
  (fiber/resume lf2)
  (if (not (= (fiber/status lf2) :paused))
    (break)))  # keep resuming until done, discarding :log payloads

(def direct-result (fiber/value lf2))
(assert (= direct-result 20) "log: same result when logs ignored")
(display "  direct result (logs ignored): ") # display prompt
(print direct-result)                        # print the result

# ========================================
# 5. silence — protecting iteration from signaling callbacks
# ========================================

# safe-map requires its callback to be inert.
# If f could signal mid-iteration, the partially-built result list
# would be abandoned — silence makes that a runtime error instead.
(defn safe-map [f xs]
  "Map f over xs. f must be inert — signals mid-iteration corrupt the result."
  # silence f to emit no signals — enforced at call time
  (silence f)
  (map f xs))  # map f over xs normally

# square is an inert callback — no signals, just arithmetic
(defn square [x]
  "Return x squared."
  (* x x))  # pure arithmetic, no signals

(def squares (safe-map square [1 2 3 4 5]))  # works fine
(assert (= squares (list 1 4 9 16 25)) "safe-map: inert callback works")
(display "  squares: ") # display prompt
(print squares)         # print the squares

# noisy-double signals :log mid-iteration — violates the silence bound
(defn noisy-double [x]
  "Double x, but also signal :log — not inert."
  # this signal will be caught by silence and turned into :signal-violation
  (emit :log {:msg "doubling"})
  (* x 2))  # the multiplication never completes

# protect catches the :signal-violation error as data
(def [ok? err] (protect (safe-map noisy-double [1 2 3])))
(display "  signal violation: ") # display prompt
(print err)                       # print the error
# err is {:error :signal-violation :message "..."} — inspect with ->
(assert (not ok?) "safe-map: signaling callback rejected")
(assert (= (-> err (get :error)) :signal-violation) "safe-map: signal-violation error")

# match on the error kind to handle different violations distinctly
(match err:error  # syntax-sugar for (get err :error)
  (:signal-violation (display "  safe-map: callback tried to signal mid-iteration\n"))
  (_ (error err)))

# ========================================
# 6. silence — ensuring callbacks are inert
# ========================================

# run-pure requires its plugin to be inert — no signals at all.
# This is the strongest guarantee: the plugin cannot interrupt execution.
(defn run-pure [plugin data]
  "Run plugin on data. plugin must be inert — no signals allowed."
  (silence plugin)
  (plugin data))

# good-plugin is inert — just arithmetic, no signals
(defn good-plugin [data]
  "A well-behaved plugin: pure computation, returns a result."
  (* data 2))

(def plugin-result (run-pure good-plugin 21))
(assert (= plugin-result 42) "plugin: inert plugin works")
(display "  good plugin result: ") # display prompt
(print plugin-result)              # print the result

# bad-plugin yields — violates the inert restriction
(defn bad-plugin [data]
  "A misbehaving plugin: attempts to yield, violating the inert bound."
  (yield :escape-attempt)
  data)  # never reached

# protect catches the :signal-violation error as data
(def [ok? err] (protect (run-pure bad-plugin 21)))
(display "  sandbox violation: ") # display prompt
(print err)                        # print the error
(assert (not ok?) "plugin: yielding plugin caught")
(assert (= (-> err (get :error)) :signal-violation) "plugin: signal-violation error")

(match err:error
  (:signal-violation (display "  plugin: callback tried to yield — rejected by silence\n"))
  (_ (error err)))

# ========================================
# 7. Signal composition — combining bits
# ========================================

# sensitive-op performs a sensitive operation and signals both :log and :audit
# simultaneously — one emission, two bits set in the signal mask.
(defn sensitive-op [data]
  "Perform a sensitive operation, signaling both :log and :audit."
  # emit both bits simultaneously — one signal, two meanings
  (emit |:log :audit| {:op :write :data data :user "alice"})
  (string "processed: " data))  # return a string describing the result

# run-sensitive is the entry point for the sensitive operation fiber
(defn run-sensitive []
  "Entry point for the sensitive operation fiber."
  (sensitive-op "secret"))  # call sensitive-op with demo data

# a monitor fiber whose mask includes :log — it catches any signal with :log set,
# including composed |:log :audit| signals because the mask check is bitwise
(def monitor (fiber/new run-sensitive |:log|))

# resume the monitor fiber — runs until it hits the composed signal
(fiber/resume monitor)

# the fiber paused on the composed signal — inspect the signal payload
(def sig-val (fiber/value monitor))
(display "  composed signal payload: ")  # display prompt
(print sig-val)                           # print the signal payload

# verify the payload contains both the operation and user fields
(assert (= (-> sig-val (get :op)) :write) "composition: op field present")
(assert (= (-> sig-val (get :user)) "alice") "composition: user field present")
(assert (= (-> sig-val (get :data)) "secret") "composition: data field present")

# demonstrate that the same composed signal is caught by different masks.
# a fiber with mask |:audit| would also catch |:log :audit| because the mask
# check is: does the signal contain any bit that the mask contains?
# |:log :audit| contains :audit, so a mask of |:audit| catches it.
# |:log :audit| contains :log, so a mask of |:log| catches it.
# |:log :audit| contains both, so a mask of |:log :audit| catches it.
# this is the power of bitmask composition — one signal, multiple observers.

# run-sensitive-again is another entry point for a second monitor
(defn run-sensitive-again []
  "Entry point for a second sensitive operation fiber."
  (sensitive-op "another-secret"))  # call sensitive-op again

# an audit-focused monitor whose mask includes :audit — it also catches the
# composed |:log :audit| signal because :audit is in the signal
(def audit-monitor (fiber/new run-sensitive-again |:audit|))

# resume the audit monitor — runs until it hits the composed signal
(fiber/resume audit-monitor)

# the audit monitor also paused on the same composed signal
(def audit-sig-val (fiber/value audit-monitor))
(display "  audit monitor signal: ")  # display prompt
(print audit-sig-val)                  # print the audit signal

# verify the audit monitor received the same payload
(assert (= (-> audit-sig-val (get :op)) :write) "audit: op field present")
(assert (= (-> audit-sig-val (get :user)) "alice") "audit: user field present")

(print "")                                    # blank line
(print "all signals tests passed.")           # final message
