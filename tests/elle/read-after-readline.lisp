(elle/epoch 9)
## tests/elle/read-after-readline.lisp — port/read must return exact byte
## count after port/read-line buffered excess data from the same fd.
##
## Regression test for: async backend IoOp::Read returning a short read
## when the fd_state buffer has fewer bytes than requested (the buffer
## held leftovers from a prior port/read-line that read past the newline).

(println "read-after-readline: starting...")

# ── Test 1: single read-line then read ──────────────────────────────

# File contains two RESP-style responses concatenated:
#   "+OK\r\n"  (simple string)
#   "$5\r\nhello\r\n"  (bulk string: 5-byte body + \r\n)
#
# port/read-line reads "+OK", may buffer "$5\r\nhello\r\n".
# port/read-line reads "$5", may buffer "hello\r\n".
# port/read 7 must return exactly "hello\r\n" (7 bytes), not a short read.

(spit "/tmp/elle-test-read-after-readline" "+OK\r\n$5\r\nhello\r\n")

(let [p (port/open "/tmp/elle-test-read-after-readline" :read)]
  (defer (port/close p)
         (let [line1 (port/read-line p)]
           (println (string/join ["  line1: " (string line1)] ""))
           (assert (= line1 "+OK") "line 1 content"))
         (let [line2 (port/read-line p)]
           (println (string/join ["  line2: " (string line2)] ""))
           (assert (= line2 "$5") "line 2 content"))
         (println "  about to port/read 7...")
         (let [body (port/read p 7)]
           (assert (not (nil? body)) "read returned data")
           (assert (= (string/size-of body) 7)
                   (string/join ["expected 7 bytes, got "
                                 (string (string/size-of body))]
                                ""))
           (assert (= (string body) "hello\r\n") "read body content"))))

(println "  single read-after-readline: ok")

# ── Test 2: 20 sequential header+body pairs ─────────────────────────
#
# Each pair is "$N\r\n<N bytes>\r\n" — a RESP bulk string.
# port/read-line reads the header, port/read reads the body.
# Catches accumulated buffer corruption across many operations.

(defn make-bulk-sequence [n]
  "Build a string of n concatenated RESP bulk strings: $1\\r\\n0\\r\\n$1\\r\\n1\\r\\n..."
  (def buf (thaw ""))
  (def @i 0)
  (while (< i n)
    (let [val (string i)]
      (push buf
            (string/join ["$" (string (string/size-of val)) "\r\n" val "\r\n"]
                         "")))
    (assign i (+ i 1)))
  (freeze buf))

(spit "/tmp/elle-test-read-after-readline-multi" (make-bulk-sequence 20))

(let [p (port/open "/tmp/elle-test-read-after-readline-multi" :read)]
  (defer (port/close p)
         (def @i 0)
         (while (< i 20)
           (let [header (port/read-line p)]
             (assert (not (nil? header))
                     (string/join ["round " (string i) ": header is nil"] ""))
             (let [expected-len (parse-int (slice header 1))]
               (let [body (port/read p (+ expected-len 2))]
                 (let [val (slice (string body) 0 expected-len)]
                   (assert (= val (string i))
                           (string/join ["round "
                                        (string i)
                                        ": expected "
                                        (string i)
                                        " got "
                                        val]
                                        ""))))))
           (assign i (+ i 1)))))

(println "  20 sequential bulk reads: ok")

(println "read-after-readline: all tests passed")
