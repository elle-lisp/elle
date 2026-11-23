(elle/epoch 18)
## audited: 2026-11-22
## A worker boots as its parent did: from the image its parent holds, or by compiling.
## tests/cli/overview.md
## docs/impl/image/boot.md
## docs/threads.md
##
## Each start names a boot-image directory of its own, `--boot-image=DIR`,
## which holds the warm cache alone. So a build that carries an embedded blob
## stores and hydrates there as a build without one does. The proof that an
## instance hydrated is the `image-hydrate` mark a `--trace=boot` start prints,
## one per instance, workers included. The counter-factual for each mark is a
## worker that compiles its own boot: it answers every call and prints no mark.

(include-file "../runner/lib/harness.lisp")

(defn marks [r what]
  "How many `[trace:boot]` lines of run R name WHAT."
  (length (filter (fn [l]
                    (and (string/starts-with? l "[trace:boot]")
                         (string/contains? l what))) (lines r:err))))

(with-temp-dir dir
  (defn flags [name]
    (let [home (path/join dir name)]
      [(string "--cache=" home) (string "--boot-image=" (path/join home "img"))]))

  (defn start [name args]
    "Start the binary on the directories NAME names, and require success."
    (let [r (elle-run [;(flags name) ;args])]
      (assert (= r:exit 0) (string "elle " args " failed: " r:err))
      r))

  (defn warm [name]
    "Store an image in NAME's directory, and answer NAME."
    (start name ["-e" "(+ 1 2)"])
    name)

  (eprintln "scenario: a worker of an image-booted start hydrates the image")
  (let [r (start (warm "heavy")
                 ["--trace=boot" "-e"
                  "(println (sys/join (sys/spawn (fn [] (eval '(map inc [1 2 3]))))))"])]
    (assert (= (string/trim r:out) "[2 3 4]")
            (string "the worker's stdlib answered " r:out))
    (assert (= (marks r "image-hydrate") 2)
            (string "the start and its worker each hydrate once: " r:err)))

  # The light worker installs the core exports and the macros out of the image,
  # and no stdlib export. The counter-factual for the refusal installs the whole
  # image, which resolves `+` and answers 3.
  (eprintln "scenario: a light worker hydrates the image and lacks the stdlib")
  (let [r (start (warm "light")
                 ["--trace=boot" "-e"
                  "(println (get (protect (sys/join (sys/spawn-vm (fn [] (eval '(+ 1 2)))))) 1))"])]
    (assert (string/contains? r:out "undefined variable: +")
            (string "the light worker resolved a stdlib name: " r:out))
    (assert (= (marks r "image-hydrate") 2)
            (string "the start and its light worker each hydrate once: " r:err)))

  # The inner join runs under `ev/run`, because a worker has no scheduler of
  # its own (docs/threads.md).
  (eprintln "scenario: a worker that a worker spawns hydrates the image too")
  (let [r (start (warm "nested")
                 ["--trace=boot" "-e"
                  "(println (sys/join (sys/spawn (fn [] (ev/run (fn [] (sys/join (sys/spawn (fn [] 7)))))))))"])]
    (assert (= (string/trim r:out) "7") (string "printed " r:out))
    (assert (= (marks r "image-hydrate") 3)
            (string "the start, its worker and the worker's worker each "
                    "hydrate once: " r:err)))

  (eprintln "scenario: a cold start's worker hydrates the image the start stored")
  (let [r (start "cold" ["--trace=boot" "-e" "(sys/join (sys/spawn (fn [] 1)))"])]
    (assert (> (marks r "image-store") 0) "the cold start stored no image")
    (assert (= (marks r "image-hydrate") 1)
            (string "the worker hydrates the image its parent stored: " r:err)))

  # A worker never reads the process policy itself, so a `--no-stdlib` start,
  # which holds no image, has a worker that compiles. The start has no
  # `sys/join`, which is the stdlib's, so it polls the primitive the join is
  # built on.
  (eprintln "scenario: a no-stdlib start's worker reads no image")
  (let [r (start (warm "bare")
                 ["--no-stdlib" "--trace=boot" "-e"
                  (string "(def h (sys/spawn (fn [] 1))) "
                          "(while (%not (%eq :ready (get (sys/thread-state h) 0))) nil)")])]
    (assert (= (marks r "image-hydrate") 0)
            (string "a worker of a start that holds no image hydrated one: "
                    r:err)))

  # The parent hydrates the file, then the test overwrites the file's header in
  # place. A hydration reads the header and never maps it, so the parent's own
  # mapping stands, and each hydration after it refuses the file. The
  # counter-factual compiles without trying the image: it answers too, and
  # reports no refusal.
  (eprintln "scenario: a worker whose inherited image refuses compiles and answers")
  (let [name (warm "refused")
        home (path/join dir name)
        ready (path/join home "ready")
        go (path/join home "go")
        child (subprocess/exec elle
                               [;(flags name) "-e"
                                (string "(file/write \"" ready "\" \"1\") "
                                        "(while (not (file/exists? \"" go
                                        "\")) (ev/sleep 0.01)) "
                                        "(println (sys/join (sys/spawn (fn [] (eval '(map inc [1 2 3]))))))")]
                               {:stdin :null})
        out (ev/spawn (fn [] (string (port/read-all (get child :stdout)))))
        err (ev/spawn (fn [] (string (port/read-all (get child :stderr)))))]
    # The program runs once its instance has booted, so `ready` existing means
    # the parent has hydrated the image.
    (def @waited 0)
    (while (and (not (file-exists? ready)) (< waited 12000))
      (ev/sleep 0.01)
      (assign waited (+ waited 1)))
    (assert (file-exists? ready) "the start never reached its program")
    (let [img (path/join home "img")
          image (first (filter (fn [n] (string/ends-with? n ".image"))
                               (file/ls img)))
          p (port/open-bytes (path/join img image) :read-write)]
      (port/write p (bytes 0 0 0 0 0 0 0 0))
      (port/close p))
    (file/write go "1")
    (let [code (subprocess/wait child)
          stdout (ev/join out)
          stderr (ev/join err)]
      (assert (= code 0) (string "the start failed: " stderr))
      (assert (= (string/trim stdout) "[2 3 4]")
              (string "the worker that refused its image answered " stdout))
      (assert (any? (fn [l] (string/starts-with? l "[boot-image]"))
                    (lines stderr))
              (string "the worker reported no refused image: " stderr)))))

(eprintln "all worker-boot scenarios passed")
