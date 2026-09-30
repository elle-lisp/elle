(elle/epoch 13)
# audited: 2026-09-30
# The ledger of tests/impl/region-collector-arg-move.lisp: what a fresh value moved into a collector parameter leaves behind per call.
(producer "tests/impl/region-collector-arg-move.lisp")
["objects gauge (live-growth)" :objects :floor 0.5 :class :growth]
["regions gauge (live-growth)" :regions :floor 0.5 :class :growth]
# One fresh value moved into each collector kind, and the positional control,
# which takes the same move through the ordinary owned-parameter release.
["& rest list" :objects 0]
["& rest list" :regions 0]
["&keys struct" :objects 0]
["&keys struct" :regions 0]
["&named struct" :objects 0]
["&named struct" :regions 0]
["positional parameter" :objects 0]
["positional parameter" :regions 0]
# Several fresh values into one collector: the release is per collected
# argument, so a collector holding three owes three.
["& rest list, three values" :objects 0]
["& rest list, three values" :regions 0]
["&keys struct, three values" :objects 0]
["&keys struct, three values" :regions 0]
# One value in two argument positions arrives with one moved reference, so the
# release is declined for it and each call keeps the reference nobody took
# over. A release moved past the aliasing check reads stale here and as a
# wrong answer or a fault in the probe.
["& rest list, aliased value" :objects 1]
["& rest list, aliased value" :regions 1]
["&keys struct, aliased value" :objects 1]
["&keys struct, aliased value" :regions 1]
# The collected value survives the call it was collected for.
["&named struct, callee reads the value" :objects 0]
["&named struct, callee reads the value" :regions 0]
# The shapes that read 0 because they remove the move itself.
["&keys struct, call not in tail" :objects 0]
["&keys struct, call not in tail" :regions 0]
["&keys struct, value the caller never owned" :objects 0]
["&keys struct, value the caller never owned" :regions 0]
# A callee that keeps what it collects is unbounded by construction: the
# collector path's own proof that the gauges see it, reading the struct and
# the bytes it holds per call.
["&keys struct the callee keeps" :objects :floor 1 :class :growth]
["&keys struct the callee keeps" :regions :floor 1 :class :growth]
