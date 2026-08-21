(elle/epoch 12)
# An h2 request that carries custom headers, sent from inside a process.
#
# The invariant: `process:start` changes who schedules a fiber, not what
# a request does. An h2 request completes inside a process exactly as it
# does outside one, whatever headers it carries and whatever fuel
# quantum the process runs under.
#
# Six cases vary the three things that could matter independently —
# inside or outside a process, custom headers or none, unary or bidi —
# so a failure names which combination broke rather than "h2 in a
# process". The pairs that share a row differ in one axis only:
#
#   1 vs 5    outside a process vs inside, both carrying headers
#   2 vs 5    no headers vs headers, both unary inside a process
#   3 vs 6    no headers vs headers, both bidi inside a process
#   4 vs 5    a large fuel quantum vs the default, otherwise identical
#
# Two ordering rules hold the file together. Break either one and the
# cases stop measuring what they name.
#
# Each case builds its own server and its own process. A process that
# fails to complete leaves the scheduler in a state that stops every
# later process in the same run, so cases sharing one server would
# report each other's outcome rather than their own.
#
# The cases that exercise the least machinery run first, for the same
# reason. Case 4 is the one this decides: placed after a case that does
# not complete, it reports failure whatever its fuel does, which is the
# opposite of what it measures.
#
# The `eprintln` inside cases 4, 5 and 6 is part of each case, not a
# trace. It is work performed in the process before the request, and how
# much work runs before the request decides how many fuel quanta the
# request itself spans. Remove it and cases 5 and 6 stop covering the
# multi-quantum path.
#
# What the multi-quantum path needs from the scheduler is in
# `tests/elle/process-io-park.lisp`: a process the scheduler preempted
# for fuel is still ready, and the forwarded read that carries the
# server's answer cannot complete until that process finishes sending
# the request.

(def process ((import "std/process")))
(def http2 ((import "std/http2")))

# ── Helpers ──────────────────────────────────────────────────────────

(defn listen-ephemeral []
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

(def grpc-ct [["content-type" "application/grpc"]])

(defn with-server [body]
  "Run `body` against a fresh echo server, then tear it down. `body`
   takes the base url."
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         handler (fn [req] {:status 200 :body (or req:body (bytes))})
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))]
    (defer
      (begin
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (body url))))

# The deadline is three orders of magnitude above what a request against
# a loopback server costs, so only a request that never leaves the client
# reaches it.
(def deadline 10)

(def @stuck @[])

(defn run-case [label thunk]
  "Run `thunk` under `deadline`. Records `label` when it does not finish."
  (if (nil? (ev/timeout deadline thunk))
    (begin
      (push stuck label)
      (println "  stuck  " label))
    (println "  ok     " label)))

(println "h2 requests with custom headers inside process:start:")

# ── 1: outside a process, carrying headers ───────────────────────────

(run-case "1. unary + headers, no process:start"
          (fn []
            (with-server (fn [url]
                           (let* [sess (http2:connect url)
                                  resp (http2:send sess "GET" "/echo"
                                  :headers grpc-ct)]
                             (assert (= resp:status 200) "1: status"))
                           true))))

# ── 2: inside a process, no custom headers ───────────────────────────

(run-case "2. unary, no headers, in process:start"
          (fn []
            (with-server (fn [url]
                           (process:start (fn []
                             (let [sess (http2:connect url)]
                               (each i in (range 0 10)
                                 (let [resp (http2:send sess "GET" "/echo")]
                                   (assert (= resp:status 200) "2: status"))))
                             true))
                           true))))

# ── 3: inside a process, bidi, no custom headers ─────────────────────

(run-case "3. open-stream, no headers, in process:start"
          (fn []
            (with-server (fn [url]
                           (process:start (fn []
                             (let [sess (http2:connect url)]
                               (http2:open-stream sess "POST" "/echo"))
                             true))
                           true))))

# ── 4: case 5's request, with fuel enough to finish in one slice ─────

(run-case "4. unary + headers, in process:start, fuel 10000"
          (fn []
            (with-server (fn [url]
                           (process:start (fn []
                             (let [sess (http2:connect url)]
                               (eprintln "    (4 connected)")
                               (let [resp (http2:send sess "GET" "/echo"
                                     :headers grpc-ct)]
                                 (assert (= resp:status 200) "4: status")))
                             true) :fuel 10000)
                           true))))

# ── 5: inside a process, carrying headers, default fuel ──────────────

(run-case "5. unary + headers, in process:start"
          (fn []
            (with-server (fn [url]
                           (process:start (fn []
                             (let [sess (http2:connect url)]
                               (eprintln "    (5 connected)")
                               (let [resp (http2:send sess "GET" "/echo"
                                     :headers grpc-ct)]
                                 (assert (= resp:status 200) "5: status")))
                             true))
                           true))))

# ── 6: the same, through the bidi entry point ────────────────────────

(run-case "6. open-stream + headers, in process:start"
          (fn []
            (with-server (fn [url]
                           (process:start (fn []
                             (let [sess (http2:connect url)]
                               (eprintln "    (6 connected)")
                               (http2:open-stream sess "POST" "/echo"
                               :headers grpc-ct))
                             true))
                           true))))

# Report and leave immediately rather than raising. A run where any case
# did not complete cannot shut down: the fibers that case left behind
# have no one to drive them, so teardown does not finish and the failure
# would surface as a timeout with no message. Exiting here keeps the
# result readable and bounded by `deadline` per case.
(unless (empty? (->list stuck))
  (println "FAIL: h2 requests did not complete inside process:start:")
  (each label in (->list stuck)
    (println "  - " label))
  (sys/exit 1))

(println "h2 headers in process: all cases completed")
