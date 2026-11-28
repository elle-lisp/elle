(elle/epoch 19)
## audited: 2026-11-27
## The tier a build starts, the JIT policy `--jit=` sets, each read back out of a running VM, and the flags a build refuses.
## tests/cli/overview.md
## docs/config.md
##
## src/config/tests.rs pins what `Config::parse` builds from the argv. This file
## pins what the binary does: `main` resolves the config, installs it, and each
## VM seeds its runtime configuration from there. Whether the dispatcher
## compiles at that threshold needs a call that stays a call, which only the rig
## can promise, so tests/impl/jit-threshold.lisp holds that half.

(include-file "../runner/lib/harness.lisp")

(defn reads [source]
  "Run SOURCE, and answer its trimmed stdout once it exits 0."
  (let [r (elle-run ["-e" source])]
    (assert (= r:exit 0) (string "elle -e " source " exited " r:exit ": " r:err))
    (string/trim r:out)))

(defn carries? [tier]
  "Whether the binary under test carries TIER, probed as the runner probes."
  (= "true"
     (reads (string "(let [[ok? err] (protect (compile/run-on " tier
                    " (fn [] 1)))] "
                    "(print (or ok? (not (= (get err :reason) :feature-disabled)))))"))))

(def jit? (carries? ":jit"))
(def mlir? (carries? ":mlir-cpu"))
(def wasm? (carries? ":wasm"))
(def jit-tier? (and jit? (not mlir?) (not wasm?)))

# The default build: the JIT, and neither MLIR nor WebAssembly.
(when jit-tier?
  (eprintln "scenario: the default build reports the JIT threshold")
  (assert (= (reads "(println (vm/config :jit))") "10")
          "the default threshold reads 10")
  # The threshold is the one JIT setting a program owns.
  (eprintln "scenario: a program reads back the threshold it sets")
  (assert (= (reads "(vm/config-set :jit 20) (println (vm/config :jit))") "20")
          "the threshold a program sets reads back"))

(when (not (and mlir? (not wasm?)))
  (eprintln "scenario: a build without MLIR reports no MLIR threshold")
  (assert (= (reads "(println (vm/config :mlir))") "nil")
          "a build without MLIR reads nil"))

# At run time no program chooses another policy. Only the `elle test` process
# may turn a tier off or make it eager, another keyword is the wrong type, and a
# count below one names no threshold (docs/config.md).
(eprintln "scenario: a program cannot turn the JIT off")
(each [value want] [[":off" "false argument-error"]
                    [":eager" "false argument-error"]
                    [":later" "false type-error"] ["0" "false argument-error"]]
  (assert (= (reads (string "(let [[ok? err] (protect (vm/config-set :jit "
                            value "))] (println ok? \" \" (get err :error)))"))
             want) (string "a program outside elle test sets :jit " value)))

# A user build has no flag that chooses a tier, a backend or a pass, and only a
# build whose tier is the JIT reads `--jit=`. The counter-factual: a flag that
# stopped parsing would become the program's name, and the run would read a
# missing file called `--mlir=off`.
(eprintln "scenario: a removed flag is refused by name")
(each flag ["--mlir=off" "--anf=off" "--no-uring" "--stats" "--flip=on"
            ;(if wasm? [] ["--wasm=full"])
            ;(if jit-tier? [] ["--jit=off" "--jit=0"])]
  (let [r (elle-run [flag "-e" "(println :ran)"])]
    (assert (not (= r:exit 0)) (string "elle " flag " ran: " r:out))
    (assert (and (string/contains? r:err "unknown option")
                 (string/contains? r:err flag))
            (string "elle " flag " names the option it refused: " r:err))))

(defn compiles [flag]
  "Run a program under FLAG with the JIT compiling on the VM thread, and answer
   how many compiles its statistics count. A synchronous compile finishes before
   the program ends, so the count does not race the background worker."
  (let [r (elle-run [flag "--trace=syncjit" "--dump=stats" "-e" "(+ 1 2)"])
        marker "compiles: "
        at (string/find r:err marker)]
    (assert (= r:exit 0) (string "elle " flag " exited " r:exit ": " r:err))
    (assert (not (nil? at))
            (string "elle " flag " printed no compile count: " r:err))
    (parse-int (first (string/split (slice r:err (+ at (length marker))) " ")))))

# The counter-factual for the readings: a flag that set the configuration the
# VM never reads. The compile counts show the policy reaches the dispatcher.
(when jit-tier?
  (eprintln "scenario: --jit= sets the policy a running VM reads")
  (each [flag want] [["--jit=off" "nil"] ["--jit=0" "0"] ["--jit=3" "3"]
                     ["--jit=10" "10"]]
    (let [r (elle-run [flag "-e" "(println (vm/config :jit))"])]
      (assert (= r:exit 0) (string "elle " flag " exited " r:exit ": " r:err))
      (assert (= (string/trim r:out) want)
              (string "elle " flag " reads " want ", got " r:out))))
  (eprintln "scenario: --jit=off and a high threshold compile nothing, eager does")
  (assert (= (compiles "--jit=off") 0) "--jit=off compiles nothing")
  (assert (= (compiles "--jit=100000") 0)
          "no function is called 100000 times, so nothing compiles")
  (assert (> (compiles "--jit=0") 0) "--jit=0 compiles at the first call")
  (eprintln "scenario: --jit= refuses a value that is no policy, by name")
  (each value ["eager" "adaptive" "" "-1" "ten" "OFF"]
    (let [flag (string "--jit=" value)
          r (elle-run [flag "-e" "(println :ran)"])]
      (assert (not (= r:exit 0)) (string "elle " flag " ran: " r:out))
      (assert (and (string/contains? r:err "--jit")
                   (string/contains? r:err (string "'" value "'")))
              (string "elle " flag " names the flag and the value: " r:err)))))

(eprintln "scenario: --dump=stats runs the program and reports at its end")
(let [r (elle-run ["--dump=stats" "-e" "(println :ran)"])]
  (assert (= r:exit 0) (string "stderr: " r:err))
  (assert (= (string/trim r:out) "ran") "the program ran")
  (assert (string/contains? r:err "[stats] live regions after teardown")
          (string "--dump=stats prints the teardown census: " r:err)))

# The runner starts every isolated child with --dump=stats, so
# tests/impl/config.lisp cannot pin the off value and leaves it here.
(eprintln "scenario: vm/config :stats reads whether --dump=stats asked")
(assert (= (reads "(println (vm/config :stats))") "false")
        "statistics are off unless --dump=stats asks")
(let [r (elle-run ["--dump=stats" "-e" "(println (vm/config :stats))"])]
  (assert (= r:exit 0) (string "stderr: " r:err))
  (assert (= (string/trim r:out) "true")
          (string "--dump=stats turns statistics on: " r:out)))

(eprintln "all tier-flags scenarios passed")
