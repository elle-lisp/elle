## Stream I/O stress tests
##
## Tests sustained port/lines, stream/collect, nested ev/run,
## and ReadLine EOF handling.

# ============================================================================
# ReadLine EOF: last line without trailing newline
# ============================================================================

(spit "/tmp/elle-test-io-stress-eof" "alpha\nbeta\ngamma")

(ev/run (fn []
  (let ((p (port/open "/tmp/elle-test-io-stress-eof" :read)))
    (assert (= (stream/read-line p) "alpha") "readline eof: first line")
    (assert (= (stream/read-line p) "beta") "readline eof: second line")
    (assert (= (stream/read-line p) "gamma") "readline eof: unterminated last line")
    (assert (= (stream/read-line p) nil) "readline eof: nil after EOF"))))

(ev/run (fn []
  (let ((lines (stream/collect (port/lines (port/open "/tmp/elle-test-io-stress-eof" :read)))))
    (assert (= lines (list "alpha" "beta" "gamma"))
            "port/lines: includes unterminated last line"))))

# ============================================================================
# port/lines + stream/collect basic
# ============================================================================

(spit "/tmp/elle-test-io-stress-1" "line1\nline2\nline3\n")

(ev/run (fn []
  (let ((lines (stream/collect (port/lines (port/open "/tmp/elle-test-io-stress-1" :read)))))
    (assert (= lines (list "line1" "line2" "line3")) "port/lines: basic multi-line"))))

# ============================================================================
# Sustained sequential stream reads (15 files)
# ============================================================================

(let ((i 0))
  (while (< i 15)
    (spit (string "/tmp/elle-test-io-stress-seq-" i) (string "content-" i "\n"))
    (assign i (+ i 1))))

(ev/run (fn []
  (let ((i 0))
    (while (< i 15)
      (let ((lines (stream/collect
                     (port/lines (port/open (string "/tmp/elle-test-io-stress-seq-" i) :read)))))
        (assert (= lines (list (string "content-" i)))
                (string "sustained sequential read: file " i)))
      (assign i (+ i 1))))))

# ============================================================================
# Sustained stream/write + stream/flush
# ============================================================================

(ev/run (fn []
  (let ((p (port/open "/tmp/elle-test-io-stress-write" :write)))
    (let ((i 0))
      (while (< i 20)
        (stream/write p (string "line " i "\n"))
        (assign i (+ i 1))))
    (stream/flush p)
    (port/close p))))

(ev/run (fn []
  (let ((lines (stream/collect
                 (port/lines (port/open "/tmp/elle-test-io-stress-write" :read)))))
    (assert (= (length lines) 20) (string "sustained write: got " (length lines) " lines"))
    (assert (= (first lines) "line 0") "sustained write: first line")
    (assert (= (get lines 19) "line 19") "sustained write: last line"))))

# ============================================================================
# Nested ev/run calls
# ============================================================================

(let ((i 0))
  (while (< i 15)
    (ev/run (fn []
      (let ((lines (stream/collect
                     (port/lines (port/open "/tmp/elle-test-io-stress-1" :read)))))
        (assert (= (length lines) 3)
                (string "nested ev/run " i ": got " (length lines) " lines")))))
    (assign i (+ i 1))))

# ============================================================================
# protect around ev/run (success path)
# ============================================================================

(let ((i 0))
  (while (< i 15)
    (let (([ok? val] (protect
      (ev/run (fn []
        (let ((lines (stream/collect
                       (port/lines (port/open "/tmp/elle-test-io-stress-1" :read)))))
          (assert (= (length lines) 3)
                  (string "protect+ev/run " i))))))))
      (assert ok? (string "protect around ev/run " i ": should succeed")))
    (assign i (+ i 1))))
