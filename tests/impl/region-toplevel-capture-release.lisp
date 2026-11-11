(elle/epoch 15)
# audited: 2026-11-09
# A top-level def a closure captures is released at its last use, not when the file ends.
# docs/impl/region/cells.md
# docs/impl/region/lastuse.md
#
# Each row reads `arena/region-count` after the last use of a captured binding
# and compares it with the count before the binding was made. Each binding's
# cell is minted before that reading: the file's letrec mints a top-level
# binding's cell when the file starts, and `populate_env` mints the `letrec`
# row's env cell when the call starts. So a row that frees the value and its
# cell reads -1, and a held cell or value reads 0 or more. The `live` row reads
# the same gauge while its binding is still in use, so a gauge that counted
# nothing fails there.
#
# The counter-factual: the file's letrec gave its cells the end of the file as
# their release, and a capture by a closure a letrec binds carried the
# captured value there too. The `plain` row read 1 and the `begin` row 0. The
# `letrec` row read 1, because a letrec binding a sibling captures inside a
# function never released its env cell, which held the value.

# live: the gauge sees a captured def before its last use.
(def live-before (arena/region-count))
(def live-v @[1 2 3])
(defn live-head []
  (first live-v))
(def live-delta (- (arena/region-count) live-before))
(def live-result (live-head))

# plain: a def, the function that captures it, and the call that reads it.
(def plain-before (arena/region-count))
(def plain-v @[1 2 3])
(defn plain-head []
  (first plain-v))
(def plain-result (plain-head))
(def plain-delta (- (arena/region-count) plain-before))

# begin: the same forms inside a top-level begin, which binds in the file.
(def begin-before (arena/region-count))
(begin
  (def begin-v @[1 2 3])
  (defn begin-head []
    (first begin-v))
  (def begin-result (begin-head)))
(def begin-delta (- (arena/region-count) begin-before))

# letrec: inside a function, a letrec binds a closure that captures a sibling.
(defn letrec-capture []
  (def before (arena/region-count))
  (letrec [v @[1 2 3]
           g (fn [] (first v))]
    (g)
    (- (arena/region-count) before)))
(def letrec-delta (letrec-capture))

(println "region-toplevel-capture-release: live " live-delta " plain "
         plain-delta " begin " begin-delta " letrec " letrec-delta)

(assert (> live-delta 0) "the gauge sees a captured def before its last use")
(assert (= live-result 1) "live: the closure reads the def")
(assert (= plain-result 1) "plain: the closure reads the def")
(assert (= begin-result 1) "begin: the closure reads the def")
(assert (= plain-delta -1)
        "a captured top-level def and its cell are released at its last use")
(assert (= begin-delta -1)
        "a captured def in a top-level begin and its cell are released at its last use")
(assert (= letrec-delta -1)
        "a letrec capture and its cell are released with the closure's binding")

(println "region-toplevel-capture-release: ok")
