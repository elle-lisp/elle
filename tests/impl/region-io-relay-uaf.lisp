(elle/epoch 13)
# audited: 2026-09-29
# A fiber that relays a child's io park with `(emit :io v)` owns its reference to `v`, so the install owes the request nothing.
# docs/impl/region/park.md
#
# The trap: the relaying fiber parks under `SIG_IO`, and its payload IS an
# `IoRequest`. Both readings that mark a park as runtime-built see exactly what
# a yielding io op leaves. The request lives in the child's region, and so does
# what the op answers into: the port `port/open` fills, the buffer a read
# fills. Each install that takes the relay for an io park releases that region
# once more, so the child reads its port and its lines out of a freed region.
#
# The counter-factual: a child whose later reads come back whole looks correct
# while the region is freed, because nothing has reused the pages yet. The
# witnesses below therefore run on after the relay: they close the port, open
# another, write, and read the file back. A freed request then reaches the
# scheduler as some other value — a write sends the display text of a stale
# heap object, or the submit refuses a request whose port is no port. The
# sidecar arms guardfree, so the first stale read faults instead.

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

# The relaying body raises the request through the `emit` primitive: a keyword
# in a parameter is no literal, so the compiler lowers no `Emit` node. The park
# is the primitive's, and the request is one of the call's own arguments, so
# the body owns a reference to it exactly as a literal `emit` does.
(defn relay-dynamic [f v kw]
  (if (= (fiber/status f) :dead)
    v
    (relay-dynamic f (fiber/resume f (emit kw v)) kw)))

(defn relayed-dynamic [body]
  (let [f (fiber/new body |:io|)]
    (relay-dynamic f (fiber/resume f) :io)))

# The same raise from a tail call, which parks through the tail arm.
(defn raise [kw v]
  (emit kw v))

(defn relay-tail [f v kw]
  (if (= (fiber/status f) :dead)
    v
    (relay-tail f (fiber/resume f (raise kw v)) kw)))

(defn relayed-tail [body]
  (let [f (fiber/new body |:io|)]
    (relay-tail f (fiber/resume f) :io)))

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
                   (check-round src dir relayed-dynamic "relayed-dynamic" i)
                   (check-round src dir relayed-tail "relayed-tail" i)
                   (assign i (%add i 1)))))

# ── a stale request ──────────────────────────────────────────────────────────
# A request is good only while the park that raised it stands
# (docs/impl/region/park.md). Each relay below raises the child's request after
# that park is over: the child released at its last use, the request carried
# out of the function that held the child, or the child resumed past the
# write. The raise fails with `:state-error`, and the write never runs.
#
# The counter-factual: nothing counts the port or the payload from the
# request, so an unchecked raise submits a request whose values lie in freed
# pages. `churn` claims those pages again before the raise, so the write fails
# or sends another value's bytes. Under `--trace=guardfree` the submit faults
# instead. A relay that the check let through would also leave bytes in the
# file, which each round reads back empty.

(defn churn []
  (def @kept @[])
  (def @i 0)
  (while (%lt i 64)
    (push kept (string "churn " i))
    (assign i (%add i 1)))
  (length kept))

# The payload is born in the child; the port is the caller's.
(defn write-payload [out n]
  (fn []
    (port/write out (string "payload " n))
    :written))

# The port and the payload are both born in the child, which parks first on
# the open and then on the write.
(defn open-and-write [dst n]
  (fn []
    (let [out (port/open dst :write)]
      (port/write out (string "payload " n))
      (port/close out)
      :written)))

# Each relay answers with what its raise of the stale request did: the pair
# `protect` makes of it.

# The `let` form: the child's last use is the resume that answers the write.
(defn relay-let [body]
  (let [f (fiber/new body |:io|)
        q (fiber/resume f)]
    (churn)
    (protect (emit :io q))))

# The `def` form of the same relay.
(defn relay-def [body]
  (def f (fiber/new body |:io|))
  (def q (fiber/resume f))
  (churn)
  (protect (emit :io q)))

# The same relay, raising through the `emit` primitive: a keyword in a
# parameter is no literal.
(defn relay-let-kw [body kw]
  (let [f (fiber/new body |:io|)
        q (fiber/resume f)]
    (churn)
    (protect (emit kw q))))

(defn relay-let-dynamic [body]
  (relay-let-kw body :io))

