(elle/epoch 18)
## audited: 2026-11-22
## `--boot-image=` at the command line: where the default stores, which flags turn it off, and which commands read it.
## tests/cli/overview.md
## docs/impl/image/boot.md
## docs/config.md
##
## Each start is a process of its own, because the policy comes from the
## process configuration.

(include-file "../runner/lib/harness.lisp")

(defn start [args &named env]
  "Start the binary with ARGS, and require the start to succeed."
  (let [r (elle-run args :env env)]
    (assert (= r:exit 0) (string "elle " args " failed: " r:err))
    r))

(defn images [dir]
  "How many `.image` files DIR's `boot` directory holds."
  (let [boot (path/join dir "boot")]
    (if (file/directory? boot)
      (length (filter (fn [n] (string/ends-with? n ".image")) (file/ls boot)))
      0)))

(defn hydrated? [r]
  "Whether a `--trace=boot` start reports a hydration."
  (any? (fn [l]
          (and (string/starts-with? l "[trace:boot]")
               (string/contains? l "image-hydrate"))) (lines r:err)))

# Whether the binary carries a boot image of its own, the embedded blob a
# release build maps at start. A hydrated blob stores nothing, so a cache
# stays empty under such a binary, and each scenario says what each kind does.
(def blob?
  (any? (fn [l] (string/starts-with? l "[trace:boot] blob-map"))
        (lines ((start ["--trace=boot" "-e" "(+ 1 2)"]) :err))))

(with-temp-dir dir
  (defn case-dir [name]
    (let [d (path/join dir name)]
      (file/mkdir-all d)
      d))

  # The counter-factual is an opt-in policy, under which neither start touches
  # the directory and every start compiles the three sources.
  (eprintln "scenario: a start with no policy hydrates, and stores what it had to compile")
  (let [d (case-dir "default")
        cache (string "--cache=" d)
        first-start (start [cache "--trace=boot" "-e" "(+ 1 2)"])]
    (if blob?
      (begin
        (assert (hydrated? first-start)
                (string "a binary that carries a blob hydrates it: "
                        first-start:err))
        (assert (= (images d) 0) "a hydrated blob stores nothing"))
      (begin
        (assert (= (images d) 1) "the first start stored one image")
        (let [next (start [cache "--trace=boot" "-e" "(+ 1 2)"])]
          (assert (hydrated? next)
                  (string "the second start did not hydrate the first's "
                          "image: " next:err))))))

  # Under a binary with no blob, the `on` start stores the image the `off`
  # start must ignore; under one with a blob, the `off` start ignores the blob.
  (eprintln "scenario: --boot-image=off reads and writes no image")
  (let [d (case-dir "off")
        cache (string "--cache=" d)]
    (start [cache "--boot-image=off" "-e" "(+ 1 2)"])
    (assert (not (file-exists? (path/join d "boot")))
            "a start with the warm cache off made its directory")
    (start [cache "--boot-image=on" "-e" "(+ 1 2)"])
    (unless blob?
      (assert (= (images d) 1) "there is an image to ignore"))
    (assert (not (hydrated? (start [cache "--boot-image=off" "--trace=boot" "-e"
                                    "(+ 1 2)"])))
            "a start with the warm cache off hydrated an image"))

  # `elle image dump-boot` boots from source, so the artifact it writes is the
  # one the three sources produce. A runtime under the default policy stores
  # into the cache when it misses, so an empty cache shows the dump's runtime
  # never read it. `ELLE_CACHE` rather than `--cache=`, because the subcommand
  # takes no flag of elle's own.
  (eprintln "scenario: dump-boot writes nothing into the cache")
  (let [d (case-dir "dump-boot")
        out (path/join d "boot.image")
        cache (path/join d "cache")]
    (start ["image" "dump-boot" out] :env (put (sys/env) "ELLE_CACHE" cache))
    (assert (file-exists? out) "dump-boot wrote no image")
    (assert (not (file-exists? (path/join cache "boot")))
            "dump-boot's runtime used the warm cache"))

  # The language server boots as `elle` does: over a cold cache it hydrates
  # the blob a binary carries, or compiles and stores an image, and it writes
  # no stdlib cache either way. The counter-factual loads the stdlib through
  # `init_stdlib` under the process cache: a stdlib cache and no image. Its
  # standard input is closed, so it exits once it booted.
  (eprintln "scenario: the language server boots as elle does and writes no stdlib cache")
  (let [d (case-dir "lsp")]
    (start ["lsp"] :env (put (sys/env) "ELLE_CACHE" d))
    (assert (= (images d) (if blob? 0 1))
            (string "the language server stored " (images d) " images"))
    (assert (not (file-exists? (path/join d "stdlib-cache")))
            "the language server wrote a stdlib cache")))

(eprintln "all boot-policy scenarios passed")
