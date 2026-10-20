(elle/epoch 15)
# audited: 2026-10-20
## A line longer than port/read-line's buffer is one answer, and the read after it answers the next line.
## docs/io.md
##
## `port/read-line` reserves a 64 KiB buffer, and what it reserves is not a
## bound on what it answers with: a longer line is read on to its newline and
## answered whole, on a file, a pipe and a socket alike. See docs/io.md and
## docs/impl/io-bytes.md.
##
## The traps, each a way to treat the reserved buffer wrongly:
##
##   - Answering with the full buffer when it fills with no newline hands the
##     caller a piece nothing marks as one, because `read-line` drops the
##     newline from every answer. A 200000-byte line then reads as four lines
##     of 65536, 65536, 65536 and 3392 bytes. A line of exactly 65536 bytes
##     is worse: the next read starts at the newline and answers an empty
##     line the stream does not hold.
##   - Copying the answer into the buffer clamped to its size drops every
##     byte past 64 KiB. The port has already taken those bytes from the
##     kernel, so nothing is left to read them again, and the next read
##     reports that the stream ended.
##   - Abandoning the operation when the buffer fills with no newline
##     leaves the fiber that asked parked with no completion coming: a
##     hang, not a short read.
##
## The counter-factual: a line under 64 KiB passes every assertion here
## whatever the backend does with a full buffer. The lines are therefore one
## byte under the buffer, the buffer's length exactly, one byte over it, and
## three times it. The thread-pool build runs this file too, so both
## backends answer it.

(def read-line-buffer 65536)

(defn digits [n]
  "A string of n digits, i mod 10 at position i, so a wrong byte shows as
   well as a wrong length."
  (let [@buf @""
        @i 0]
    (while (< i n)
      (push buf (string (mod i 10)))
      (assign i (+ i 1)))
    (freeze buf)))

(defn line-lengths [p]
  "The length of each line port/read-line answers from p, in order, until
   it reports the end of the stream. Closes p."
  (let [@got @[]
        @line (port/read-line p)]
    (while (not (nil? line))
      (push got (length line))
      (assign line (port/read-line p)))
    (port/close p)
    (freeze got)))

## A file port, around the buffer's length and well past it.
(with-temp-dir dir
               (let [path (path/join dir "lines.txt")]
                 (each n in [(- read-line-buffer 1) read-line-buffer
                             (+ read-line-buffer 1) 200000]
                   (file/write path (string (digits n) "\nyy\n"))
                   (let [lengths (line-lengths (port/open path :read))]
                     (assert (= [n 2] lengths)
                             (string "a " n
                                     "-byte line then \"yy\" is two lines of " n
                                     " and 2 bytes: got " lengths))))
                 (println "  1. a file's long line is one answer, and the next read is the next line")

                 ## Byte for byte, not merely the right length.
                 (let [line (digits 200000)]
                   (file/write path (string line "\nyy\n"))
                   (let [p (port/open path :read)]
                     (assert (= line (port/read-line p))
                             "the long line comes back byte for byte")
                     (assert (= "yy" (port/read-line p))
                             "and the read after it answers the next line")
                     (assert (nil? (port/read-line p))
                             "and the one after that reports the end")
                     (port/close p)))
                 (println "  2. and byte for byte")

                 ## A `\r` as the buffer's last byte belongs to the newline that follows it
                 ## in the next read, not to the line.
                 (let [line (string/repeat "x" (- read-line-buffer 1))]
                   (file/write path (string line "\r\nyy\r\n"))
                   (let [p (port/open path :read)]
                     (assert (= line (port/read-line p))
                             "a \\r that fills the buffer's last byte goes with the newline after it")
                     (assert (= "yy" (port/read-line p))
                             "and the next line follows it")
                     (port/close p)))
                 (println "  3. a \\r\\n split across the buffer's end is one terminator")

                 ## A stream that ends mid-line past the buffer answers the partial line
                 ## whole, then the end.
                 (file/write path (digits 200000))
                 (assert (= [200000] (line-lengths (port/open path :read)))
                         "a partial last line past the buffer is one answer")
                 (println "  4. a partial last line past the buffer is one answer")))

## A pipe from a child, whose writer is paced by the pipe's own buffer.
(let* [peer (subprocess/exec "/bin/sh"
                             ["-c"
                              "head -c 200000 /dev/zero | tr '\\0' x; echo; echo yy"])
       out (get peer :stdout)
       line (port/read-line out)]
  (assert (= 200000 (length line))
          (string "a pipe's 200000-byte line is one answer: got " (length line)))
  (assert (= "yy" (port/read-line out))
          "and the read after it answers the next line")
  (subprocess/wait peer))
(println "  5. a pipe's long line is one answer")

## A socket, whose peer's write lands in several segments the way a real
## protocol's would. The line is `READ_LINE_BUF_SIZE` several times over, and
## over the loopback receive buffer.
(defn listener-port [listener]
  "The port number a listener bound to an ephemeral port received."
  (parse-int (get (string/split (port/path listener) ":") 1)))

(def long-line (digits 200000))

(let [listener (tcp/listen "127.0.0.1" 0)
      port-num (listener-port listener)]
  (ev/spawn (fn []
              (let [conn (tcp/accept listener)]
                (port/write conn (concat long-line "\nyy\n"))
                (port/flush conn)
                (ev/sleep 0.3)
                (port/close conn))))
  (let [client (tcp/connect "127.0.0.1" port-num :encoding :text)
        got (port/read-line client)]
    (assert (= (length got) 200000)
            (string "a socket's 200000-byte line is one answer: got "
                    (length got)))
    (assert (= got long-line) "and byte for byte, not merely the right length")
    (assert (= "yy" (port/read-line client))
            "and the read after it answers the next line")
    (port/close client)
    (port/close listener)))
(println "  6. a socket's long line is one answer")

(println "port-longline: ok")
