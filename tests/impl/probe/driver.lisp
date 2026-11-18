(elle/epoch 15)
# audited: 2026-11-17
# The direct-loop driver every row table runs with, and the sets of rows read on the region and allocation counts too.
#
# docs/ratchet.md
# docs/impl/region/diagnostics.md
# A row is `[label (fn [j] body)]`, and `j` varies the input so a body cannot
# constant-fold. The rate a row is held to is its row in the ledger.
#
# A label in `dual-read` is read on the object count AND the region count in
# one drive. The object count cannot see a region entry that holds no object:
# a pages-less owner node (docs/impl/region/owner.md), or a region emptied of
# objects but pinned by an unbalanced count. So the park families, whose
# machinery moves whole region entries between fibers and frames, and the
# adopt-park family, the one set whose activation mints an owner node, read
# the second dimension. Every other row is object-only: a strand that holds an
# object already moves the object count, and none of their shapes reaches the
# owner-node machinery. The ledger's `:regions` rows hold the set here: a
# label dropped from it leaves a missing row, and one added an unledgered
# reading.
#
# A label in `alloc-read` is read on the allocation count as well. Each such
# probe builds from `j` the value whose release its row reads. A literal of
# constants is built once per unit (docs/impl/hoist.md), so a probe that reads
# one builds nothing per op and reads 0 whatever the release does. The
# allocation pin holds the probe to building its value: a probe that stops
# reads `stale` there.
#
# The instrument is bound here rather than in oracle.lisp: `include-file`
# splices every included file ahead of the including file's own forms, so a
# binding the rows need has to come from the first include.
(def r ((import "std/ratchet")))

(def dual-read
  |"fiber-nested" "multi-resume" "yield-discard" "denied-discard"
   "abort-discard" "cancel-discard" "adopt-park-drop" "adopt-park-abort"
   "adopt-park-cancel" "adopt-complete" "adopt-nopark" "plain-park-drop"
   "adopt-before-park-drop" "adopt-before-park-cancel"|)

(def alloc-read
  |"bound-callee" "fold" "read" "each-array" "each-manual" "distinct" "group-by"
   "frequencies" "merge" "wrap-map" "mut-string" "emit-dyn-error-repeat"|)

(defn run-direct-loop [rows]
  (each entry rows
    (let [label (get entry 0)
          probe (get entry 1)
          on (if (has? dual-read label) [r:objects r:regions] [r:objects])]
      (r:rate label probe
              :on (if (has? alloc-read label) (concat on [r:allocs]) on)))))
