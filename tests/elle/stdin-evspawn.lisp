(elle/epoch 9)
## stdin + ev/spawn interaction
##
## Regression test: port/read-line on stdin must work when a long-lived
## ev/spawn fiber is active.  Before the fix, the scheduler blocked in
## wait_uring() forever because stdin completions arrive via StdinThread
## (a channel), not io_uring, and the mixed-pending path only blocked on
## io_uring.
##
## We test via subprocess so make test can run this without piping stdin.

(def inner-script
  "(ev/spawn (fn [] (ev/sleep 100000)))
   (def @count 0)
   (forever
     (let [line (port/read-line (*stdin*))]
       (when (nil? line) (break))
       (assign count (inc count))))
   (println count)
   (sys/exit 0)")

(file/write "/tmp/elle-stdin-evspawn-inner.lisp" inner-script)

# Find the elle binary. The Makefile runs us from the project root as
# ./target/{release,debug}/elle, so check both paths.
(def elle-bin
  (cond
    (file/exists? "./target/release/elle") "./target/release/elle"
    (file/exists? "./target/debug/elle")   "./target/debug/elle"
    true (error {:error :test-skip
                  :message "cannot find elle binary in ./target/"})))

(def result
  (subprocess/system "sh"
    ["-c" (string "printf 'alpha\\nbeta\\ngamma\\n' | '"
                   elle-bin "' /tmp/elle-stdin-evspawn-inner.lisp")]))

(assert (= result:exit 0)
  (string "subprocess exited " result:exit ": " result:stderr))
(def output (string/trim result:stdout))
(assert (= output "3")
  (string "expected '3', got '" output "'"))
(println "stdin-evspawn: PASS")
