(elle/epoch 12)
# udp/send-to declares RegionEffect::Immediate (docs/impl/region-effects.md):
# the io completion returns (SIG_OK . result_code) — the number of bytes sent —
# so the result is always an immediate, with no heap region for the solver to
# release.
#
# This pins the RESUMED-value side of that claim. udp/send-to yields
# (SIG_YIELD | SIG_IO) and a signal-carrying return is EXEMPT from the
# declaration oracle in `dispatch_native_call`, so the resumed integer arrives
# through the io completion path, which the oracle never sees. This .lisp pin,
# not the oracle, holds udp/send-to to its Immediate declaration: were the
# result ever a heap value, `Immediate` would be unsound and this goes RED.
#
# UDP is connectionless: a send to a port with no listener still succeeds and
# returns the byte count (the datagram is handed to the kernel fire-and-forget),
# so no receiver or ephemeral-port discovery is needed.
#
# (The solver-side consequence — no opaque-call arg clique among udp/send-to's
# three heap args, hence no per-call leak — is pinned in
# src/hir/regions/tests/effects.rs `udp_send_to_declares_immediate_no_arg_clique`.)

(let [s (udp/bind "127.0.0.1" 0)]
  (let [n (udp/send-to s "hello" "127.0.0.1" 9999)]
    (assert (= n 5) (string "udp/send-to yields the integer byte count, got " n))
    (assert (= (arena/region-of n) 0)
            "udp/send-to's result is an immediate (region 0), as Immediate claims"))
  (port/close s))
