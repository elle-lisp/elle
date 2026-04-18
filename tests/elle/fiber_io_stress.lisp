## Stream I/O stress tests
##
## Tests sustained port/lines, stream/collect, and ReadLine EOF handling.
## User code runs in the async scheduler by default — no ev/run needed.

# ============================================================================
# ReadLine EOF: last line without trailing newline
# ============================================================================

(spit "/tmp/elle-test-io-stress-eof" "alpha\nbeta\ngamma")

(let ((p (port/open "/tmp/elle-test-io-stress-eof" :read)))
  (assert (= (port/read-line p) "alpha") "readline eof: first line")
  (assert (= (port/read-line p) "beta") "readline eof: second line")
  (assert (= (port/read-line p) "gamma") "readline eof: unterminated last line")
  (assert (= (port/read-line p) nil) "readline eof: nil after EOF"))

(let ((lines (stream/collect (port/lines (port/open "/tmp/elle-test-io-stress-eof" :read)))))
  (assert (= lines (list "alpha" "beta" "gamma"))
          "port/lines: includes unterminated last line"))

# ============================================================================
# port/lines + stream/collect basic
# ============================================================================

(spit "/tmp/elle-test-io-stress-1" "line1\nline2\nline3\n")

(let ((lines (stream/collect (port/lines (port/open "/tmp/elle-test-io-stress-1" :read)))))
  (assert (= lines (list "line1" "line2" "line3")) "port/lines: basic multi-line"))

# ============================================================================
# Sustained sequential stream reads (15 files)
# ============================================================================

(let ((@i 0))
  (while (< i 15)
    (spit (string "/tmp/elle-test-io-stress-seq-" i) (string "content-" i "\n"))
    (assign i (+ i 1))))

(let ((@i 0))
  (while (< i 15)
    (let ((lines (stream/collect
                   (port/lines (port/open (string "/tmp/elle-test-io-stress-seq-" i) :read)))))
      (assert (= lines (list (string "content-" i)))
              (string "sustained sequential read: file " i)))
    (assign i (+ i 1))))

# ============================================================================
# Sustained port/write + port/flush
# ============================================================================

(let ((p (port/open "/tmp/elle-test-io-stress-write" :write)))
  (let ((@i 0))
    (while (< i 20)
      (port/write p (string "line " i "\n"))
      (assign i (+ i 1))))
  (port/flush p)
  (port/close p))

(let ((lines (stream/collect
               (port/lines (port/open "/tmp/elle-test-io-stress-write" :read)))))
  (assert (= (length lines) 20) (string "sustained write: got " (length lines) " lines"))
  (assert (= (first lines) "line 0") "sustained write: first line")
  (assert (= (get lines 19) "line 19") "sustained write: last line"))

# ============================================================================
# Repeated reads (was "nested ev/run")
# ============================================================================

(let ((@i 0))
  (while (< i 15)
    (let ((lines (stream/collect
                   (port/lines (port/open "/tmp/elle-test-io-stress-1" :read)))))
      (assert (= (length lines) 3)
              (string "repeated read " i ": got " (length lines) " lines")))
    (assign i (+ i 1))))

# ============================================================================
# protect around reads (success path)
# ============================================================================

(let ((@i 0))
  (while (< i 15)
    (let (([ok? val] (protect
      (let ((lines (stream/collect
                     (port/lines (port/open "/tmp/elle-test-io-stress-1" :read)))))
        (assert (= (length lines) 3)
                (string "protect+read " i))))))
      (assert ok? (string "protect around read " i ": should succeed")))
    (assign i (+ i 1))))
