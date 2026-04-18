(elle/epoch 8)
## tests/elle/readline-long.lisp — Verify port/read-line handles lines > 4096 bytes
##
## This tests the fix for the 4096-byte-per-read limit in the async backend.

# Build a string that exceeds 4096 bytes by repeated concatenation
(defn make-long-string [n]
  (def buf @"")
  (def @i 0)
  (while (< i n)
    (push buf "x")
    (assign i (+ i 1)))
  (freeze buf))

(def long-line (make-long-string 8192))
(assert (= (string/size-of long-line) 8192) "setup: long-line is 8192 bytes")

# Write it to a temp file as a single line (with trailing newline)
(spit "/tmp/elle-readline-long-test" (string/join [long-line "\n"] ""))

# Read it back with port/read-line
(let [p (port/open "/tmp/elle-readline-long-test" :read)]
  (defer (port/close p)
    (let [line (port/read-line p)]
      (assert (= (string/size-of line) 8192)
        (string/join ["expected 8192 bytes, got " (string (string/size-of line))] ""))
      (assert (= line long-line) "read-line content mismatch"))))

# Also test a line well under the limit still works
(spit "/tmp/elle-readline-short-test" "hello\nworld\n")
(let [p (port/open "/tmp/elle-readline-short-test" :read)]
  (defer (port/close p)
    (let [line1 (port/read-line p)
          line2 (port/read-line p)
          line3 (port/read-line p)]
      (assert (= line1 "hello") "short line 1")
      (assert (= line2 "world") "short line 2")
      (assert (nil? line3) "EOF after last line"))))

# Test multiple long lines in sequence
(def medium-line (make-long-string 5000))
(spit "/tmp/elle-readline-multi-test"
  (string/join [long-line "\n" medium-line "\n" "short\n"] ""))
(let [p (port/open "/tmp/elle-readline-multi-test" :read)]
  (defer (port/close p)
    (let [l1 (port/read-line p)
          l2 (port/read-line p)
          l3 (port/read-line p)]
      (assert (= (string/size-of l1) 8192) "multi: line 1 length")
      (assert (= (string/size-of l2) 5000) "multi: line 2 length")
      (assert (= l3 "short") "multi: line 3 content"))))

(println "readline-long: all tests passed")