# Both binding forms again, relaying the child's open first so that the request
# bound for the write names a port the child opened.
(defn relay-let-second [body]
  (let [f (fiber/new body |:io|)
        opened (emit :io (fiber/resume f))
        q (fiber/resume f opened)]
    (churn)
    (protect (emit :io q))))

(defn relay-def-second [body]
  (def f (fiber/new body |:io|))
  (def opened (emit :io (fiber/resume f)))
  (def q (fiber/resume f opened))
  (churn)
  (protect (emit :io q)))

# The request leaves the function that held the child, by return.
(defn grab [body]
  (let [f (fiber/new body |:io|)]
    (fiber/resume f)))

(defn relay-returned [body]
  (let [q (grab body)]
    (churn)
    (protect (emit :io q))))

# The request leaves by a store into a container the caller holds.
(defn stash [box body]
  (let [f (fiber/new body |:io|)]
    (push box (fiber/resume f))
    nil))

(defn relay-stored [body]
  (let [box @[]]
    (stash box body)
    (churn)
    (protect (emit :io (get box 0)))))

# The child is answered without its write running, and runs to its end. The
# fiber is still reachable, which is why a check on its lifetime alone would
# pass this raise: its frames released the payload when the write returned.
(defn relay-resumed [body]
  (let [f (fiber/new body |:io|)
        q (fiber/resume f)]
    (fiber/resume f 0)
    (churn)
    (let [raised (protect (emit :io q))]
      (assert (= (fiber/status f) :dead) "the resumed child ran to its end")
      raised)))

(defn refused? [raised label]
  (let [[ok? err] raised]
    (assert (not ok?) (string label ": a stale request was spent"))
    (assert (= (get err :error) :state-error)
            (string label ": a stale request raised " err))))

(defn check-stale-round [dir n]
  (each [label relay] [["relay-let" relay-let] ["relay-def" relay-def]
                       ["relay-let-dynamic" relay-let-dynamic]
                       ["relay-returned" relay-returned]
                       ["relay-stored" relay-stored]
                       ["relay-resumed" relay-resumed]]
    (let [dst (path/join dir (string label "-" n ".out"))
          out (port/open dst :write)]
      (refused? (relay (write-payload out n)) label)
      (port/close out)
      (assert (= (file/read dst) "")
              (string label ": a stale write sent bytes: " (file/read dst)))))
  (each [label relay] [["relay-let-second" relay-let-second]
                       ["relay-def-second" relay-def-second]]
    (let [dst (path/join dir (string label "-" n ".out"))]
      (refused? (relay (open-and-write dst n)) label)
      (assert (= (file/read dst) "")
              (string label ": a stale write sent bytes: " (file/read dst))))))

(with-temp-dir dir
               (let [dst (path/join dir "top.out")
                     out (port/open dst :write)]
                 (def @i 0)
                 (while (%lt i rounds)
                   (check-stale-round dir i)
                   (assign i (%add i 1)))
                 # The `def` relay written as the block's own bindings, with no
                 # function around them.
                 (def f (fiber/new (write-payload out "top") |:io|))
                 (def q (fiber/resume f))
                 (churn)
                 (refused? (protect (emit :io q))
                           "a relay at the top of a block")
                 (port/close out)
                 (assert (= (file/read dst) "")
                         (string "a stale write at the top of a block sent "
                                 "bytes: " (file/read dst)))))

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
(def dynamic-d (measure (fn [] (relayed-dynamic (sleeper 0))) 30 window))
(def tail-d (measure (fn [] (relayed-tail (sleeper 0))) 30 window))

# A stranded request is at least one object per relayed park, and each round
# parks twice, so a strand reads at least 400 over the window.
(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(bounded? direct-d "control: a timer with no relay")
(bounded? relayed-d "a timer relayed once")
(bounded? twice-d "a timer relayed twice")
(bounded? dynamic-d "a timer relayed through the emit primitive")
(bounded? tail-d "a timer relayed through a tail emit")

# A refused raise builds an error, and the relay drops it. Neither that error
# nor the stale request may outlive the round, so a stale relay must stay as
# bounded as the others.
(defn measure-stale [out]
  (measure (fn [] (relay-let (write-payload out 0))) 30 window))

(with-temp-dir dir
               (let [out (port/open (path/join dir "gauge.out") :write)]
                 (bounded? (measure-stale out)
                           "a stale request raised after its child is released")
                 (port/close out)))

(println "region-io-relay-uaf: ok")
