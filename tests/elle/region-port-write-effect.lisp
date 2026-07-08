(elle/epoch 12)
# port/write declares RegionEffect::Immediate (docs/impl/region/effects.md
# "Native region effects"): every non-error path yields an integer byte count
# — the empty-write short-circuit returns (SIG_OK . 0) directly, and the io
# completion returns (SIG_OK . result_code). The result is therefore always an
# immediate, with no heap region for the solver to release.
#
# This pins the RESUMED-value side of that claim. The declaration oracle in
# `dispatch_native_call` checks the result region of a NORMALLY-completing
# native call, but port/write yields (SIG_YIELD | SIG_IO) and a signal-carrying
# return is oracle-EXEMPT — the resumed integer arrives through the io
# completion path, which the oracle never sees. So this .lisp pin, not the
# oracle, is what holds port/write to its Immediate declaration: were the result
# ever a heap value, `Immediate` would be unsound (the solver records no
# result region and would leak it), and this pin goes RED.
#
# (The solver-side consequence of Immediate — no opaque-call arg clique between
# port/write's two heap args, hence no per-call data-region leak — is pinned
# directly in src/hir/regions/tests/effects.rs
# `port_write_declares_immediate_no_arg_clique`.)

(let [p (port/open "/dev/shm/elle-port-write-effect-test" :write)]
  (let [n (port/write p "hello")]
    (assert (= n 5) (string "port/write yields the integer byte count, got " n))
    (assert (= (arena/region-of n) 0)
            "port/write's result is an immediate (region 0), as Immediate claims"))
  (port/close p))
