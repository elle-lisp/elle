# Filesystem watch tests — event-driven via inotify/kqueue

(def dir "/tmp/elle-watch-test")

# ── Setup ───────────────────────────────────────────────────────────────
(protect (each f in (list-directory dir) (delete-file (string dir "/" f))))
(protect (delete-directory dir))
(create-directory dir)

# ── Basic: create file, receive event ───────────────────────────────────
(def w (watch))
(watch-add w dir)

# Use a subprocess to create the file after a delay — this avoids
# scheduler deadlock on the thread-pool backend (macOS) where both
# ev/sleep and watch-next compete for the same wait() call.
(subprocess/exec "sh" ["-c" (string "sleep 0.1 && echo hello > " dir "/a.txt")])

(def events (watch-next w))

(assert (not (empty? events)) "got events")
# inotify reports :create; kqueue reports :modify (NOTE_WRITE on directory)
(assert (contains? |:create :modify| (get (first events) :kind)) "first event is create or modify")

# ── Modify event ────────────────────────────────────────────────────────
(subprocess/exec "sh" ["-c" (string "sleep 0.1 && echo updated > " dir "/a.txt")])

(def events2 (watch-next w))

(assert (not (empty? events2)) "got modify events")
(assert (= (get (first events2) :kind) :modify) "event is modify")

# ── Close and cleanup ──────────────────────────────────────────────────
(watch-close w)
(delete-file (string dir "/a.txt"))
(delete-directory dir)
