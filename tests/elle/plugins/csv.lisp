(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## CSV plugin integration tests

## Try to load the csv plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_csv.so")))
(when (not ok?)
  (display "SKIP: csv plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn      (get plugin :parse))
(def parse-rows-fn (get plugin :parse-rows))
(def write-fn      (get plugin :write))
(def write-rows-fn (get plugin :write-rows))

## ── csv/parse ───────────────────────────────────────────────────

(def result (parse-fn "name,age\nAlice,30\nBob,25"))

(assert-eq
  (length result)
  2
  "csv/parse row count")

(assert-eq
  (get (get result 0) :name)
  "Alice"
  "csv/parse first name")

(assert-eq
  (get (get result 0) :age)
  "30"
  "csv/parse age is string")

(assert-eq
  (get (get result 1) :name)
  "Bob"
  "csv/parse second name")

## ── csv/parse-rows ──────────────────────────────────────────────

(def raw (parse-rows-fn "a,b,c\n1,2,3"))

(assert-eq
  (length raw)
  2
  "csv/parse-rows row count")

(assert-eq
  (get (get raw 0) 0)
  "a"
  "csv/parse-rows first field")

(assert-eq
  (get (get raw 1) 1)
  "2"
  "csv/parse-rows second row second field")

## ── csv/write ───────────────────────────────────────────────────

(def written (write-fn [{:age "30" :name "Alice"} {:age "25" :name "Bob"}]))

(assert-true
  (string? written)
  "csv/write returns string")

## Written CSV should contain the data
(assert-true
  (> (length written) 0)
  "csv/write non-empty output")

## ── csv/write-rows ──────────────────────────────────────────────

(def rows-text (write-rows-fn [["a" "b"] ["1" "2"]]))

(assert-true
  (string? rows-text)
  "csv/write-rows returns string")

(assert-true
  (> (length rows-text) 0)
  "csv/write-rows non-empty output")

## ── custom delimiter (tab) ──────────────────────────────────────

(def tsv (parse-fn "name\tage\nAlice\t30" {:delimiter "\t"}))

(assert-eq
  (length tsv)
  1
  "csv/parse tab delimiter row count")

(assert-eq
  (get (get tsv 0) :name)
  "Alice"
  "csv/parse tab delimiter name")

(assert-eq
  (get (get tsv 0) :age)
  "30"
  "csv/parse tab delimiter age")

## ── tab delimiter write-rows ────────────────────────────────────

(def tsv-out (write-rows-fn [["x" "y"] ["1" "2"]] {:delimiter "\t"}))

(assert-true
  (string? tsv-out)
  "csv/write-rows tab delimiter returns string")

## ── empty input ─────────────────────────────────────────────────

(def empty-result (parse-fn "name,age"))

(assert-eq
  (length empty-result)
  0
  "csv/parse headers-only = empty result")

## ── error cases ─────────────────────────────────────────────────

(assert-err
  (fn () (parse-fn 42))
  "csv/parse wrong type")

(assert-err
  (fn () (write-fn 42))
  "csv/write non-array")
