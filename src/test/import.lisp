(elle/epoch 12)
# audited: 2026-09-22
## elle test — merging another store's runs into this one: the key that makes
## an import repeatable, the rows that follow a run, and the bytes copied by
## address.
## docs/test-store.md
##
## A fragment of one module (see store.lisp).

# ── the columns each table is copied by ──────────────────────────────
# One list per table, in the order the INSERT binds them, and each starting
# with the column the import supplies itself: a run's key, and the run or
# result id the row is remapped onto. Written as data because five tables are
# copied the same way, and a hand-written INSERT per table is five chances to
# bind a value under the wrong name.
(def run-columns
  ["run_key" "started_at" "finished_at" "tiers" "selection" "n_selected"
   "git_commit" "git_dirty" "tree_hash" "worktree" "boot_fingerprint"
   "elle_version" "build_profile" "host" "argv" "n_pass" "n_fail" "n_skip"
   "n_diverge" "n_timeout"])

(def form-columns
  ["hash" "origin" "session" "file" "form_index" "line" "col" "label" "src"
   "caps" "touches" "signal"])

(def result-columns
  ["run_id" "form_hash" "tier" "status" "reason" "expected" "actual" "syntax"
   "signal" "wall_ms" "cpu_us"])

(def asset-columns ["result_id" "kind" "hash" "size" "codec"])

(def measurement-columns
  ["run_id" "result_id" "subject" "axis" "value" "unit" "verdict"])

(def gauge-columns ["run_id" "file" "kind" "delta" "reading"])

(def changed-columns ["run_id" "path" "status" "blob_hash"])

(defn insert-sql [verb table cols]
  "An INSERT over COLS, binding ?1..?N in their order."
  (string verb " INTO " table " (" (string/join cols ", ") ") VALUES ("
          (string/join (map (fn [i] (string "?" (+ i 1)))
                            (->list (range (length cols)))) ", ") ")"))

(defn row-values [row cols]
  "One foreign row's values, in column order. A column the source store does
   not have reads nil, which lands as SQL NULL — the honest answer for a fact
   the run never recorded."
  (map (fn [c] (get row (keyword c))) cols))

(defn missing-table? [e]
  (string/contains? (string (get e :message)) "no such table"))

(defn source-rows [src sql params]
  "Rows from the foreign store. A table its schema does not carry holds no
   rows to merge, so that one error answers the empty list; every other error
   is a store this import cannot read, and it propagates."
  (let [[ok? rows] (protect (sqlite:query src sql params))]
    (if ok? rows (if (missing-table? rows) [] (error rows)))))

# ── which run a foreign row is ───────────────────────────────────────
# A run's id is a row number, and every store mints the same numbers, so the
# id cannot say which run a row is. The key can: the runner mints one per run
# (docs/test-store.md § The run key) and it travels with the row.
#
# A run recorded before the key existed carries none. Deriving one from the row
# itself keeps a repeat import of that store a no-op, which is the property the
# key is here for.
(defn source-run-key [r]
  (or (get r :run_key)
      (string "keyless:"
              (hash (string (get r :id) "|" (get r :started_at) "|"
                            (get r :host) "|" (get r :worktree) "|"
                            (get r :argv))))))

# ── what one import did ──────────────────────────────────────────────
(defn new-tally []
  @{:runs 0 :results 0 :assets 0 :cas 0 :absent 0 :present 0})

(defn bump [tally key n]
  (put tally key (+ (get tally key) n))
  nil)

(defn plural [n what]
  (string n " " what (if (= n 1) "" "s")))

# ── the CAS ──────────────────────────────────────────────────────────
# An address is the content, so a file this store already holds is that file
# and the copy is skipped. Bytes the foreign CAS does not carry leave the asset
# row pointing at an address that reads as soon as the bytes arrive.
#
# A copy is not part of the run's transaction. A run that fails to land can
# therefore leave its bytes behind, which costs disk and nothing else: an
# address nothing references is never read.
(defn copy-cas [far-cas addr tally]
  (let [here (string cas-dir "/" addr)
        there (string far-cas "/" addr)]
    (if (file-exists? here)
      nil
      (if (file-exists? there)
        (begin
          (file/copy there here)
          (bump tally :cas 1))
        (bump tally :absent 1)))))

