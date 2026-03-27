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
# side-effects — a function can mutate state and still be silent.
## ── Declaring user-defined signals ─────────────────────────────────

# Declare four user-defined signal keywords.
# Each becomes a named flow-control interrupt the caller can intercept.
(signal :abort)    # signals early termination — caller catches and uses the value
(signal :progress) # signals a progress update — caller can display or collect
(signal :log)      # signals a log entry — caller decides whether to record it
(signal :audit)    # signals an audit event — caller can log for compliance
## ── Early termination with :abort ──────────────────────────────────

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
(resume search-fiber)

# if the fiber paused, it caught an :abort signal; fiber/value holds the payload
(def found (fiber/value search-fiber))
(print "  first even in [1 3 5 4 7 9]: ") # display prompt
(println found)                                # print the found value
(assert (= found 4) "abort: found first even, stopped early")
## ── Progress reporting ─────────────────────────────────────────────

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
  (resume pf)
  (if (= (fiber/status pf) :paused)
    # fiber paused on a :progress signal — collect the payload
    (push progress-log (fiber/value pf))
    # fiber finished — stop driving
    (break)))

(assert (= (length progress-log) 5) "progress: 5 updates received")
# use -> to drill into the first and last progress entries
(assert (= (-> progress-log (get 0) (get :result)) 1)  "progress: 1^2 = 1")
(assert (= (-> progress-log (get 4) (get :result)) 25) "progress: 5^2 = 25")
(print "  progress log: ") # display prompt
(println progress-log)         # print the log
## ── Logging — caller decides what to do with log entries ───────────

# compute-with-log does a two-step calculation, signaling :log at each step.
# The caller chooses whether to collect, display, or ignore the entries.
(defn compute-with-log [x]
  "Double x, add 10, signaling :log at each step. Returns the result."
  # signal the start — caller may record this
  (emit :log {:level :info :msg "starting"})
  (def step1 (* x 2))   # double the input
  # signal the intermediate value
  (emit :log {:level :debug :msg (string "doubled to " step1)})
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
  (resume lf)
  (match (fiber/status lf)
         # fiber paused on :log — record the entry
         (:paused (push log-entries (fiber/value lf)))
         (_ (break))))  # fiber finished — stop driving

(assert (= (length log-entries) 3) "log: 3 entries collected")
# the fiber's final return value is accessible via fiber/value after completion
(assert (= (fiber/value lf) 20) "log: result is 20")
(print "  log entries: ") # display prompt
(println log-entries)         # print the entries

# caller 2: ignore :log entirely — run in a fiber that catches :log but
# discards the payloads, driving to completion for the return value
(defn run-logged-ignore []
  "Entry point: run compute-with-log, ignoring :log signals."
  (compute-with-log 5))

(def lf2 (fiber/new run-logged-ignore |:log|))
(forever
  (resume lf2)
  (if (not (= (fiber/status lf2) :paused))
    (break)))  # keep resuming until done, discarding :log payloads

(def direct-result (fiber/value lf2))
(assert (= direct-result 20) "log: same result when logs ignored")
(print "  direct result (logs ignored): ") # display prompt
(println direct-result)                        # print the result
## ── silence — protecting iteration from signaling callbacks ────────

# safe-map requires its callback to be silent.
# If f could signal mid-iteration, the partially-built result list
# would be abandoned — silence makes that a runtime error instead.
(defn safe-map [f xs]
  "Map f over xs. f must be silent — signals mid-iteration corrupt the result."
  # silence f to emit no signals — enforced at call time
  (silence f)
  (map f xs))  # map f over xs normally

# square is an silent callback — no signals, just arithmetic
(defn square [x]
  "Return x squared."
  (* x x))  # pure arithmetic, no signals

(def squares (safe-map square [1 2 3 4 5]))  # works fine
(assert (= squares [1 4 9 16 25]) "safe-map: silent callback works")
(print "  squares: ") # display prompt
(println squares)         # print the squares

# noisy-double signals :log mid-iteration — violates the silence bound
(defn noisy-double [x]
  "Double x, but also signal :log — not silent."
  # this signal will be caught by silence and turned into :signal-violation
  (emit :log {:msg "doubling"})
  (* x 2))  # the multiplication never completes

