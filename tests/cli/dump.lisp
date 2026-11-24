(elle/epoch 18)
## audited: 2026-11-24
## The `--dump=STAGE[,STAGE,...]` surface: each stage prints its banner and the markers its artifact is read by.
## tests/cli/overview.md
## docs/config.md

(include-file "../runner/lib/harness.lisp")

(defn dump [stages source &opt flags]
  "Run SOURCE under `--dump=STAGES` and FLAGS, and answer the run."
  (elle-run [;(or flags []) (string "--dump=" stages) "-e" source]))

(defn shows [stages source markers]
  "Require `--dump=STAGES` of SOURCE to exit 0 and print each of MARKERS."
  (let [r (dump stages source)]
    (assert (= r:exit 0) (string "--dump=" stages " failed: " r:err))
    (each marker markers
      (assert (string/contains? r:out marker)
              (string "--dump=" stages " lacks " marker ": " r:out)))))

(eprintln "scenario: each stage prints its banner and its markers")
(shows "ast" "(+ 1 2)" ["── ast" "(+ 1 2)"])
(shows "hir" "(defn sq [x] (* x x)) (sq 3)" ["── hir" "entry" "closure[0]"])
# The arrow is the LIR printer's register assignment.
(shows "lir" "(+ 1 2)" ["── lir" "block0:" "←"])
(shows "cfg" "(if true 1 2)" ["── cfg" "→"])
(shows "dfa" "(defn f [x] x)" ["── dfa" "signal=" "capture_params_mask="])
(shows "jit" "(+ 1 2)" ["── jit" "eligible="])
# `git` is the SPIR-V stage, which prints a block per closure whether or not
# the build carries MLIR.
(shows "git" "(+ 1 2)" ["── git" "; entry"])
# The binding source regions say which regions a binding's value may live in,
# which the scope-region section does not answer.
(shows "regions" "(let [x 1] (+ x 2))"
       ["── regions" "region assignments" "binding source regions"
        "region inference stats"])

# AST, HIR, LIR, CFG, DFA, JIT, then git, a codegen stage like the JIT.
(eprintln "scenario: all stages run in pipeline order")
(let [r (dump "all" "(defn f [x] x)")]
  (assert (= r:exit 0) (string "--dump=all failed: " r:err))
  (def @last 0)
  (each stage ["ast" "hir" "lir" "cfg" "dfa" "jit" "git"]
    (let [i (string/find r:out (string "── " stage))]
      (assert (not (nil? i)) (string "--dump=all lacks " stage))
      (assert (>= i last) (string stage " printed out of pipeline order"))
      (assign last i))))

# The bytecode golden compares a recorded dump with a fresh one, so one file
# prints one text whatever the run. The stdlib cache and the boot image are
# off, so each run compiles the standard library and mints its static region
# slots before the file's own. The counter-factual iterates the emitter's
# location map or prints a constant's address, and two runs disagree.
(eprintln "scenario: the bytecode dump prints one text across two runs")
(let [src (string "(defn sq [x] (* x x))\n(def words '(alpha \"beta\" :gamma))\n"
                  "(defn adder [n] (fn [y] (+ y n (sq 2))))\n"
                  "(defn gen [] (fn [] (yield 1) (yield 2)))\n((adder 3) 4)")
      flags ["--cache=" "--boot-image=off"]
      run1 (dump "bytecode" src flags)
      run2 (dump "bytecode" src flags)]
  (assert (= run1:exit 0) (string "--dump=bytecode failed: " run1:err))
  (each marker ["── bytecode" "code object [0]" "constants:"]
    (assert (string/contains? run1:out marker)
            (string "the bytecode dump lacks " marker)))
  (assert (= run1:out run2:out) "two runs printed different bytecode dumps"))

(defn integers [line]
  "The integers among LINE's words, in order."
  (let [@out @[]
        bare (fold (fn [s c] (string/replace s c " ")) line ["(" ")" "," ":"])]
    (each w in (string/split bare " ")
      (let [[ok? n] (protect (parse-int w))]
        (when ok? (push out n))))
    out))

# The program forces one compile, so a zero is a count nobody kept. Only a
# build that carries the JIT prints the JIT's lines.
(when jit?
  (eprintln "scenario: stats counts the JIT's compiles and their cost")
  (let [r (dump "stats" "(compile/run-on :jit (fn [] 1))")
        hit (filter (fn [l] (string/starts-with? l "  compiles: "))
                    (lines r:err))]
    (assert (= r:exit 0) (string "--dump=stats failed: " r:err))
    (assert (= (length hit) 1) (string "stats prints one compiles line: " r:err))
    (let [[n wall cpu] (integers (first hit))]
      (assert (>= n 1) (string "the forced compile counts: " (first hit)))
      (assert (> wall 0) (string "the compiles took wall time: " (first hit)))
      (assert (> cpu 0) (string "the compiles took CPU time: " (first hit))))))

# The report drops the compiles still queued, and the one in progress, rather
# than wait for them (docs/config.md). Four hundred closures, each called past the threshold,
# queue far faster than the thread compiles them, so some are queued at the
# exit. The counter-factual is a drain: it prints no dropped line, and waits
# for every compile.
(when jit?
  (eprintln "scenario: stats drops the queued compiles and counts them")
  (let [fns (string/join (map (fn [i] (string "(fn [x] (+ x " i "))"))
                              (range 400)) " ")
        r (dump "stats"
                (string "(def fs [" fns "]) (each f in fs (repeat 11 (f 1)))"))
        line (fn [prefix]
               (filter (fn [l] (string/starts-with? l prefix)) (lines r:err)))
        dropped (line "  dropped: ")
        compiles (line "  compiles: ")]
    (assert (= r:exit 0) (string "--dump=stats failed: " r:err))
    (assert (= (length dropped) 1)
            (string "stats prints one dropped line: " r:err))
    (let [[d] (integers (first dropped))
          [n] (integers (first compiles))]
      (assert (> d 0) (string "the exit found no compile queued: " r:err))
      (assert (>= (+ n d) 400)
              (string "each closure's compile is taken or dropped: " r:err)))))

(eprintln "scenario: an unknown stage is refused")
(let [r (dump "bogus" "(+ 1 2)")]
  (assert (not (= r:exit 0)) "--dump=bogus ran")
  (assert (and (string/contains? r:err "--dump: unknown stage 'bogus'")
               (string/contains? r:err
                                 "Valid: ast, hir, fhir, lir, jit, cfg, dfa, defuse, regions, escape, git, bytecode"))
          (string "the refusal lists the valid stages: " r:err)))

(eprintln "all dump scenarios passed")
