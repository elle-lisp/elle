(elle/epoch 12)
# Counterfactual: a fiber whose final action is a TAIL-POSITION io must have its
# io driven by the scheduler and its result delivered.
#
# THE GAP: a tail-position io call goes through `rt_prepare_tail_call`, which
# writes the native's SIG_IO to memory[0..8]; the returning function's
# `handle_wasm_result` (src/wasm/store/call.rs) then folded that signal for the
# caller. It REPLACED SIG_IO with SIG_YIELD — the WASM caller keys yield-through
# off SIG_YIELD (bit 1), but the scheduler keys io submission off SIG_IO (bit 9)
# via fiber/bits. Dropping SIG_IO left the yielded io-request tagged a plain
# yield, so the scheduler re-queued the fiber and resumed it with nil instead of
# submitting the io. `tcp/connect` (whose body is `(apply tcp/connect-ip …)` in
# tail position) then read nil for its socket, and the framing built on it hung.
# The fix ORs SIG_YIELD onto SIG_IO instead of replacing it.
#
# `ev/sleep` in tail position is the minimal native tail-io; `tcp/connect` is
# the compiled-wrapper tail-io (`apply` in tail position) the redis/framing path
# depends on. RED only under `--wasm=full` before the fix (the fiber resumes
# with nil, so the value diverges from the VM/JIT tiers), GREEN on every tier
# after. Companion of tests/elle/port-shortread-framing.lisp.

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
