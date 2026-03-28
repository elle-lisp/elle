## Watch plugin integration tests

(def [ok? w] (protect (import "target/release/libelle_watch.so")))
(when (not ok?)
  (print "SKIP: watch plugin not built\n")
  (exit 0))

## ── Create and close ────────────────────────────────────────────────

(var watcher (w:new))
(assert (not (nil? watcher)) "new returns a watcher")
(w:close watcher)

## ── next on closed watcher returns nil ──────────────────────────────

(var event (w:next watcher))
(assert (nil? event) "next on closed watcher returns nil")

## ── Watch a temp directory and detect file creation ─────────────────

(var tmp-dir (string "/tmp/elle-watch-test-" (int (* (clock/monotonic) 1000000))))
(file/mkdir-all tmp-dir)

(var watcher2 (w:new {:debounce 100}))
(w:add watcher2 tmp-dir)

## Write a file to trigger an event
(var test-file (string tmp-dir "/test.lisp"))
(file/write test-file "(println :hello)")

## Poll with timeout — debouncer needs time to flush
(var event2 (w:next watcher2 {:timeout 2000}))
(assert (not (nil? event2)) "detected file write event")
(assert (= (get event2 :kind) :modify) "event kind is modify")

(var paths (get event2 :paths))
(assert (not (empty? paths)) "event has paths")
(assert (string/ends-with? (first paths) "test.lisp") "path matches written file")

## ── Remove path ─────────────────────────────────────────────────────

(w:remove watcher2 tmp-dir)

## Write another file — should NOT generate an event
(file/write (string tmp-dir "/test2.lisp") "(println :bye)")
(var event3 (w:next watcher2 {:timeout 500}))
(assert (nil? event3) "no event after remove")

## ── Cleanup ─────────────────────────────────────────────────────────

(w:close watcher2)
(file/delete (string tmp-dir "/test.lisp"))
(file/delete (string tmp-dir "/test2.lisp"))
(file/delete-dir tmp-dir)

(println "watch: all tests passed")
