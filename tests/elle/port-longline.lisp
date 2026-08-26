(elle/epoch 12)
## tests/elle/port-longline.lisp
##
## A line longer than the buffer `port/read-line` reserves is answered
## without loss: successive reads hand back its pieces, in order, and the
## pieces reassemble the line byte for byte. See docs/io.md § "A read that
## overshoots keeps the rest for the same port".
##
## The trap, one per backend, both from the same mistake — treating the
## reserved buffer as a bound on what a read may answer with:
##
##   - The thread-pool worker reads to the newline however far away it is,
##     and its bytes were then copied into that buffer clamped to its
##     size. Everything past 64 KiB was dropped. The bytes were already
##     out of the kernel, so nothing was left to read them again, and the
##     next read reported the stream had ended.
##   - The io_uring resubmit loop found no room left in the same buffer
##     and abandoned the operation without a completion. The fiber that
##     asked was never resumed — a hang, not a short read.
##
## The counter-factual: a payload under 64 KiB passes both assertions on
## the unfixed code. The line has to outgrow the reservation before either
## defect is reachable, which is why the payload here is 200 KiB.

(defn listener-port [listener]
  "The port number a listener bound to an ephemeral port received."
  (parse-int (get (string/split (port/path listener) ":") 1)))

## Over `READ_LINE_BUF_SIZE` (64 KiB) several times over, and over the
## loopback receive buffer, so the peer's write lands in several segments
## the way a real protocol's would.
(def line-size 200000)

(def long-line
  (let [@buf @""
        @i 0]
    (while (< i line-size)
      (push buf (string (mod i 10)))
      (assign i (+ i 1)))
    (freeze buf)))

(defn read-whole-line [p]
  "Read until `line-size` bytes have arrived, joining what each read
   answers with. A read that reports the stream ended stops the loop, so a
   backend that loses bytes shows up as a short result rather than a spin."
  (let [@got @""
        @more true]
    (while (and more (< (length got) line-size))
      (let [piece (port/read-line p)]
        (if (nil? piece) (assign more false) (push got piece))))
    (freeze got)))

(let [listener (tcp/listen "127.0.0.1" 0)
      port-num (listener-port listener)]
  (ev/spawn (fn []
              (let [conn (tcp/accept listener)]
                (port/write conn (concat long-line "\n"))
                (port/flush conn)
                (ev/sleep 0.3)
                (port/close conn))))
  (let [client (tcp/connect "127.0.0.1" port-num :encoding :text)
        got (read-whole-line client)]
    (assert (= (length got) line-size)
            (concat "the whole line is answered: got " (string (length got))
                    " of " (string line-size)))
    (assert (= got long-line) "and byte for byte, not merely the right length")
    (port/close client)
    (port/close listener)))
(println "  1. a line past its buffer is answered whole")

(println "port-longline: ok")
