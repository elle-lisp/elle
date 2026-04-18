(elle/epoch 8)
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
  (ev/sleep 0.1)
  (spit (string dir "/a.txt") "hello"))))

(def watcher (ev/spawn (fn [] (watch-next w))))

(def events (ev/join watcher))
(ev/join writer)

(assert (not (empty? events)) "got events")
# inotify reports :create; kqueue reports :modify (NOTE_WRITE on directory)
(assert (contains? |:create :modify| (get (first events) :kind)) "event is create or modify")

# ── Second event: create another file ──────────────────────────────────
# Use a new file rather than overwriting — kqueue EVFILT_VNODE on a
# directory only fires for entry changes (create/delete/rename), not
# for content modifications to existing files.
(def writer2 (ev/spawn (fn []
  (ev/sleep 0.1)
  (spit (string dir "/b.txt") "world"))))

(def watcher2 (ev/spawn (fn [] (watch-next w))))

(def events2 (ev/join watcher2))
(ev/join writer2)

(assert (not (empty? events2)) "got second event")

# ── Close and cleanup ──────────────────────────────────────────────────
(watch-close w)
(delete-file (string dir "/a.txt"))
(delete-file (string dir "/b.txt"))
(delete-directory dir)