# protect catches the :signal-violation error as data
(def [ok? err] (protect (safe-map noisy-double [1 2 3])))
(print "  signal violation: ") # display prompt
(println err)                       # print the error
# err is {:error :signal-violation :message "..."} — inspect with ->
(assert (not ok?) "safe-map: signaling callback rejected")
(assert (= (-> err (get :error)) :signal-violation) "safe-map: signal-violation error")

# match on the error kind to handle different violations distinctly
(match err:error  # syntax-sugar for (get err :error)
  (:signal-violation (print "  safe-map: callback tried to signal mid-iteration\n"))
  (_ (error err)))

# squelch forbids specific signals (blacklist), allowing everything else.
# silence is a total suppressor — (silence f) means f must emit nothing at all.
# squelch is ideal for composition: "f must not yield, but may error."

# safe-iterate requires its callback to not yield.
# Errors (e.g. bad input) are still allowed to propagate — only yielding is forbidden.
# (squelch f :yield) returns a new closure; we call map with the squelched version.
(defn safe-iterate [f xs]
  "Iterate f over xs. f must not yield — errors are still allowed."
  (let ((safe-f (squelch f :yield)))
    (map safe-f xs)))

# double is a silent callback — pure arithmetic, no signals
(defn double [x]
  "Return x doubled."
  (* x 2))

# safe-iterate accepts double because double has no :yield signal
(def doubled-results (safe-iterate double [1 2 3]))
(assert (= doubled-results [2 4 6]) "squelch: silent callback passes")
(print "  doubled results: ")  # display prompt
(println doubled-results)          # print the results

# yielding-callback yields — violates the squelch bound
(defn yielding-callback [x]
  "Process x but also yield — violates squelch."
  (yield :escape)
  (* x 2))

# safe-iterate rejects yielding-callback because :yield is squelched
(def [ok4? err4] (protect (safe-iterate yielding-callback [1 2 3])))
(print "  squelch violation: ")  # display prompt
(println err4)                         # print the error
(assert (not ok4?) "squelch: yielding callback rejected")
(assert (= (-> err4 (get :error)) :signal-violation) "squelch: signal-violation error")

# match on the error kind to handle the violation
(match err4:error
  (:signal-violation (print "  squelch: callback tried to yield — rejected\n"))
  (_ (error err4)))
## ── silence — ensuring callbacks are silent ────────────────────────

# run-pure requires its plugin to be silent — no signals at all.
# This is the strongest guarantee: the plugin cannot interrupt execution.
# silence is the right tool for sandboxing (total suppression).
# squelch (from section 5) is for composition safety (targeted prohibition).
(defn run-pure [plugin data]
  "Run plugin on data. plugin must be silent — no signals allowed."
  (silence plugin)
  (plugin data))

# good-plugin is silent — just arithmetic, no signals
(defn good-plugin [data]
  "A well-behaved plugin: pure computation, returns a result."
  (* data 2))

(def plugin-result (run-pure good-plugin 21))
(assert (= plugin-result 42) "plugin: silent plugin works")
(print "  good plugin result: ") # display prompt
(println plugin-result)              # print the result

# bad-plugin yields — violates the silent restriction
(defn bad-plugin [data]
  "A misbehaving plugin: attempts to yield, violating the silent bound."
  (yield :escape-attempt)
  data)  # never reached

# protect catches the :signal-violation error as data
(def [ok? err] (protect (run-pure bad-plugin 21)))
(print "  sandbox violation: ") # display prompt
(println err)                        # print the error
(assert (not ok?) "plugin: yielding plugin caught")
(assert (= (-> err (get :error)) :signal-violation) "plugin: signal-violation error")

(match err:error
  (:signal-violation (print "  plugin: callback tried to yield — rejected by silence\n"))
  (_ (error err)))
## ── Signal composition — combining bits ────────────────────────────

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
(resume monitor)

# the fiber paused on the composed signal — inspect the signal payload
(def sig-val (fiber/value monitor))
(print "  composed signal payload: ")  # display prompt
(println sig-val)                           # print the signal payload

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
(resume audit-monitor)

# the audit monitor also paused on the same composed signal
(def audit-sig-val (fiber/value audit-monitor))
(print "  audit monitor signal: ")  # display prompt
(println audit-sig-val)                  # print the audit signal

# verify the audit monitor received the same payload
(assert (= (-> audit-sig-val (get :op)) :write) "audit: op field present")
(assert (= (-> audit-sig-val (get :user)) "alice") "audit: user field present")

(println "")                                    # blank line
(println "all signals tests passed.")           # final message
