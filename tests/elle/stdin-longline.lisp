(elle/epoch 12)
## tests/elle/stdin-longline.lisp
##
## A line on STDIN longer than the buffer `port/read-line` reserves is
## answered without loss, and the read after it still frames the stream.
## See docs/io.md § "A read that overshoots keeps the rest for the same
## port": what a read reserves is not a bound on what it answers with.
##
## The trap is the stdin sibling of the one tests/elle/port-longline.lisp
## names — treating the reserved buffer as a bound. Stdin has its own
## worker (`src/io/threadpool/stdin.rs`) and its own converter, and a
## converter that stages the worker's bytes into the fiber's
## pre-allocated buffer under `data.len().min(dst_cap)` drops everything
## past 64 KiB. `read_line_with_cancel` reads to the newline however far
## away it is, so those bytes are already out of the kernel and nothing is
## left to read them again: the fiber gets a truncated line with no way to
## tell that it was truncated.
##
## Stdin reaches the StdinThread on either backend, so unlike
## port-longline this file has no `--no-uring` counterpart to pin: there
## is one mechanism, and it is the one that runs here.
##
## The counter-factual: a payload under 64 KiB passes every assertion here
## whether or not the bytes past the reservation survive. The line has to
## outgrow the reservation before the property is measurable, which is why
## the payload is 200 KiB.
##
## We test through a subprocess so `make test` can run this without piping
## stdin into the runner itself.

(def line-size 200000)

(def long-line
  (let [@buf @""
        @i 0]
    (while (< i line-size)
      (push buf (string (mod i 10)))
      (assign i (+ i 1)))
    (freeze buf)))

## The child joins what each read answers with, the way port-longline
## does, so a backend that answers a long line in pieces and one that
## answers it whole both pass — what is pinned is that no byte is lost. It
## rebuilds the same digit pattern and compares, so a read that answers
## with the right LENGTH but the wrong bytes still fails.
(def inner-script
  "(def line-size 200000)
   (def expected
     (let [@buf @\"\"
           @i 0]
       (while (< i line-size)
         (push buf (string (mod i 10)))
         (assign i (+ i 1)))
       (freeze buf)))
   (def first-line
     (let [@got @\"\"
           @more true]
       (while (and more (< (length got) line-size))
         (let [piece (port/read-line (*stdin*))]
           (if (nil? piece) (assign more false) (push got piece))))
       (freeze got)))
   (def second-line (port/read-line (*stdin*)))
   (println (length first-line))
   (println (if (= first-line expected) \"same\" \"differs\"))
   (println second-line)
   (sys/exit 0)")

(def elle-bin
  (cond
    (file/exists? "./target/release/elle") "./target/release/elle"
    (file/exists? "./target/debug/elle") "./target/debug/elle"
    true (error {:error :test-skip
                 :message "cannot find elle binary in ./target/"})))

(def scratch (file/mktempdir))
(def inner-path (path/join scratch "stdin-longline-inner.lisp"))
(def input-path (path/join scratch "stdin-longline-input.txt"))
(file/write inner-path inner-script)

## The long line, then a short one. The second line is what proves the
## overshoot was framed rather than merely delivered: a converter that
## mislays the remainder loses it or replays part of the first line here.
(file/write input-path (concat long-line "\ntail\n"))

(def result
  (subprocess/system "sh"
                     ["-c"
                      (string "cat '" input-path "' | '" elle-bin "' '"
                              inner-path "'")]))

(assert (= result:exit 0)
        (string "subprocess exited " result:exit ": " result:stderr))

(def lines (string/split (string/trim result:stdout) "\n"))
(def reported-length (get lines 0))
(def byte-verdict (get lines 1))
(def next-line (get lines 2))

(assert (= reported-length (string line-size))
        (string "the whole line is answered: got " reported-length " of "
                (string line-size)))
(println "  1. a stdin line past its buffer is answered whole")

(assert (= byte-verdict "same") "and byte for byte, not merely the right length")
(println "  2. and byte for byte")

(assert (= next-line "tail")
        (string "the next read resumes after the newline: got " next-line))
(println "  3. the read after it frames the stream correctly")

(println "stdin-longline: ok")
