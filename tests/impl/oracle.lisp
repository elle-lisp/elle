(elle/epoch 13)
# audited: 2026-09-30
# The region leak dashboard: one per-op leak rate per probe, judged against tests/ledger/oracle.lisp.
# docs/ratchet.md
# docs/impl/region/diagnostics.md
#
# Every probe is a shape driven in a loop under a heap gauge — the object
# count, the region count, the byte count or the physical-id counter, chosen
# for the dimension its class leaks in — and read as a per-op rate by the
# ratchet's estimator (lib/ratchet.md). A rate says how large a leak is and
# nothing about soundness: the trustworthy UAF signal is `--trace=guardfree`
# under the full stdlib (docs/impl/region/diagnostics.md).
#
# The ledger holds every pin. A rate that moves either way fails, a probe
# with no row fails as unledgered, and a row with no probe fails as missing,
# so a probe read on the region count beside the object count is held to
# both by its two rows. Each family below is spliced at compile time, so the
# dashboard is one program reporting in one place: the split is a reading
# budget (DOCUMENTATION.md § Documents). Include order is run order and
# definition order, and the driver comes first: it binds the instrument and
# every row table runs through it.
(include-file "probe/driver.lisp")
(include-file "probe/gauge.lisp")
(include-file "probe/shape.lisp")
(include-file "probe/frame.lisp")
(include-file "probe/factory.lisp")
(include-file "probe/park.lisp")
(include-file "probe/direct.lisp")
(include-file "probe/concurrent.lisp")
(include-file "probe/tailcall.lisp")
(include-file "probe/stdlib.lisp")
(include-file "probe/abort.lisp")
(include-file "probe/store.lisp")
(include-file "probe/yield.lisp")
(include-file "probe/container.lisp")
(include-file "probe/branch.lisp")
(include-file "probe/native.lisp")

# The double-release counter, read once after the last probe. It is monotonic
# from process start, so its row at 0 covers the stdlib load ahead of every
# probe as well as the probes, and a release that ran twice — which no rate
# can see — fails here (docs/impl/region/diagnostics.md).
(r:read "over-free" :releases (arena/over-frees))

(r:report)
(println "oracle: ok")
