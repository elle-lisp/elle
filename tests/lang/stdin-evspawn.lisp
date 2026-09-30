(elle/epoch 13)
# audited: 2026-09-30
## port/read-line on stdin answers while a long-lived ev/spawn fiber is pending.
## docs/io.md
##
## Stdin completions arrive through the StdinThread's channel, not through
## io_uring. The counter-factual: a scheduler that blocks in wait_uring alone
## while a fiber is pending never sees them, and the child hangs until the
## runner's deadline kills it.
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

(def scratch (file/mktempdir))
(def inner-path (path/join scratch "stdin-evspawn-inner.lisp"))
(file/write inner-path inner-script)

(def elle-bin (elle/executable))

# The inner-script path must be spliced into the shell string at runtime —
# a literal inside the string would not see the scratch binding.
(def result
  (subprocess/system "sh"
                     ["-c"
                      (string "printf 'alpha\\nbeta\\ngamma\\n' | '" elle-bin
                              "' '" inner-path "'")]))

(assert (= result:exit 0)
        (string "subprocess exited " result:exit ": " result:stderr))
(def output (string/trim result:stdout))
(assert (= output "3") (string "expected '3', got '" output "'"))
(file/delete-dir-all scratch)
(println "stdin-evspawn: PASS")
