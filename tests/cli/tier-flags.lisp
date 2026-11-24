(elle/epoch 18)
## audited: 2026-11-24
## The tier a build starts, read back out of a running VM, and the tier flags a user build refuses.
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

# The default build: the JIT, and neither MLIR nor WebAssembly.
(when (and jit? (not mlir?) (not wasm?))
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

# The JIT compiles adaptively, and no program chooses another policy. Only the
# `elle test` process may turn a tier off or make it eager, another keyword is
# the wrong type, and a count below one names no threshold (docs/config.md).
(eprintln "scenario: a program cannot turn the JIT off")
(each [value want] [[":off" "false argument-error"]
                    [":eager" "false argument-error"]
                    [":later" "false type-error"] ["0" "false argument-error"]]
  (assert (= (reads (string "(let [[ok? err] (protect (vm/config-set :jit "
                            value "))] (println ok? \" \" (get err :error)))"))
             want) (string "a program outside elle test sets :jit " value)))

# A user build has no flag that chooses a tier, a backend or a pass. The
# counter-factual: a flag that stopped parsing would become the program's name,
# and the run would read a missing file called `--jit=off`.
(eprintln "scenario: a removed flag is refused by name")
(each flag ["--jit=off" "--jit=eager" "--mlir=off" "--anf=off" "--no-uring"
            "--stats" "--flip=on" ;(if wasm? [] ["--wasm=full"])]
  (let [r (elle-run [flag "-e" "(println :ran)"])]
    (assert (not (= r:exit 0)) (string "elle " flag " ran: " r:out))
    (assert (and (string/contains? r:err "unknown option")
                 (string/contains? r:err flag))
            (string "elle " flag " names the option it refused: " r:err))))

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