# ── one run's rows ───────────────────────────────────────────────────
(defn insert-run [conn r]
  "Append one foreign run under its key and answer the local id it landed at,
   or nil when this store already holds that run."
  (let [n (sqlite:exec conn (insert-sql "INSERT OR IGNORE" "run" run-columns)
                       (concat [(source-run-key r)]
                               (row-values r (rest run-columns))))]
    (if (= n 0) nil (last-rowid conn))))

(defn import-results [conn src far-cas far-id run-id tally]
  "One run's results, and the assets and measurements that name them. A result
   id belongs to the store that minted it, so each row is inserted fresh and
   the id it lands at is what its assets and measurements are written under."
  (let [@ids @{}]
    (each r in (source-rows src "SELECT * FROM result WHERE run_id = ?1"
                            [far-id])
      (sqlite:exec conn (insert-sql "INSERT" "result" result-columns)
                   (concat [run-id] (row-values r (rest result-columns))))
      (put ids (get r :id) (last-rowid conn))
      (bump tally :results 1))
    (each a in (source-rows src
                            (string "SELECT a.* FROM asset a "
                                    "JOIN result r ON r.id = a.result_id "
                                    "WHERE r.run_id = ?1") [far-id])
      (copy-cas far-cas (get a :hash) tally)
      (sqlite:exec conn (insert-sql "INSERT" "asset" asset-columns)
                   (concat [(get ids (get a :result_id))]
                           (row-values a (rest asset-columns))))
      (bump tally :assets 1))
    (each m in (source-rows src "SELECT * FROM measurement WHERE run_id = ?1"
                            [far-id])
      (sqlite:exec conn (insert-sql "INSERT" "measurement" measurement-columns)
                   (concat [run-id (get ids (get m :result_id))]
                           (row-values m (rest (rest measurement-columns))))))
    nil))

(defn import-rows [conn src table cols far-id run-id]
  "One run's rows from a table the run id alone keys."
  (each row in (source-rows src
                            (string "SELECT * FROM " table " WHERE run_id = ?1")
                            [far-id])
    (sqlite:exec conn (insert-sql "INSERT" table cols)
                 (concat [run-id] (row-values row (rest cols)))))
  nil)

# One run is one transaction. A run already here is the commit of nothing,
# which keeps the two paths one shape. The transaction is what makes the key
# safe: an import that dies partway would otherwise leave the run row it wrote
# first, and every later import would read that run as already merged.
(defn import-run [conn src far-cas r tally]
  (sqlite:exec conn "BEGIN")
  (let [run-id (insert-run conn r)]
    (if (= run-id nil)
      (begin
        (sqlite:exec conn "COMMIT")
        (bump tally :present 1))
      (begin
        (import-results conn src far-cas (get r :id) run-id tally)
        (import-rows conn src "gauge" gauge-columns (get r :id) run-id)
        (import-rows conn src "changed_file" changed-columns (get r :id) run-id)
        (sqlite:exec conn "COMMIT")
        (bump tally :runs 1))))
  nil)

(defn import-forms [conn src]
  "The foreign corpus index. A form is the hash of its syntax, so a form both
   stores hold is one row and IGNORE is the whole merge."
  (each f in (source-rows src "SELECT * FROM form" [])
    (sqlite:exec conn (insert-sql "INSERT OR IGNORE" "form" form-columns)
                 (row-values f form-columns)))
  nil)

(defn import-report [path tally]
  (eprintln "imported " (plural (get tally :runs) "run") " · "
            (plural (get tally :results) "result") " · "
            (plural (get tally :assets) "asset") " · "
            (plural (get tally :cas) "CAS file")
            (if (> (get tally :present) 0)
              (string " · " (get tally :present) " already present")
              "")
            (if (> (get tally :absent) 0)
              (string " · " (get tally :absent) " with no bytes")
              "") " from " path))

# PATH names the foreign session DB, and its CAS is the `cas` directory beside
# it — the layout --db makes. The import records no run of its own: it is a
# store operation, and a run row for it would say that tests ran here.
(defn do-import [conn path]
  (when (not (file-exists? path))
    (eprintln "elle test: --import found no store at " path)
    (os/exit 2))
  (let [src (sqlite:open path)
        far-cas (string (path/parent path) "/cas")
        tally (new-tally)]
    (import-forms conn src)
    (each r in (source-rows src "SELECT * FROM run ORDER BY id" [])
      (import-run conn src far-cas r tally))
    (sqlite:close src)
    (import-report path tally))
  nil)
