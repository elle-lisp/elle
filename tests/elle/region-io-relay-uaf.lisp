(elle/epoch 13)
# audited: 2026-09-28
# A fiber that relays a child's io park with `(emit :io v)` owns its reference
# to `v`, so the install that answers the relay owes the request no release
# (docs/impl/region/park.md).
#
# The trap: the relaying fiber parks under `SIG_IO`, and its payload IS an
# `IoRequest`. Both readings that mark a park as runtime-built see exactly what a
# yielding io op leaves. The request lives in the child's region, and so does
# what the op answers into: the port `port/open` fills, the buffer a read fills.
# Each install that takes the relay for an io park releases that region once
# more, so the child reads its port and its lines out of a freed region.
#
# The counter-factual: a child whose later reads come back whole looks correct
# while the region is freed, because nothing has reused the pages yet. The
# witnesses below therefore run on after the relay: they close the port, open
# another, write, and read the file back. A freed request then reaches the
# scheduler as some other value — a write sends the display text of a stale
# heap object, or the submit refuses a request whose port is no port. Under
# `--trace=guardfree` the first stale read faults instead.

# ── the relays ───────────────────────────────────────────────────────────────
# The relaying body catches the child's io park, raises the same request to its
# own scheduler, and answers the child with what came back.
(defn relay [f v]
  (if (= (fiber/status f) :dead)
    v
    (relay f (fiber/resume f (emit :io v)))))

(defn relayed [body]
  (let [f (fiber/new body |:io|)]
    (relay f (fiber/resume f))))

# Two relaying fibers between the child and the scheduler: every level is an
# install that must owe the request nothing.
(defn relayed-twice [body]
  (relayed (fn [] (relayed body))))

# The relaying fiber reads each request it relayed AFTER the install that
# answered it. Its binding of the request is a counted reference of its own.
(defn relay-reading [f v]
  (if (= (fiber/status f) :dead)
    v
    (let [next (fiber/resume f (emit :io v))]
      (assert (= (type-of v) :io-request)
              "a relayed request was freed under the fiber that relayed it")
      (assert (string/starts-with? (string v) "#<io-request")
              "a relayed request reads back as some other value")
      (relay-reading f next))))

(defn relayed-reading [body]
  (let [f (fiber/new body |:io|)]
    (relay-reading f (fiber/resume f))))

# The control: the child's io goes straight to the scheduler, with no relay.
(defn direct [body]
  (fiber/resume (fiber/new body |:error|)))

# ── the child bodies ─────────────────────────────────────────────────────────
# The port `port/open` answers with lives in the region of the open's request.
(defn open-read-close [src]
  (fn []
    (let [p (port/open src :read)
          line (port/read-line p)]
      (port/close p)
      line)))

# The repro shape: open, close, then write. The write's request is built after
# the close, so a freed region reaches the scheduler inside it.
(defn write-after-close [src dst n]
  (fn []
    (let [p (port/open src :read)]
      (port/close p)
      (let [out (port/open dst :write)]
        (port/write out (string "payload " n))
        (port/close out)
        :written))))

# A read answers in a buffer that lives in the region of its own request. Both
# lines are held across the reads and the timer that follow them.
(defn held-lines [src]
  (fn []
    (let [p (port/open src :read)
          a (port/read-line p)
          b (port/read-line p)]
      (ev/sleep 0)
      (port/close p)
      [a b])))

# A timer allocates nothing the child reads back, so this body reads only what
# it builds after the relays.
(defn sleeper [n]
  (fn []
    (ev/sleep 0)
    (ev/sleep 0)
    @[n (string "slept " n)]))

# ── drive: fresh parks per round keep region ids churning, so a recycled id
# detonates on its generation stamp rather than reading stale bytes.
(def rounds 40)

(defn check-round [src dir run label n]
  (assert (= (run (open-read-close src)) "first")
          (string label ": the port an open answered with was freed"))
  (let [dst (path/join dir (string label "-" n ".out"))]
    (assert (= (run (write-after-close src dst n)) :written)
            (string label ": a write after a close did not complete"))
    (assert (= (file/read dst) (string "payload " n))
            (string label ": a write after a close sent other bytes: "
                    (file/read dst))))
  (assert (= (run (held-lines src)) ["first" "second"])
          (string label ": a line held across later parks was freed"))
  (let [r (run (sleeper n))]
    (assert (= (get r 1) (string "slept " n))
            (string label ": a body's own value was freed after a relay"))))

(with-temp-dir dir
               (let [src (path/join dir "lines.txt")]
                 (file/write src "first\nsecond\n")
                 (def @i 0)
                 (while (%lt i rounds)
                   (check-round src dir direct "direct" i)
                   (check-round src dir relayed "relayed" i)
                   (check-round src dir relayed-twice "relayed-twice" i)
                   (check-round src dir relayed-reading "relayed-reading" i)
                   (assign i (%add i 1)))))

# ── the leak face ────────────────────────────────────────────────────────────
# A relay must not answer by releasing nothing at any io install. The child's
# own install still owes its request the release, so a relayed timer must stay
# as bounded as a direct one.
(def window 200)

(defn measure [thunk warm n]
  (def @i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (def @j 0)
  (while (%lt j n)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(def direct-d (measure (fn [] (direct (sleeper 0))) 30 window))
(def relayed-d (measure (fn [] (relayed (sleeper 0))) 30 window))
(def twice-d (measure (fn [] (relayed-twice (sleeper 0))) 30 window))

# A stranded request is at least one object per relayed park, and each round
# parks twice, so a strand reads at least 400 over the window.
(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(bounded? direct-d "control: a timer with no relay")
(bounded? relayed-d "a timer relayed once")
(bounded? twice-d "a timer relayed twice")

(println "region-io-relay-uaf: ok")
