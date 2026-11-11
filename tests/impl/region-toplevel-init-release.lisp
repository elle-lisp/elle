(elle/epoch 15)
# audited: 2026-11-09
# A top-level def whose init is an if, a let or a begin is released at its last use, not when the file ends.
# docs/impl/region/lastuse.md
#
# Each row reads `arena/region-count` after the last read of a def and
# compares it with the count before the def was made, so a held value reads 1
# or more. The `live` row reads the same gauge while its def is still in use,
# and the `plain` row is a def whose init is the allocation itself.
#
# The counter-factual: the walk gave a letrec init the letrec node and passed
# it to the forms that hand the init's value on, so each array below took the
# end of the file as its last use. The if, let and begin rows read 1.

(def c (sys/args))

# live: the gauge sees an array whose def is still read below.
(def live-before (arena/region-count))
(def live-v (if c @[1] @[2]))
(def live-delta (- (arena/region-count) live-before))
(def live-first (first live-v))

# plain: the init is the array itself.
(def plain-before (arena/region-count))
(def plain-v @[3])
(def plain-first (first plain-v))
(def plain-delta (- (arena/region-count) plain-before))

# if: each arm hands its array to the def.
(def if-before (arena/region-count))
(def if-v (if c @[4] @[5]))
(def if-first (first if-v))
(def if-delta (- (arena/region-count) if-before))

# let: the body of a let hands its array to the def.
(def let-before (arena/region-count))
(def let-v
  (let [a 6]
    @[a]))
(def let-first (first let-v))
(def let-delta (- (arena/region-count) let-before))

# begin: the last form of a begin hands its array to the def.
(def begin-before (arena/region-count))
(def begin-v
  (begin
    (sys/args)
    @[7]))
(def begin-first (first begin-v))
(def begin-delta (- (arena/region-count) begin-before))

(println "region-toplevel-init-release: live " live-delta " plain " plain-delta
         " if " if-delta " let " let-delta " begin " begin-delta)

(assert (> live-delta 0) "the gauge sees an array whose def is still read")
(assert (= live-first 1) "live: the def reads its array")
(assert (= plain-first 3) "plain: the def reads its array")
(assert (= if-first 4) "if: the def reads its array")
(assert (= let-first 6) "let: the def reads its array")
(assert (= begin-first 7) "begin: the def reads its array")
(assert (= plain-delta 0)
        "control: a def of an array is released at its last use")
(assert (= if-delta 0) "a def of an if is released at its last use")
(assert (= let-delta 0) "a def of a let is released at its last use")
(assert (= begin-delta 0) "a def of a begin is released at its last use")

(println "region-toplevel-init-release: ok")
