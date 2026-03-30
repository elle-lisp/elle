# Filesystem watch tests — event-driven via inotify/kqueue

(def dir "/tmp/elle-watch-test")

# ── Setup ───────────────────────────────────────────────────────────────
(protect (each f in (list-directory dir) (delete-file (string dir "/" f))))
(protect (delete-directory dir))
(create-directory dir)

# ── Basic: create file, receive event ───────────────────────────────────
(def w (watch))
(watch-add w dir)

# Spawn writer and watcher concurrently
(def writer (ev/spawn (fn []
  (ev/sleep 0.05)
  (spit (string dir "/a.txt") "hello"))))

(def watcher (ev/spawn (fn [] (watch-next w))))

(def events (ev/join watcher))
(ev/join writer)

(assert (not (empty? events)) "got events")
(assert (= (get (first events) :kind) :create) "first event is create")
(assert (string/ends-with? (get (first events) :path) "/a.txt") "path ends with a.txt")

# ── Modify event ────────────────────────────────────────────────────────
(def writer2 (ev/spawn (fn []
  (ev/sleep 0.05)
  (spit (string dir "/a.txt") "updated"))))

(def watcher2 (ev/spawn (fn [] (watch-next w))))

(def events2 (ev/join watcher2))
(ev/join writer2)

(assert (not (empty? events2)) "got modify events")
(assert (= (get (first events2) :kind) :modify) "event is modify")

# ── Close and cleanup ──────────────────────────────────────────────────
(watch-close w)
(delete-file (string dir "/a.txt"))
(delete-directory dir)
