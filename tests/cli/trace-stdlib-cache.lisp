(elle/epoch 18)
## audited: 2026-11-22
## `--trace` phase marks around the stdlib disk cache, and their silence when nobody asks.
## tests/cli/overview.md
## docs/impl/stdlib-cache.md
##
## The cache and the boot marks meet in `init_stdlib`: the cache exists to skip
## the stdlib compile, the marks exist to attribute it. Loading the stdlib is
## still the phase, so it is still marked. The runs turn the boot image off,
## because an instance that hydrates one reads no stdlib cache.

(include-file "../runner/lib/harness.lisp")

(defn boot-mark? [stderr]
  "Whether STDERR holds a stdlib-compile boot mark."
  (any? (fn [l]
          (and (string/starts-with? l "[trace:boot]")
               (string/contains? l "stdlib-compile"))) (lines stderr)))

(with-temp-dir dir
  (def script (write-fixture dir "script.lisp" "(+ 1 2)\n"))
  (def flags
    [(string "--cache=" (path/join dir "cache")) "--boot-image=off"
     "--trace=boot" script])

  # The trap: the cache-hit path returns early, so a mark placed only on the
  # compile arm vanishes once the cache starts working. The counter-factual is
  # a second run whose stderr names no stdlib phase at all.
  (eprintln "scenario: a cache hit still reports the stdlib boot phase")
  (let [first (elle-run flags)
        second (elle-run flags)]
    (assert (= first:exit 0) (string "the first run failed: " first:err))
    (assert (= second:exit 0) (string "the second run failed: " second:err))
    (assert (boot-mark? first:err)
            (string "the first run reports no stdlib-compile mark: " first:err))
    (assert (boot-mark? second:err)
            (string "the second run reports no stdlib-compile mark: " second:err))
    (assert (string/contains? second:err "stdlib-compile (cache hit)")
            (string "the second run must load the stdlib the first stored: "
                    second:err)))

  # A diagnostic that prints when nobody asked gets filtered out and ignored.
  (eprintln "scenario: phase marks stay quiet without the keyword")
  (let [r (elle-run [script])]
    (assert (= r:exit 0) (string "elle exited non-zero: " r:err))
    (each subsystem ["[trace:boot]" "[trace:compile]"]
      (assert (not (string/contains? r:err subsystem))
              (string "a " subsystem " mark printed without --trace: " r:err)))))

(eprintln "all trace-stdlib-cache scenarios passed")
