(elle/epoch 9)
## lib/watch.lisp — event-driven filesystem watcher
##
## Uses the built-in watch primitives (inotify on Linux, kqueue on macOS).
## No polling — watch-next yields to the scheduler and resumes when the
## kernel delivers filesystem events.
##
## Usage:
##   (def w ((import "std/watch")))
##   (def h (w:start "lib/" :filter ".lisp"))
##   (w:each h (fn [event] (println "changed:" (get event :path))))
##   (w:stop h)

(fn []

  ## ── Helpers ──────────────────────────────────────────────────────────

  (defn matches-filter? [event ext]
    "Check if the event path matches the extension filter."
    (if (nil? ext) true (string/ends-with? (get event :path) ext)))

  (defn filter-events [events ext]
    "Filter event batch by extension. Returns list (may be empty)."
    (if (nil? ext)
      events
      (filter (fn [e] (matches-filter? e ext)) events)))

  ## ── Core ─────────────────────────────────────────────────────────────

  (defn start [path &named filter @recursive]
    "Create a watcher on path, return a handle struct.
     :filter extension string (e.g. \".lisp\"),
     :recursive bool (default true)."
    (default recursive true)
    (def w (watch))
    (watch-add w path {:recursive recursive})
    {:watcher w :filter filter})

  (defn next-events [handle]
    "Wait for filesystem events. Yields to the scheduler (zero polling).
     Returns a list of event structs [{:kind :modify :path \"...\"}].
     Filters by extension if :filter was set on the handle."
    (let [raw (watch-next (get handle :watcher))
          ext (get handle :filter)]
      (filter-events raw ext)))

  (defn each-event [handle callback]
    "Loop calling callback on each event. Yields between batches.
     Runs forever until the watcher is closed."
    (forever
      (let [events (next-events handle)]
        (each event in events
          (callback event)))))

  (defn stop [handle]
    "Close the watcher."
    (watch-close (get handle :watcher)))

  ## ── Export ───────────────────────────────────────────────────────────
  {:start start :next next-events :each each-event :stop stop})
