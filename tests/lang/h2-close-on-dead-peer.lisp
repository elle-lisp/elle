(elle/epoch 12)
# audited: 2026-09-14
# Closing an h2 session returns even when the peer stopped reading.
#
# The trap is a peer whose socket is open and healthy but whose process
# never reads again. Writes to it do not fail — they block, and they
# block with no deadline, because a TCP send buffer that nobody drains
# has nothing to report. The session's writer fiber parks inside
# port/write and stays there.
#
# The counter-factual is a close that joins that writer: it returns when
# the peer comes back, and a peer that never comes back holds the caller
# for as long as the process lives. Nothing in this file asserts that
# case, because the failure has no value to compare — the budget below
# is what catches it.
#
# See lib/http2/overview.md § Design decisions.

(def http2 ((import "std/http2")))
(def frame ((import "std/http2/frame")))
(def transport ((import "std/http2/transport")))
(def C frame:constants)

# A budget no phase here can reach, reported on expiry so a slow run
# reads differently from a wedged one. tests/integration/budget.rs holds
# it under the corpus per-file budget.
(def deadline 20)

# The largest window RFC 9113 allows. The peer opens both the connection
# and the stream this wide so that flow control never stops the client
# queueing — the socket is what has to fill, not a window.
(def max-window 2147483647)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)]
    [l (parse-int (slice p (+ 1 (string/find p ":"))))]))

(def chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "A body of n bytes by repeated doubling — O(log n) concats."
  (let [@b chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

(defn deaf-peer [listener]
  "Answer the client handshake, open both windows wide, then stop
   reading for the rest of the file. The `defer` is what keeps the
   socket open across the sleep, and the socket staying open is the
   whole point: a closed one would fail the client's writes instead of
   blocking them."
  (let* [tcp (tcp/accept listener)
         t (transport:tcp tcp)]
    (defer
      (protect (port/close tcp))
      (frame:read-exact t 24)
      (frame:read-frame t 16384)
      (let [[ft fl si pl] (frame:make-settings-frame [[C:settings-initial-window-size
            max-window] [C:settings-max-frame-size 16384]
            [C:settings-enable-push 0]])]
        (frame:write-frame t ft fl si pl))
      (let [[ft fl si pl] (frame:make-settings-ack)]
        (frame:write-frame t ft fl si pl))
      (let [[ft fl si pl] (frame:make-window-update-frame 0 (- max-window 65535))]
        (frame:write-frame t ft fl si pl))
      (t:flush)
      # Longer than the budget on purpose. A peer that woke inside it
      # would drain the socket and free the writer, and the close would
      # then return because the peer came back rather than because it
      # stopped waiting — which is the measurement this file exists to
      # avoid making.
      (ev/sleep (* 10 deadline)))))

# Enough to overrun a loopback send buffer and the peer's receive buffer
# together, with room to spare on a kernel tuned larger than this one.
(def wedge-bytes (* 32 1024 1024))

(defn wedge [url]
  "A session whose writer fiber is parked inside port/write.
   Queue `wedge-bytes`, then watch the queue over two turns of the
   scheduler. A queue that does not shrink at all is the proof: the
   writer is inside a write the kernel will not take, rather than merely
   behind. Asserting on one sample instead would pass on a writer that
   is draining normally, which is the setup this file must not have."
  (let [sess (http2:connect url)
        body (make-body (* 1024 1024))]
    (def [sid s] (http2:open-stream sess "POST" "/sink"))
    (each _ in (range 0 (/ wedge-bytes (* 1024 1024)))
      (http2:stream-send sess sid body))
    (ev/sleep 0.5)
    (let [before (sess:write-queue:size)]
      (ev/sleep 0.5)
      (let [after (sess:write-queue:size)]
        (assert (and (> after 0) (= before after))
                (concat "setup: the writer has to be parked in port/write; "
                        "the queue went from " (string before) " to "
                        (string after)))))
    sess))

(defn wedged-close [grace]
  "Wedge one session, close it, and report what the close cost.
   A nil grace takes the module's default."
  (let* [[listener lport] (listen-ephemeral)
         peer (ev/spawn (fn [] (protect (deaf-peer listener))))
         url (concat "http://127.0.0.1:" (string lport))
         sess (wedge url)
         t0 (clock/monotonic)
         # protect, not a bare call, so a close that raises is reported
         # here rather than unwinding the file with the peer still up.
         # The [ok? _] pair is checked below: swallowing it would let an
         # arity error read as a fast close.
         returned (ev/timeout deadline
                              (fn []
                                (protect (if (nil? grace)
                                  (http2:close sess)
                                  (http2:close sess :grace grace)))))
         elapsed (- (clock/monotonic) t0)]
    (protect (ev/abort peer))
    (protect (port/close listener))
    (assert (not (nil? returned))
            (concat "http2:close never returned against a peer that stopped "
                    "reading; gave it " (string deadline) " s"))
    (assert (get returned 0)
            (concat "http2:close raised: " (string (get returned 1))))
    (println "  close returned after " (string elapsed) " s")
    elapsed))

# ── The close returns, and the grace is what decides when ────────────

(println "closing an h2 session whose peer stopped reading...")

(def default-wait (wedged-close nil))
(def short-wait (wedged-close 0.5))

(assert (< short-wait default-wait)
        (concat "a shorter grace has to close sooner: 0.5 s took "
                (string short-wait) " s against " (string default-wait)
                " s for the default"))

(assert (< short-wait 3)
        (concat "a 0.5 s grace must not hold the caller for 3 s; took "
                (string short-wait) " s"))

(assert (< default-wait (/ deadline 2))
        (concat "the default grace has to close well inside the budget; took "
                (string default-wait) " s of " (string deadline) " s"))

(println "h2 close on a dead peer: bounded, and the grace sets the bound")
