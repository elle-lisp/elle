(elle/epoch 18)
## audited: 2026-11-25
## The source boot's gate: the runner's flag, and the target that proves the boot, checks parity, then runs the suite.
## tests/runner/overview.md
## docs/impl/image/boot.md
## docs/analysis/ci.md
##
## Every part fails quietly. `elle test` partitions its argv, and a flag it does
## not know is handed to the runner as a file path. The gate target is shell
## text nothing compiles, so a target that drops the flag runs the language
## suite from the blob and reports the green a source boot reports.
## tests/cli/boot-parity.lisp pins the parity check itself.

(include-file "lib/harness.lisp")
(include-file "lib/make.lisp")

(def gate "smoke-source-boot")

(defn boot-marks [err]
  "The `[trace:boot]` lines of ERR."
  (filter (fn [l] (string/starts-with? l "[trace:boot]")) (lines err)))

(defn hydrated? [marks]
  "Whether MARKS show an instance that mapped or hydrated an image."
  (any? (fn [l]
          (or (string/contains? l "image-hydrate")
              (string/contains? l "blob-map"))) marks))

# The counter-factual: a runner that drops the flag boots its instance and
# every worker from the blob, and each file passes as it does under
# `smoke-lang`.
(eprintln "scenario: the runner boots from source under --boot-image=off")
(with-temp-dir dir
  (let [file (write-fixture dir "arithmetic.lisp"
                            "(assert (= (+ 1 2) 3) \"the runner ran a file\")\n")
        r (elle-test (path/join dir "session.db")
                     ["--boot-image=off" "--trace=boot" file])
        marks (boot-marks r:err)]
    (assert (= r:exit 0)
            (string "`elle test --boot-image=off` did not pass its file: " r:err))
    (assert (any? (fn [l] (string/starts-with? l "[trace:boot] prelude ")) marks)
            (string "`elle test --boot-image=off` compiled no prelude: " r:err))
    (assert (not (hydrated? marks))
            (string "`elle test --boot-image=off` booted from an image: " r:err))))

(def recipe (make-dry-run gate))
(def runner (string (make-var "ELLE") " test"))
(def batches
  (filter (fn [l] (string/contains? l runner)) (string/split recipe "\n")))

(defn first-word [text]
  "The text before the first blank, tab or newline of TEXT."
  (first (string/split (string/replace (string/replace text "\t" " ") "\n" " ")
                       " ")))

# A batch without the flag boots from the blob, and a recipe that names an
# image directory anywhere reads an image somewhere.
(eprintln "scenario: every start the gate makes boots from source")
(assert (not (empty? batches))
        (string "`make " gate "` never runs `" runner "`: " recipe))
(each line batches
  (assert (string/contains? line " --boot-image=off ")
          (string "a batch in `make " gate "` boots from an image: " line)))
(let [values (map first-word (rest (string/split recipe "--boot-image=")))]
  (assert (all? (fn [v] (= v "off")) values)
          (string "`make " gate "` names a boot image other than off: " values)))

# A binary that ignores `--boot-image=off` accepts it and hydrates its blob,
# and the suite passes. So the target reads the boot marks of a start and
# fails on a hydration, before the suite runs.
(eprintln "scenario: the gate proves the source boot before the suite runs")
(let [proof (string/find recipe "--trace=boot")
      suite (string/find recipe runner)]
  (assert (not (nil? proof))
          (string "`make " gate "` runs no `--trace=boot` start: " recipe))
  (assert (< proof suite)
          (string "`make " gate "` runs the suite before its proof: " recipe))
  (each mark ["image-hydrate" "blob-map" "prelude"]
    (assert (string/contains? recipe mark)
            (string "`make " gate "` never reads the `" mark "` mark: " recipe))))

# A corpus that answers alike under a source boot can still have compiled to
# other bytecode, and only the dumps show that.
(eprintln "scenario: the gate compares bytecode before the suite runs")
(let [parity (string/find recipe "scripts/bytecode-golden parity")
      suite (string/find recipe runner)]
  (assert (not (nil? parity))
          (string "`make " gate "` never runs the parity check: " recipe))
  (assert (< parity suite)
          (string "`make " gate "` runs the suite before the parity check: "
                  recipe)))

(eprintln "all source-boot scenarios passed")
