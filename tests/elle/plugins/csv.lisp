(elle/epoch 8)

## CSV plugin integration tests

## Try to load the csv plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_csv.so")))
(when (not ok?)
  (print "SKIP: csv plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn      (get plugin :parse))
(def parse-rows-fn (get plugin :parse-rows))
(def write-fn      (get plugin :write))
(def write-rows-fn (get plugin :write-rows))

## ── csv/parse ───────────────────────────────────────────────────

(def result (parse-fn "name,age\nAlice,30\nBob,25"))

(assert (= (length result) 2) "csv/parse row count")

(assert (= (get (get result 0) :name) "Alice") "csv/parse first name")

(assert (= (get (get result 0) :age) "30") "csv/parse age is string")

(assert (= (get (get result 1) :name) "Bob") "csv/parse second name")

## ── csv/parse-rows ──────────────────────────────────────────────

(def raw (parse-rows-fn "a,b,c\n1,2,3"))

(assert (= (length raw) 2) "csv/parse-rows row count")

(assert (= (get (get raw 0) 0) "a") "csv/parse-rows first field")

(assert (= (get (get raw 1) 1) "2") "csv/parse-rows second row second field")

## ── csv/write ───────────────────────────────────────────────────

(def written (write-fn [{:age "30" :name "Alice"} {:age "25" :name "Bob"}]))

(assert (string? written) "csv/write returns string")

## Written CSV should contain the data
(assert (> (length written) 0) "csv/write non-empty output")

## ── csv/write-rows ──────────────────────────────────────────────

(def rows-text (write-rows-fn [["a" "b"] ["1" "2"]]))

(assert (string? rows-text) "csv/write-rows returns string")

(assert (> (length rows-text) 0) "csv/write-rows non-empty output")

## ── custom delimiter (tab) ──────────────────────────────────────

(def tsv (parse-fn "name\tage\nAlice\t30" {:delimiter "\t"}))

(assert (= (length tsv) 1) "csv/parse tab delimiter row count")

(assert (= (get (get tsv 0) :name) "Alice") "csv/parse tab delimiter name")

(assert (= (get (get tsv 0) :age) "30") "csv/parse tab delimiter age")

## ── tab delimiter write-rows ────────────────────────────────────

(def tsv-out (write-rows-fn [["x" "y"] ["1" "2"]] {:delimiter "\t"}))

(assert (string? tsv-out) "csv/write-rows tab delimiter returns string")

## ── empty input ─────────────────────────────────────────────────

(def empty-result (parse-fn "name,age"))

(assert (= (length empty-result) 0) "csv/parse headers-only = empty result")

## ── error cases ─────────────────────────────────────────────────

(let [[ok? _] (protect ((fn () (parse-fn 42))))] (assert (not ok?) "csv/parse wrong type"))

(let [[ok? _] (protect ((fn () (write-fn 42))))] (assert (not ok?) "csv/write non-array"))
