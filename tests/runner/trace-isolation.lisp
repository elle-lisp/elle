(elle/epoch 18)
## audited: 2026-11-22
## A file that turns on a trace leaves it on for its own instance, and no other file in the run traces.
## tests/runner/overview.md
## docs/config.md
##
## Each file runs in a worker VM with its own trace cell, and the readers off
## the VM, such as the region-page claims, read their instance's cell. The
## counter-factual: `(vm/config-set :trace |:pages|)` wrote one process-wide bit
## that every region pool read, so one file that set it and never cleared it
## made every later file and the runner's own work print `[trace:pages]`. The
## run of P then Q is compared with P alone, so the lines Q adds are counted
## exactly; a fixed bound would drift with what P's own instance prints.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn pages-lines [name files]
    "The `[trace:pages]` lines one run over FILES prints to stderr."
    (let [r (elle-test (path/join dir (string name ".db")) files)]
      (length (filter (fn [l] (string/contains? l "[trace:pages]"))
                      (lines r:err)))))

  (def p
    (write-fixture dir "p-enables-pages.lisp"
                   (string "(vm/config-set :trace |:pages|)\n"
                           "(assert (= 1 2) \"P aborts with :pages still enabled\")\n")))
  (def q
    (write-fixture dir "q-allocates.lisp"
                   (string "(defn build [n] (if (= n 0) [] (concat [n] (build (- n 1)))))\n"
                           "(assert (= (length (build 400)) 400) \"Q allocates a lot\")\n")))

  (eprintln "scenario: a file that never enables :pages prints no page trace")
  (let [alone (pages-lines "q" [q])]
    (assert (= alone 0)
            (string "an allocating file that never enables :pages printed "
                    alone " [trace:pages] lines")))

  (eprintln "scenario: one file's :pages does not reach the next file")
  (let [own (pages-lines "p" [p])
        together (pages-lines "pq" [p q])]
    (assert (> own 0) "the file that enables :pages traces its own instance")
    (assert (= together own)
            (string "a file enabling :pages must trace its own instance alone; "
                    "it printed " own " lines alone and " together
                    " with a file after it")))

  (eprintln "all trace-isolation scenarios passed"))
