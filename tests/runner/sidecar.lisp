(elle/epoch 18)
## audited: 2026-11-24
## Acceptance test for `elle-rig test`: a file with a sidecar is refused in-process and passes under --isolate.
## tests/runner/overview.md
## docs/test-runner.md
##
## A worker shares the runner's process and cannot run under a sidecar, so the
## rig's runner records the file as one file-level failure that names the
## sidecar and --isolate, and runs none of it. Under --isolate the child reads
## the sidecar, and the same file passes. The counter-factual: a runner that
## ignored the sidecar records a pass. The rig is the binary running this file
## under the suite; under any other, the file gates.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (eprintln "scenario: a sidecar file in-process on the rig")
  (let [rig (elle/executable)
        [rig? _] (protect (eval (quote (elle/build))))]
    (gate! rig? "the sidecar scenario needs the rig; elle test reads no sidecar"
      (let [db (path/join dir "rt-sidecar.db")
            r (subprocess/system rig
                                 @["test" "--db" db (fixture "sidecar.lisp")])
            res (select-results db "sidecar.lisp"
                                "result.status AS status, result.reason AS reason")]
        (assert (not (= r:exit 0))
                (string "sidecar: a refused file gates nonzero, got " r:exit))
        (assert (= (length res) 1)
                (string "sidecar: one file-level row, got " (length res)))
        (let [row (get res 0)]
          (assert (= row:status "fail")
                  (string "sidecar: expected fail, got " row:status))
          (assert (string/contains? (string row:reason) "sidecar.toml")
                  (string "sidecar: the reason names the sidecar: " row:reason))
          (assert (string/contains? (string row:reason) "--isolate")
                  (string "sidecar: the reason names --isolate: " row:reason)))
        # The isolated run starts in the scratch directory, which holds no
        # ledger. From the repository root, the child's residue reading would
        # owe the fixture a row of tests/ledger, and gate as unledgered.
        (each name in ["sidecar.lisp" "sidecar.toml"]
          (write-fixture dir name (slurp (fixture name))))
        (let [idb (path/join dir "rt-sidecar-isolated.db")
              i (run-binary rig
                            ["test" "--db" idb "--isolate" "" "sidecar.lisp"]
                            :cwd dir)]
          (assert (= i:exit 0)
                  (string "sidecar: under --isolate the file passes; exit "
                          i:exit " — stderr: " i:err))))))

  (eprintln "the sidecar scenario passed"))
