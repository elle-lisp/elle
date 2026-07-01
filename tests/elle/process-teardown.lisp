(elle/epoch 10)
## tests/elle/process-teardown.lisp — process:start orphan sub-fiber teardown
##
## When a process:start body completes but leaves a fire-and-forget
## sub-fiber parked — on a futex, on blocked I/O, in a background accept
## loop — no live process can ever wake it or read its result. The
## process scheduler must tear such orphans down (aborting them so their
## defer/protect cleanup runs, and cancelling their in-flight I/O) and
## return, rather than spinning/blocking forever on work that can never
## complete. This mirrors ev/run's program-completion teardown for the
## root scheduler (stdlib.lisp make-async-scheduler).
##
## Every "orphan" test below HANGS on a scheduler without this teardown
## (the CI timeout wrapper kills the run). Reaching each println means the
## orphan was torn down inside the process:start call.
##
## Run: elle tests/elle/process-teardown.lisp

(def process ((import "std/process")))
(def sync ((import "std/sync")))
(def http2 ((import "std/http2")))

(defn listen-ephemeral []
  "Bind an ephemeral TCP port, return [listener port]."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn h2-handler [req]
  {:status 200 :headers {:content-type "text/plain"} :body (bytes 79 75)})

(println "tests/elle/process-teardown.lisp:")

## ── 1. Orphan parked on a futex (never notified) ──────────────────
## The sub-fiber blocks on an empty queue forever. Nothing will ever
## put to it, and the body exits without joining it.

(process:start (fn []
                 (ev/spawn (fn []
                             (let [q (sync:make-queue 1)]
                               (q:take))))
                 nil))
(println "  1. futex orphan torn down: ok")

## ── 2. Orphan blocked on forwarded I/O (accept) ───────────────────
## The sub-fiber blocks in accept with no client ever connecting; the
## listener is deliberately left open so the accept cannot fail.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn [] (tcp/accept listener)))
                   nil)))
(println "  2. i/o orphan torn down: ok")

## ── 3. Orphan background h2 server (never stopped) ────────────────
## The forever-accept serve loop is the http2:serve shape from
## process-io.lisp; the body never closes the listener or the server.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   nil)))
(println "  3. background h2 server orphan torn down: ok")

## ── 4. Nested orphan (sub-fiber spawns a parked sub-sub-fiber) ─────

(process:start (fn []
                 (ev/spawn (fn []
                             (ev/spawn (fn []
                                         (let [q (sync:make-queue 1)]
                                           (q:take))))
                             nil))
                 nil))
(println "  4. nested orphan torn down: ok")

## ── 5. Orphan cleanup runs (graceful abort, not a hard kill) ──────
## The orphan runs (via drain) and parks inside its defer after the body
## exits, then teardown aborts it through its unwind path — so the defer
## must fire. Asserts the observable side effect, not just that the
## process returned.

(def @cleanup-ran false)
(process:start (fn []
                 (ev/spawn (fn []
                             (defer
                               (assign cleanup-ran true)
                               (let [q (sync:make-queue 1)]
                                 (q:take)))))
                 nil))
(assert cleanup-ran "orphan defer ran during teardown")
(println "  5. orphan defer runs during teardown: ok")

## ── 6. Real work, then an orphaned server (process-io.lisp shape) ──
## The body does a full h2 round-trip and closes the session, but leaves
## the background server running (never closes the listener).

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (let [resp (http2:send session "GET" "/health")]
                       (assert (= resp:status 200) "real work: got 200"))
                     (protect (http2:close session))))))
## listener left open → orphan
(println "  6. real work + server orphan torn down: ok")

## ── Regression guards: teardown must NOT fire while a process lives ──

## 7. A joined sub-fiber is not an orphan — the process stays alive until
##    the join returns.

(def @joined-val nil)
(process:start (fn []
                 (let [f (ev/spawn (fn [] (+ 20 22)))]
                   (assign joined-val (ev/join f)))))
(assert (= joined-val 42) "joined sub-fiber completed")
(println "  7. joined sub-fiber completes (no premature teardown): ok")

## 8. A child process outlives PID 0 and completes joined sub-fiber work.
##    The scheduler must keep running while the child is alive (even while
##    the child is parked waiting on its own sub-fiber), and must not tear
##    the child's work down just because PID 0 exited first.

(def @child-result nil)
(process:start (fn []
                 (process:spawn (fn []
                                  (assign
                                    child-result
                                    (ev/join (ev/spawn (fn [] (* 6 7)))))))
                 nil))
## PID 0 exits first; the child still has work to do
(assert (= child-result 42) "child completed joined sub-fiber after PID 0 exit")
(println "  8. child outlives PID 0, completes sub-fiber work: ok")

## 9. Teardown must not leak forwarded I/O into the root scheduler.
##    Several orphan-leaving process:start calls in a row, then a normal
##    top-level TCP round-trip: if teardown left a cancelled-but-tracked
##    submission behind, the root pump would wedge on the final read.

(each i in (range 3)
  (process:start (fn []
                   (let* [[listener lport] (listen-ephemeral)]
                     (ev/spawn (fn [] (tcp/accept listener)))  ## I/O orphan
                     (ev/spawn (fn []
                                 (let [q (sync:make-queue 1)]
                                   (q:take))))  ## futex orphan
                     nil))))
(let* [[listener lport] (listen-ephemeral)]
  (ev/spawn (fn []
              (let [c (tcp/accept listener)]
                (port/write c (bytes 88))
                (port/close c))))
  (let [c (tcp/connect "127.0.0.1" lport)
        d (port/read c 1)]
    (assert (= d (bytes 88)) "top-level I/O works after orphan teardowns")
    (port/close c)
    (port/close listener)))
(println "  9. sequential orphans, root scheduler still healthy: ok")

(println "")
(println "tests/elle/process-teardown.lisp: all tests passed")
