(elle/epoch 15)
# audited: 2026-10-20
# A record std/framing answers from inside a chunk is a view of the chunk, and claims no page for its payload.
# lib/framing.md
#
# `arena/bytes` sums the pages every live region owns and the pages the pool
# caches. A page that moves from the cache to a region leaves the sum where it
# was, so one copy of a 1 MiB line can read as free when the cache holds a
# page of its class. Eight copies held at once cannot: the cache is bounded at
# 4 MiB by default, so at least four of them map fresh pages. Eight views of
# one 8 MiB chunk claim at most eight base pages, for their headers.
#
# The counter-factual: a framer that copied each line out of its chunk read
# 14 MiB here.

(def framing ((import "std/framing")))

(def mib 1048576)
(def line-text (string (string/repeat "x" (dec mib)) "\n"))
(def chunk (bytes (string/repeat line-text 8)))
(assert (= (* 8 mib) (length chunk)) "the chunk is eight 1 MiB lines")

(def @served false)
(def source
  (fn [n]
    (if served
      nil
      (begin
        (assign served true)
        chunk))))
(def f (framing:open source :size (* 16 mib)))

(let [before (arena/bytes)
      held [(framing:line f) (framing:line f) (framing:line f) (framing:line f)
            (framing:line f) (framing:line f) (framing:line f) (framing:line f)]
      after (arena/bytes)
      grew (- after before)]
  (assert (= 8 (length held)) "every line is held while the gauge is read")
  (each l in held
    (assert (= (dec mib) (length l)) "each line is 1 MiB less its newline"))
  (assert (< grew mib)
          (string "a line inside the chunk claims no page for its payload: grew "
                  grew)))
(println "  1. a line inside the chunk is a view of it")

(assert (nil? (framing:line f)) "the source ends after its one chunk")
(println "framing-view: ok")
