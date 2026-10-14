(elle/epoch 15)
# audited: 2026-10-14
# The file/ calls that move contents run on the I/O pool: other fibers run while one waits, and each needs :fs and :io.
# docs/io.md
#
# Each call below waits about 0.3 s for the other end of a fifo, which a shell
# opens late. The trap: a call that ran on the scheduler's thread would block
# that thread, and the ticking fiber could not run until the call returned, so
# it would count no tick. Three ticks is a floor far below the thirty that
# 0.3 s allows, so a slow machine passes.

(defn fails-with [kind thunk]
  "Run thunk, and answer the error it raises after checking its kind."
  (let [[ok? err] (protect (thunk))]
    (assert (not ok?) (string "expected an error of kind " kind))
    (assert (= (get err :error) kind) (string "the kind is " kind ": " err))
    err))

(defn while-ticking [thunk]
  "Run thunk while another fiber counts 10 ms sleeps. Answers [value ticks]."
  (let [ticks (box 0)
        ticker (ev/spawn (fn []
                           (forever
                             (ev/sleep 0.01)
                             (rebox ticks (+ 1 (unbox ticks))))))
        v (thunk)]
    (ev/abort ticker)
    [v (unbox ticks)]))

(defn late [script]
  "Run script in a shell after 0.3 s."
  (subprocess/exec "/bin/sh" ["-c" (string "sleep 0.3; " script)]))

(defn quoted [path]
  (string "'" path "'"))

(with-temp-dir dir
               (let [fifo (path/join dir "fifo")
                     out (path/join dir "out")
                     dest (path/join dir "dest")
                     text "one\ntwo\n"
                     feed (string "printf 'one\\ntwo\\n' > " (quoted fifo))
                     drain (string "cat " (quoted fifo) " > " (quoted out))]
                 (assert (= 0
                            (subprocess/wait (subprocess/exec "mkfifo" [fifo])))
                         "mkfifo made the fifo")

                 (let [shell (late feed)
                       [v ticks] (while-ticking (fn [] (file/read fifo)))]
                   (subprocess/wait shell)
                   (assert (= v text) "file/read reads what the writer wrote")
                   (assert (>= ticks 3) "other fibers run while file/read waits"))

                 (let [shell (late feed)
                       [v ticks] (while-ticking (fn [] (file/lines fifo)))]
                   (subprocess/wait shell)
                   (assert (= v (list "one" "two")) "file/lines reads the lines")
                   (assert (>= ticks 3)
                           "other fibers run while file/lines waits"))

                 # A copy's source must be a regular file, so the fifo is the
                 # destination, and the copy waits for its reader.
                 (file/write dest text)
                 (let [shell (late drain)
                       [v ticks] (while-ticking (fn [] (file/copy dest fifo)))]
                   (subprocess/wait shell)
                   (assert (= v true) "file/copy answers true")
                   (assert (= (file/read out) text)
                           "and the reader got what it copied")
                   (assert (>= ticks 3) "other fibers run while file/copy waits"))

                 (let [shell (late drain)
                       [v ticks] (while-ticking (fn [] (file/write fifo text)))]
                   (subprocess/wait shell)
                   (assert (= v true) "file/write answers true")
                   (assert (= (file/read out) text)
                           "and the reader got what it wrote")
                   (assert (>= ticks 3)
                           "other fibers run while file/write waits"))

                 (let [shell (late drain)
                       [v ticks] (while-ticking (fn [] (file/append fifo text)))]
                   (subprocess/wait shell)
                   (assert (= v true) "file/append answers true")
                   (assert (= (file/read out) text)
                           "and the reader got what it wrote")
                   (assert (>= ticks 3)
                           "other fibers run while file/append waits"))

                 # A failure names the file it failed on: for a copy, the file it copies.
                 (let [missing (path/join dir "missing" "file")]
                   (each thunk in [(fn [] (file/read missing))
                                   (fn [] (file/lines missing))
                                   (fn [] (file/write missing "x"))
                                   (fn [] (file/append missing "x"))
                                   (fn [] (file/copy missing dest))]
                     (let [err (fails-with :io-error thunk)]
                       (assert (= (get err :path) missing)
                               "the error's :path names the file"))))

                 # file/read answers text, so bytes that are not UTF-8 raise.
                 (let [raw (path/join dir "raw")]
                   (subprocess/wait (subprocess/exec "/bin/sh"
                                    ["-c"
                                     (string "printf '\\377a' > " (quoted raw))]))
                   (fails-with :io-error (fn [] (file/read raw))))

                 # Each needs :io as well as :fs. A child denied :io alone traps on the call.
                 (each [name thunk] in [["file/read" (fn [] (file/read out))]
                                        ["file/lines" (fn [] (file/lines out))]
                                        ["file/write"
                                        (fn [] (file/write out "x"))]
                                        ["file/append"
                                        (fn [] (file/append out "x"))]
                                        ["file/copy"
                                        (fn [] (file/copy out dest))]]
                   (let [child (fiber/new thunk |:io :error| :deny |:io|)]
                     (fiber/resume child)
                     (assert (= (fiber/status child) :paused)
                             (string "a child without :io traps on " name))
                     (assert (= name (get (fiber/value child) :primitive))
                             (string "and the trap names " name))))))

(println "tests/lang/file-content.lisp: all tests passed")
