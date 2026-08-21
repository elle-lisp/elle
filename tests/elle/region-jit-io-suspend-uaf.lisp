(elle/epoch 12)
# Counterfactual: a yielding I/O native called from JIT-compiled code escapes
# its result into `fiber.signal` WITHOUT the Rule-5 suspend-escape retain that
# the interpreter performs — so the escaped value's region is freed once by the
# resuming consumer and a second time by the scheduler: double-free / UAF.
# docs/impl/region/rules.md Rule 5 ("suspended frame" / suspend-escape) and Rule 8.
# This is the redis.lisp eager+adaptive crash in its minimal, network-server-free
# form (a loopback TCP pair in one process).
#
# THE BUG (Increment 6 residual): a yielding I/O primitive like `port/read-line`
# returns `(SIG_YIELD | SIG_IO, IoRequest{buffer})` — the IoRequest (an External)
# and its read buffer (an LBytes) are CO-LOCATED in the native's one fresh
# per-execution region (rc=1). The fiber suspends; the IoRequest escapes into
# `fiber.signal`, where the scheduler reads it to perform the I/O, and the read
# buffer becomes the resume RESULT in the same region. So that one rc=1 region is
# referenced TWICE: by the escaped IoRequest (released by the scheduler) and by
# the result value (released by the consumer's `DecrefValueRegion`).
#
# The interpreter's `handle_primitive_signal` SignalAction::Suspend arm retains
# the escape: `incref_for_escape(region_of(value), SuspendEscape)` (src/vm/signal.rs)
# — bumping rc to 2 so the two releases balance. The JIT's mirror,
# `jit_handle_primitive_signal` (src/jit/calls.rs), OMITTED that incref: it just
# set `fiber.signal`. So when a *JIT-compiled* function calls the yielding native
# (yielding functions ARE JIT-compiled, via side-exit — src/jit/compiler.rs:104),
# rc stays 1, the result release frees the region (buffer + IoRequest), and the
# scheduler's release double-frees it: `DecrefRegion(N) but region was never
# alloc_in_region'd` (regionstore.rs phantom/double-free), or a SIGSEGV under
# `--trace=guardfree`.
#
# REACHABILITY (why this needs a hot per-call reader, not one big read): the JIT
# only compiles a function once it is HOT (call-counted). `read1` does exactly
# ONE `port/read-line` per call and is called per line, so thousands of calls
# drive it past the adaptive/background-compile threshold; once compiled, every
# subsequent yielding read takes the JIT suspend path. A single function that
# loops the reads internally is called once, never gets hot, and stays
# interpreted — so it never exercises the defect (which is why the bug hid behind
# redis, whose RESP reader is driven hot under the async scheduler).
#
# RED now: the program double-frees and aborts mid-read under any JIT tier
# (eager/adaptive). GREEN once `jit_handle_primitive_signal` mirrors the
# interpreter's suspend-escape retain. Under `--jit=off` the interpreter already
# retains correctly, so this file is a valid harness on both tiers (it passes).

# ── loopback server: stream a fixed line many times, then close ───────────
(def line-count 20000)
(def the-line "PONGPONGPONG")

(def listener (tcp/listen "127.0.0.1" 0))
(def server-port
  (let [path (port/path listener)]
    (parse-int (slice path (+ 1 (string/find path ":"))))))

(def server
  (ev/spawn (fn []
              (let [client (tcp/accept listener)]
                (var i 0)
                (while (< i line-count)
                  (port/write client (concat the-line "\r\n"))
                  (assign i (+ i 1)))
                (port/flush client)
                (port/close client)))))

# ── the JIT target: ONE yielding read per call, called per line so it goes
#    hot and the JIT compiles it (with a yield side-exit). ──────────────────
(defn read1 (sock)
  (port/read-line sock))

(def reader
  (ev/spawn (fn []
              (let [sock (tcp/connect "127.0.0.1" server-port)]
                (var i 0)
                (var last nil)
                (while (< i line-count)
                  (assign last (read1 sock))
                  (assign i (+ i 1)))
                (port/close sock)
                last))))

# ── witness ───────────────────────────────────────────────────────────────
# RED: the hot JIT-compiled `read1`'s escaped IoRequest is over-released → the
# program double-frees and aborts before `reader` finishes. GREEN: every read
# survives and the last line read is intact.
(def result (ev/join-protected reader))
(protect (port/close listener))
(protect (ev/join-protected server))

(assert (get result 0)
        "reader fiber faulted — a JIT yielding-read escape was over-released")
(assert (= (get result 1) the-line)
        "the last JIT-read line was corrupted (its region was freed under the read)")

(println "region-jit-io-suspend-uaf: ok")
