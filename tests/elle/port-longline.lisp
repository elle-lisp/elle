(elle/epoch 12)
## audited: 2026-09-23
## tests/elle/port-longline.lisp
##
## A line longer than the buffer `port/read-line` reserves is answered
## without loss: successive reads hand back its pieces, in order, each no
## longer than that buffer, and the pieces reassemble the line byte for
## byte. See docs/io.md § "A read that overshoots keeps the rest for the
## same port" and docs/impl/io-bytes.md.
##
## The traps, each a way to treat the reserved buffer wrongly:
##
##   - Copying the answer into the buffer clamped to its size drops every
##     byte past 64 KiB. The port has already taken those bytes from the
##     kernel, so nothing is left to read them again, and the next read
##     reports that the stream ended.
##   - Abandoning the operation when the buffer fills with no newline
##     leaves the fiber that asked parked with no completion coming: a
##     hang, not a short read.
##   - Reading on past the buffer to the newline answers with the whole
##     line in one piece. That loses nothing, but the other backend answers
##     in pieces, so the same program sees different reads on each; and it
##     stages the line outside the caller's region.
##
## The counter-factual: a payload under 64 KiB passes every assertion
## whatever the backend does. The line has to outgrow the reservation
## before any trap is reachable, which is why the payload here is 200 KiB.
## Run on the other backend by `port_longline_threadpool`.

(defn listener-port [listener]
  "The port number a listener bound to an ephemeral port received."
  (parse-int (get (string/split (port/path listener) ":") 1)))

## Over `READ_LINE_BUF_SIZE` (64 KiB) several times over, and over the
## loopback receive buffer, so the peer's write lands in several segments
## the way a real protocol's would.
(def line-size 200000)
(def read-line-buffer 65536)

(def long-line
  (let [@buf @""
        @i 0]
    (while (< i line-size)
      (push buf (string (mod i 10)))
      (assign i (+ i 1)))
    (freeze buf)))

(defn read-whole-line [p]
  "Read until `line-size` bytes have arrived, joining what each read
   answers with, and report the longest piece beside the joined line. A
   read that reports the stream ended stops the loop, so a backend that
   loses bytes shows up as a short result rather than a spin."
  (let [@got @""
        @longest 0
        @more true]
    (while (and more (< (length got) line-size))
      (let [piece (port/read-line p)]
        (if (nil? piece)
          (assign more false)
          (begin
            (assign longest (max longest (length piece)))
            (push got piece)))))
    [(freeze got) longest]))

(let [listener (tcp/listen "127.0.0.1" 0)
      port-num (listener-port listener)]
  (ev/spawn (fn []
              (let [conn (tcp/accept listener)]
                (port/write conn (concat long-line "\n"))
                (port/flush conn)
                (ev/sleep 0.3)
                (port/close conn))))
  (let [client (tcp/connect "127.0.0.1" port-num :encoding :text)
        [got longest] (read-whole-line client)]
    (assert (= (length got) line-size)
            (concat "the whole line is answered: got " (string (length got))
                    " of " (string line-size)))
    (assert (= got long-line) "and byte for byte, not merely the right length")
    (assert (<= longest read-line-buffer)
            (concat "each piece fits the buffer it was read into: the longest was "
                    (string longest) " bytes of a " (string read-line-buffer)
                    "-byte buffer"))
    (port/close client)
    (port/close listener)))
(println "  1. a line past its buffer is answered whole, in pieces that fit it")

(println "port-longline: ok")
