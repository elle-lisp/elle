(elle/epoch 12)
# audited: 2026-09-29
# A fiber whose last action is a tail-position I/O call has that I/O driven by the scheduler and its result delivered.
# docs/impl/wasm.md
#
# On the WASM tier a tail-position io call goes through `rt_prepare_tail_call`,
# which writes the native's SIG_IO to memory[0..8], and the returning
# function's `handle_wasm_result` (src/wasm/store/call.rs) folds that signal for
# the caller by ORing SIG_YIELD onto SIG_IO. The WASM caller keys yield-through
# off SIG_YIELD (bit 1), and the scheduler keys io submission off SIG_IO (bit 9)
# via fiber/bits. The counter-factual REPLACES SIG_IO with SIG_YIELD: the
# scheduler re-queues the fiber as a plain yield and resumes it with nil, so
# `tcp/connect` (whose body is `(apply tcp/connect-ip …)` in tail position)
# reads nil for its socket and the framing built on it hangs.
#
# `ev/sleep` in tail position is the minimal native tail-io; `tcp/connect` is
# the compiled-wrapper tail-io (`apply` in tail position) the redis/framing path
# depends on. Only the WASM tier can fold the signal wrong, and there the fiber
# resumes with nil where every other tier delivers the value. Companion of
# tests/lang/port-shortread-framing.lisp.

# Minimal native tail-io: the fiber's last form is `(ev/sleep …)`, which yields
# SIG_IO and completes with nil.
(assert (nil? (ev/join (ev/spawn (fn [] (ev/sleep 0.001)))))
        "fiber ending in a tail ev/sleep completes with nil")

# The value BEFORE a tail-io still flows: sleep is tail, its own result (nil) is
# the fiber value, but a sibling fiber's computed value is unaffected.
(assert (= 7 (ev/join (ev/spawn (fn [] 7))))
        "a sibling fiber's value is unaffected by tail-io routing")

# Compiled-wrapper tail-io: `tcp/connect` tail-calls `tcp/connect-ip`. A server
# fiber accepts and closes; the client connect (in a fiber, joined) must return
# a live port rather than nil.
(let [listener (tcp/listen "127.0.0.1" 0)
      port-num (parse-int (get (string/split (port/path listener) ":") 1))]
  (ev/spawn (fn [] (port/close (tcp/accept listener))))
  (let [conn (ev/join (ev/spawn (fn [] (tcp/connect "127.0.0.1" port-num))))]
    (assert (not (nil? conn))
            "tail tcp/connect in a fiber returns a port, not nil")
    (port/close conn)))

(println "wasm-tail-io-in-fiber: ok")
