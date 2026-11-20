(elle/epoch 18)
# audited: 2026-11-20
# The file/ calls that move contents go through ports: other fibers run while one waits, and a cancel stops it.
# docs/io/files.md
#
# Each call below waits about 0.3 s for the other end of a fifo, which a shell
# opens late. The trap: a call that ran on the scheduler's thread would block
# that thread, and the ticking fiber could not run until the call returned, so
# it would count no tick. Three ticks is a floor far below the thirty that
# 0.3 s allows, so a slow machine passes.
#
# The counter-factual for the cancel: a call that ran as one blocking system
# call on a pool worker keeps that worker after `ev/timeout` gives up, because
# nothing reaches a thread inside `open(2)` on a fifo. The busy count in
# `ev/report` then never falls back.

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

(defn workers-settle [baseline]
  "Wait up to 2 s for the busy I/O workers to fall back to `baseline`, and
   answer the last count."
  (let [until (+ (clock/monotonic) 2)
        @n (get (ev/report) :workers)]
    (while (and (> n baseline) (< (clock/monotonic) until))
      (ev/sleep 0.01)
      (assign n (get (ev/report) :workers)))
    n))

(with-temp-dir dir
  (let [fifo (path/join dir "fifo")
        out (path/join dir "out")
        dest (path/join dir "dest")
        text "one\ntwo\n"
        feed (string "printf 'one\\ntwo\\n' > " (quoted fifo))
        drain (string "cat " (quoted fifo) " > " (quoted out))]
    (assert (= 0 (subprocess/wait (subprocess/exec "mkfifo" [fifo])))
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
      (assert (>= ticks 3) "other fibers run while file/lines waits"))

    (file/write dest text)
    (let [shell (late drain)
          [v ticks] (while-ticking (fn [] (file/copy dest fifo)))]
      (subprocess/wait shell)
      (assert (= v true) "file/copy answers true")
      (assert (= (file/read out) text) "and the reader got what it copied")
      (assert (>= ticks 3) "other fibers run while file/copy waits"))

    # A copy reads its source as a stream, so a fifo can be the source too.
    (let [copied (path/join dir "copied")
          shell (late feed)
          [v ticks] (while-ticking (fn [] (file/copy fifo copied)))]
      (subprocess/wait shell)
      (assert (= v true) "a copy from a fifo answers true")
      (assert (= (file/read copied) text) "and holds what the writer wrote")
      (assert (>= ticks 3) "other fibers run while that copy waits"))

    (let [shell (late drain)
          [v ticks] (while-ticking (fn [] (file/write fifo text)))]
      (subprocess/wait shell)
      (assert (= v true) "file/write answers true")
      (assert (= (file/read out) text) "and the reader got what it wrote")
      (assert (>= ticks 3) "other fibers run while file/write waits"))

    (let [shell (late drain)
          [v ticks] (while-ticking (fn [] (file/append fifo text)))]
      (subprocess/wait shell)
      (assert (= v true) "file/append answers true")
      (assert (= (file/read out) text) "and the reader got what it wrote")
      (assert (>= ticks 3) "other fibers run while file/append waits"))

    # ── A cancel stops a call that waits ───────────────────────────────

    # Nobody opens the fifo's other end, so each call waits until the deadline.
    (let [baseline (get (ev/report) :workers)]
      (assert (nil? (ev/timeout 0.2 (fn [] (file/read fifo))))
              "the deadline ends a read nobody writes to")
      (assert (= baseline (workers-settle baseline))
              "and the read gives its worker back")
      (assert (nil? (ev/timeout 0.2 (fn [] (file/write fifo text))))
              "the deadline ends a write nobody reads")
      (assert (= baseline (workers-settle baseline))
              "and the write gives its worker back"))

    # ── The errors are the port calls' own ─────────────────────────────

    # An open that fails names the file it failed on.
    (let [missing (path/join dir "missing" "file")]
      (each thunk in [(fn [] (file/read missing)) (fn [] (file/lines missing))
                      (fn [] (file/write missing "x"))
                      (fn [] (file/append missing "x"))
                      (fn [] (file/copy missing dest))]
        (let [err (fails-with :io-error thunk)]
          (assert (= (get err :path) missing) "the error's :path names the file")))
      (let [err (fails-with :io-error (fn [] (file/copy dest missing)))]
        (assert (= (get err :path) missing)
                "a copy that cannot open its destination names the destination")))

    # Text that is not UTF-8 is an encoding error, as on any text port.
    (let [raw (path/join dir "raw")]
      (subprocess/wait (subprocess/exec "/bin/sh"
                                        ["-c"
                                         (string "printf '\\377a' > "
                                           (quoted raw))]))
      (fails-with :encoding-error (fn [] (file/read raw))))

    # The content is checked before the open, which would truncate the file.
    (file/write out "kept")
    (fails-with :type-error (fn [] (file/write out 42)))
    (assert (= (file/read out) "kept")
            "a refused write leaves the file as it was")

    # Each needs :io as well as :fs, and the open is where a denial stops it.
    (each [name thunk] in [["file/read" (fn [] (file/read out))]
                           ["file/lines" (fn [] (file/lines out))]
                           ["file/write" (fn [] (file/write out "x"))]
                           ["file/append" (fn [] (file/append out "x"))]
                           ["file/copy" (fn [] (file/copy out dest))]]
      (let [child (fiber/new thunk |:io :error| :deny |:io|)]
        (fiber/resume child)
        (assert (= (fiber/status child) :paused)
                (string "a child without :io traps in " name))
        (assert (= "port/open" (get (fiber/value child) :primitive))
                (string "and " name "'s trap names port/open"))))))

(println "tests/lang/file-content.lisp: all tests passed")
