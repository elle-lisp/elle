(elle/epoch 12)
## tests/elle/port-text-framing.lisp
##
## `port/read-exact` on a TEXT port counts grapheme clusters. A cluster
## has no upper bound in bytes, so the byte length of the answer is not
## known when the read is submitted. See docs/io.md § "A read that
## overshoots keeps the rest for the same port".
##
## The trap these assertions guard: a fiber buffer sized `4 * n` was
## treated as an upper bound on the answer. A cluster wider than four
## bytes overran it — `std::ptr::copy` past the end of a region slice,
## which smashes whatever the heap put next — and the io_uring resubmit
## loop, finding no room left in that buffer, dropped the operation
## without a completion and left the fiber parked forever. A SIGSEGV and a
## hang, from safe Elle code, on either backend.
##
## The counter-factual: every assertion here passes byte-for-byte with
## ASCII payloads, because ASCII is one byte per cluster and `4 * n` then
## has three bytes of slack per cluster. Only a payload whose clusters are
## wider than the guess reaches the defect, which is why the first two
## cases read a multi-codepoint emoji rather than text.

## One family emoji: four people joined by three zero-width joiners. 25
## UTF-8 bytes, one grapheme cluster — six times the four bytes per
## cluster the buffer used to reserve.
(def family "👨‍👩‍👧‍👦")

(defn listener-port [listener]
  "The port number a listener bound to an ephemeral port received."
  (parse-int (get (string/split (port/path listener) ":") 1)))

(defn with-text-peer [chunks body]
  "Run `body` against a connected text port whose peer writes each of
   `chunks` in turn, pausing between them so each lands as its own read,
   and then closes."
  (let [listener (tcp/listen "127.0.0.1" 0)
        port-num (listener-port listener)]
    (ev/spawn (fn []
                (let [conn (tcp/accept listener)]
                  (each chunk in chunks
                    (port/write conn chunk)
                    (port/flush conn)
                    (ev/sleep 0.15))
                  (ev/sleep 0.2)
                  (port/close conn))))
    (let [client (tcp/connect "127.0.0.1" port-num :encoding :text)]
      (defer
        (begin
          (port/close client)
          (port/close listener))
        (body client)))))

(defn drain-clusters [p]
  "Everything left on `p`, read one cluster at a time and joined."
  (let [@got @""]
    (let [@piece (port/read-exact p 1)]
      (while (not (nil? piece))
        (push got piece)
        (assign piece (port/read-exact p 1))))
    (freeze got)))

## ── clusters wider than the buffer's guess ─────────────────────────────
##
## Four family emoji are 100 bytes. Reading them back a cluster at a time
## and joining the pieces must reproduce the payload exactly: no byte lost
## between two reads, none written twice, and nothing written past the end
## of the buffer that carries them.
##
## The assertion is the join rather than each piece, because where one read
## stops is a segmentation question this file does not own: a cluster is
## only certainly finished once a later codepoint declines to join it, and
## a stream can always deliver that codepoint next. What every read does
## owe is that the bytes it hands back, followed by the bytes the next read
## hands back, are the bytes the peer sent.
(def wide (concat family family family family))
(assert (= (with-text-peer [wide] drain-clusters) wide)
        "reads of wide clusters reassemble the payload byte for byte")
(println "  1. wide clusters: reassembled")

## ── a wide remainder survives the handover ─────────────────────────────
##
## The kernel read behind `read-line` takes the whole block, so all 100
## bytes of the payload arrive as the port's remainder and the reads that
## follow are served from it. Those held bytes are what used to be copied
## into a buffer far too small for them.
(assert (= (with-text-peer [(concat "hdr\n" wide)]
                           (fn [p]
                             (assert (= (port/read-line p) "hdr")
                                     "the header line")
                             (drain-clusters p))) wide)
        "a remainder wider than the next read's buffer is served whole")
(println "  2. wide remainder carried forward")

## ── read-exact after an over-reading read-line ─────────────────────────
##
## The read-line takes "hdr" and leaves "BODYBODY" behind as the port's
## remainder — fewer clusters than the read-exact asks for, so the
## read-exact must join the remainder to bytes it reads itself.
(assert (= (with-text-peer [(concat "hdr\n" "BODYBODY") "0123456789AB"]
                           (fn [p]
                             (assert (= (port/read-line p) "hdr")
                                     "the header line")
                             (port/read-exact p 12))) "BODYBODY0123")
        "read-exact joins the held remainder to the bytes it reads")
(println "  3. remainder joined to a following read-exact")

## ── the join leaves its own remainder behind ───────────────────────────
(assert (= (with-text-peer [(concat "hdr\n" "BODYBODY") "0123456789AB"]
                           (fn [p]
                             (port/read-line p)
                             (port/read-exact p 12)
                             (port/read-exact p 8))) "456789AB")
        "what the join did not answer with stays with the port")
(println "  4. the join's own remainder")

## ── a stream that ends early answers nil ───────────────────────────────
##
## `read-exact` is all-or-nothing, and the fiber must be resumed to hear
## it. The buffer running out used to abandon the operation instead, which
## parked the fiber for good.
(assert (nil? (with-text-peer [wide] (fn [p] (port/read-exact p 40))))
        "a stream that ends before the count answers nil")
(println "  5. short stream answers nil")

(println "port-text-framing: ok")
