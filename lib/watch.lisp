## lib/watch.lisp — fiber-aware filesystem watcher
##
## Wraps the elle-watch plugin with fiber-friendly polling and
## convenience filters. The plugin does blocking poll; this library
## yields to the scheduler between polls.
##
## Dependencies:
##   - elle-watch plugin loaded via (import "target/release/libelle_watch.so")
##   - ev/sleep from stdlib (fiber scheduler)
##
## Usage:
##   (def watch-plugin (import "target/release/libelle_watch.so"))
##   (def watch ((import "lib/watch.lisp") watch-plugin))
##
##   (def w (watch:start "lib/" {:debounce 300 :filter ".lisp"}))
##   (watch:for-each w (fn [event] (println "changed:" (get event :paths))))
##   (watch:stop w)

(fn [plugin]

  ## ── Helpers ──────────────────────────────────────────────────────────

  (defn matches-filter? [event ext]
    "Check if any path in event matches the extension filter."
    (if (nil? ext)
      true
      (let [[paths (get event :paths)]]
        (any? (fn [p] (string/ends-with? p ext)) paths))))

  ## ── Core ─────────────────────────────────────────────────────────────

  (defn watch/start [path &named debounce filter recursive]
    "Create a watcher on path, return a handle struct.
     :debounce ms (default 500), :filter extension string,
     :recursive bool (default true)."
    (default debounce 500)
    (var opts {:debounce debounce})
    (var watcher (plugin:new opts))
    (var add-opts (if (nil? recursive) {} {:recursive recursive}))
    (plugin:add watcher path add-opts)
    {:watcher watcher :filter filter :plugin plugin})

  (defn watch/next [handle &named timeout]
    "Fiber-aware event poll. Yields to scheduler between polls.
     Returns event struct or nil after timeout (default 60s)."
    (default timeout 60000)
    (var watcher (get handle :watcher))
    (var ext (get handle :filter))
    (var deadline (+ (int (* (clock/monotonic) 1000)) timeout))
    (forever
      (var event (plugin:next watcher))
      (cond
        ((and (not (nil? event)) (matches-filter? event ext))
          (return event))
        ((>= (int (* (clock/monotonic) 1000)) deadline)
          (return nil))
        (true
          (ev/sleep 0.05)))))

  (defn watch/for-each [handle callback]
    "Loop calling callback on each event. Yields between polls.
     Runs until the watcher is closed (watch/next returns nil
     after the watcher handle is invalid)."
    (forever
      (var event (watch/next handle :timeout 60000))
      (when (nil? event)
        (continue))
      (callback event)))

  (defn watch/stop [handle]
    "Close the watcher."
    (plugin:close (get handle :watcher)))

  (defn watch/lisp-files [path &named debounce]
    "Convenience: watch for *.lisp file changes."
    (default debounce 500)
    (watch/start path :debounce debounce :filter ".lisp"))

  ## ── Export struct ───────────────────────────────────────────────────
  {:start      watch/start
   :next       watch/next
   :for-each   watch/for-each
   :stop       watch/stop
   :lisp-files watch/lisp-files})
