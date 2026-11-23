(elle/epoch 18)
## audited: 2026-11-22
## The boot-image gate: the runner's flag, the target that runs the suite from an image, and when it checks bytecode parity.
## tests/runner/overview.md
## docs/impl/image/boot.md
## docs/analysis/ci.md
##
## Every part fails quietly. `elle test` partitions its argv, and a flag it does
## not know is handed to the runner as a file path. The gate target is shell
## text nothing compiles, so a target that drops the flag runs the language
## suite on a source boot and reports the green a hydrated run reports.
## tests/cli/boot-parity.lisp pins the parity check itself.

(include-file "lib/harness.lisp")
(include-file "lib/make.lisp")

(def gate "smoke-boot-image")

# The counter-factual is the whole gate: point the language suite at an image
# through a flag the runner swallows, and every batch boots from source under
# a target that says otherwise.
(eprintln "scenario: the runner boots from the image its flag names")
(with-temp-dir dir
  (let [flag (string "--boot-image=" (path/join dir "image"))
        file (write-fixture dir "arithmetic.lisp"
                            "(assert (= (+ 1 2) 3) \"the runner ran a file\")\n")
        # This start meets an empty directory, so it compiles the three sources
        # and stores what the run below has to hydrate.
        stored (elle-run [flag file])]
    (assert (= stored:exit 0) (string "the storing run failed: " stored:err))
    (let [r (elle-test (path/join dir "session.db") [flag "--trace=boot" file])]
      (assert (= r:exit 0)
              (string "`elle test " flag "` did not pass its file: " r:err))
      (assert (any? (fn [l]
                      (and (string/starts-with? l "[trace:boot]")
                           (string/contains? l "image-hydrate"))) (lines r:err))
              (string "`elle test " flag "` booted without hydrating the "
                      "image: " r:err)))))

(def recipe (make-dry-run gate))
(def runner (string (make-var "ELLE") " test"))
(def batches
  (filter (fn [l] (string/contains? l runner)) (string/split recipe "\n")))

# A gate that greens whatever it is given gates nothing. A recipe that names
# two directories fills one and reads the other, which is a source boot with
# extra steps. A value that closes a quoted argument ends before the quote.
(eprintln "scenario: the gate points the language suite at one image directory")
(defn first-word [text]
  "The text before the first blank, tab or newline of TEXT."
  (first (string/split (string/replace (string/replace text "\t" " ") "\n" " ")
                       " ")))

(defn unquoted [word]
  "WORD less the quotes that close it."
  (if (or (string/ends-with? word "'") (string/ends-with? word "\""))
    (unquoted (slice word 0 (- (length word) 1)))
    word))

(let [values (map (fn [after] (unquoted (first-word after)))
                  (rest (string/split recipe "--boot-image=")))]
  (assert (not (empty? values))
          (string "`make " gate "` names no `--boot-image=`: " recipe))
  (assert (all? (fn [v] (= v (first values))) values)
          (string "`make " gate "` names more than one image directory: " values))
  (assert (string/contains? (first values) "/")
          (string "`make " gate "` points the suite at " (first values)
                  " rather than at a directory of its own"))
  # Every batch goes through the runner, which hands each child its flags,
  # and the child is the process whose boot the flag decides.
  (assert (not (empty? batches))
          (string "`make " gate "` never runs `" runner "`: " recipe))
  (each line batches
    (assert (string/contains? line "--boot-image=")
            (string "a batch in `make " gate "` runs without the flag: " line))))

# A binary that ignores `--boot-image=` accepts it and boots from source, and
# an image every start refuses is replaced and refused again. Either way the
# suite passes, so the target asks for the boot mark and fails without it.
(eprintln "scenario: the gate proves the image hydrated before the suite runs")
(assert (string/contains? recipe "--trace=boot")
        (string "`make " gate "` runs no `--trace=boot` start: " recipe))
(assert (string/contains? recipe "image-hydrate")
        (string "`make " gate "` never reads the `image-hydrate` mark: " recipe))

# A corpus that answers alike under an image boot can still have compiled to
# other bytecode, and only the dumps show that.
(eprintln "scenario: the gate compares bytecode before the suite runs")
(let [parity (string/find recipe "scripts/bytecode-golden parity")
      suite (string/find recipe runner)]
  (assert (not (nil? parity))
          (string "`make " gate "` never runs the parity check: " recipe))
  (assert (< parity suite)
          (string "`make " gate "` runs the suite before the parity check: "
                  recipe)))

(eprintln "all boot-image scenarios passed")
