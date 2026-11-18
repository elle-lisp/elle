(elle/epoch 15)
# audited: 2026-11-17
# The ledger of tests/impl/oracle.lisp, part 3: what a probe builds per op, where its rate reads the release of a value it builds.
(producer "tests/impl/oracle.lisp")
# The allocation gauge's live-growth row: a pair per op, dropped at once. A
# floor, voiding every allocation row when it reads flat.
["allocs gauge (live-growth)" :allocs :floor 0.5 :class :growth]
# Each row pins the objects a probe mints per op, beside its rate of 0. The
# probe builds from the op's index the value whose release that rate reads. A
# literal of constants is built once per unit (docs/impl/hoist.md), so a probe
# that stopped building its value would still read 0 on its rate. Its row here
# reads stale instead.
#
# The direct-loop rows (probe/direct.lisp, probe/concurrent.lisp).
["bound-callee" :allocs 1]
["fold" :allocs 1]
["read" :allocs 1]
["each-array" :allocs 1]
["each-manual" :allocs 1]
["distinct" :allocs 6]
["group-by" :allocs 4]
["frequencies" :allocs 3]
["merge" :allocs 6]
["wrap-map" :allocs 3]
["mut-string" :allocs 2]
["emit-dyn-error-repeat" :allocs 4]
# The native-tail and read rows (probe/stdlib.lisp).
["native-tail-put-struct" :allocs 2]
["native-tail-put-array" :allocs 2]
["native-tail-del-ctl" :allocs 2]
["native-tail-push-array" :allocs 2]
["native-tail-add-set" :allocs 2]
["native-tail-push-string" :allocs 3]
["read-first" :allocs 1]
["read-last" :allocs 1]
["read-get-array" :allocs 1]
["read-get-struct" :allocs 1]
["read-has-struct" :allocs 1]
["read-length" :allocs 1]
# The injected abort delivery (probe/abort.lisp).
["abort-masked" :allocs 4]
["abort-escape" :allocs 7]
["abort-caught" :allocs 8]
["abort-own-error" :allocs 9]
["abort-reraise" :allocs 8]
["abort-defer" :allocs 7]
["abort-held" :allocs 2]
["abort-other" :allocs 3]
["abort-aborting-frame" :allocs 7]
["abort-tail-result" :allocs 8]
["abort-tail-discarded" :allocs 8]
["abort-mask-caught-literal" :allocs 8]
["abort-mask-caught-bound" :allocs 8]
["refuse-tail-result" :allocs 8]
["refuse-tail-discarded" :allocs 8]
# The accumulator fed a per-op map (probe/container.lisp). The run-block builds
# one accumulator per block of a hundred ops.
["push-accum" :allocs 3.01]
# The call-result and break rows (probe/branch.lisp).
["branch-call" :allocs 1]
["break-value" :allocs 1]
["break-value-used" :allocs 1]
["break-skipped" :allocs 1]
["break-skipped-nobreak" :allocs 1]
