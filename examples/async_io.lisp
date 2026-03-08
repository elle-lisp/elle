## Async I/O example: concurrent file reads
##
## Demonstrates ev/run with multiple concurrent I/O thunks.

(import "examples/assertions.lisp")

(spit "/tmp/elle-async-example-1" "hello from file one")
(spit "/tmp/elle-async-example-2" "hello from file two")

(let ((results @[]))
  (ev/run
    (fn ()
      (let ((content (stream/read-all (port/open "/tmp/elle-async-example-1" :read))))
        (push results content)))
    (fn ()
      (let ((content (stream/read-all (port/open "/tmp/elle-async-example-2" :read))))
        (push results content))))
  (assert-eq (length results) 2 "both async reads completed")
  (assert-true (> (length (get results 0)) 0) "first read got content")
  (assert-true (> (length (get results 1)) 0) "second read got content"))
