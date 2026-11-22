(elle/epoch 18)
## audited: 2026-11-22
## A run recorded in another store joins local history once, with the code state it ran against and the bytes its assets name.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: with no merge a downloaded run is a database you point
## `--db` at, so it answers alone and never joins a local query. With no run
## key a second import of one artifact appends every run again, and the history
## reads as twice the runs rather than as one run seen twice.

(include-file "lib/harness.lisp")

(def root (path/cwd))

(defn rows-in [db table]
  (scalar db (string "SELECT count(*) AS c FROM " table)))

(defn census [db]
  "The whole store, as the row counts an import has to reproduce."
  (map (fn [t] [t (rows-in db t)])
       ["run" "form" "result" "asset" "measurement" "gauge" "changed_file"]))

(defn bytes-of [path]
  (let [p (port/open-bytes path :read)
        b (port/read p 4000000)]
    (port/close p)
    b))

(with-temp-dir dir
  (defn store [name]
    "A session DB with its CAS beside it, in a directory of its own, so two
     stores never share a CAS and a copied file is provably copied."
    (file/mkdir-all (path/join dir name))
    (path/join dir name "elle-tests.db"))

  # The printed text names the file. A form is identified by the hash of its
  # syntax, so two fixtures with one body would be one form in both stores,
  # and a merge that dropped the foreign row would still look right.
  (defn record [db name]
    "Record one run of a printing fixture into DB, from the repository, so the
     run row carries a commit, a worktree and a tree hash."
    (let [r (elle-test db
                       [(write-fixture dir name
                                       (string "(println \"printed by " name
                                         "\")\n"))] :cwd root)]
      (assert (= r:exit 0) (string "the recorded run gates green: " r:err))))

  (defn merge-into [db src]
    (elle-test db ["--import" src] :cwd root))

  (defn merge-ok [db src]
    (let [r (merge-into db src)]
      (assert (= r:exit 0) (string "the import should succeed: " r:err))
      r))

  (eprintln "scenario: an imported run joins local history")
  (let [far (store "joins-far")
        near (store "joins-near")]
    (record far "printer.lisp")
    (let [r (merge-ok near far)]
      (assert (string/contains? r:err "imported 1 run")
              (string "the import must say what it took: " r:err)))
    # A run whose commit, worktree, host and argv did not survive the merge
    # names no code and no machine, which is the reason the columns exist.
    (let [identity (string "SELECT git_commit, worktree, host, argv, tiers, "
                           "n_selected, pid, wall_ms, "
                           "(finished_at IS NOT NULL) AS done FROM run")]
      (assert (= (string (rows near identity)) (string (rows far identity)))
              "an imported run carries the identity the recorded run had"))
    (assert (= (census near) (census far))
            (string "the import brings every row: " (census near)))
    (assert (any? (fn [row] (string/contains? row:file "printer.lisp"))
                  (rows near
                        (string "SELECT f.file AS file FROM result r "
                                "JOIN form f ON f.hash = r.form_hash")))
            "an imported result joins to its form"))

  # Without the column, a test-heap sum lands as a runner-heap delta and breaks
  # the runner heap's chain in the store that received it.
  (eprintln "scenario: an imported gauge row keeps its heap")
  (let [far (store "heap-far")
        near (store "heap-near")
        by-heap "SELECT heap, count(*) AS n FROM gauge GROUP BY heap ORDER BY heap"]
    (record far "measured.lisp")
    (merge-ok near far)
    (assert (any? (fn [row] (= row:heap "test")) (rows far by-heap))
            "the recorded run wrote test-heap rows")
    (assert (= (string (rows near by-heap)) (string (rows far by-heap)))
            "the imported rows keep the heap each one measured"))

  (eprintln "scenario: importing the same store twice changes nothing")
  (let [far (store "twice-far")
        near (store "twice-near")]
    (record far "printer.lisp")
    (merge-ok near far)
    (let [once (census near)
          again (merge-ok near far)]
      (assert (string/contains? again:err "already present")
              (string "a repeated import says the run was already here: "
                      again:err))
      (assert (= (census near) once)
              "a run already imported is not appended a second time")))

  (eprintln "scenario: importing a store into itself changes nothing")
  (let [near (store "self")]
    (record near "printer.lisp")
    (let [before (census near)]
      (merge-ok near near)
      (assert (= (census near) before)
              "every run in the source is already in the destination")))

  (eprintln "scenario: an imported asset brings its bytes")
  (let [far (store "cas-far")
        near (store "cas-near")
        there (path/join dir "cas-far" "cas")
        here (path/join dir "cas-near" "cas")]
    (record far "printer.lisp")
    (assert (> (rows-in far "asset") 0)
            "the fixture prints, so the recorded run holds an asset")
    (merge-ok near far)
    (let [entries (file/ls there)]
      (assert (not (empty? entries)) "the source CAS held nothing")
      (each entry entries
        (assert (file-exists? (path/join here entry))
                (string "the import copies " entry " into the local CAS"))
        (assert (= (bytes-of (path/join here entry))
                   (bytes-of (path/join there entry)))
                (string "a CAS file is its content, so the copy of " entry
                        " is byte-identical")))))

  # An asset and a measurement name a result by its row id, and the ids of two
  # stores mean nothing to each other. A merge that kept the foreign ids would
  # point every imported asset at whichever local result held that number.
  (eprintln "scenario: a result id is remapped as the row lands")
  (let [far (store "remap-far")
        near (store "remap-near")]
    (record far "printer.lisp")
    # The local store's result ids are taken before the foreign rows arrive.
    (record near "local.lisp")
    # No run records a dashboard's verdict or a changed file yet, so both are
    # seeded, each hanging off a run the merge has to carry.
    (query! far
            (string "INSERT INTO measurement (run_id, result_id, subject, "
                    "axis, value, unit, verdict) SELECT run_id, id, "
                    "'a-probe', 'regions', 1.5, 'regions/op', 'closed' "
                    "FROM result ORDER BY id LIMIT 1"))
    (query! far
            (string "INSERT INTO changed_file (run_id, path, status, "
                    "blob_hash) SELECT id, 'src/vm.rs', 'M', 'cafe1234' "
                    "FROM run"))
    (merge-ok near far)
    (assert (any? (fn [row] (string/contains? row:file "printer.lisp"))
                  (rows near
                        (string "SELECT f.file AS file FROM measurement m "
                                "JOIN result r ON r.id = m.result_id "
                                "JOIN form f ON f.hash = r.form_hash "
                                "WHERE m.subject = 'a-probe'")))
            "the imported measurement points at the imported result")
    (assert (= (scalar near
                       (string "SELECT count(*) AS c FROM asset a "
                               "JOIN result r ON r.id = a.result_id"))
               (rows-in near "asset"))
            "every imported asset points at a result that exists")
    (assert (any? (fn [row] (= row:path "src/vm.rs"))
                  (rows near
                        (string "SELECT cf.path AS path FROM changed_file cf "
                                "JOIN run r ON r.id = cf.run_id")))
            "a changed file follows its run"))

  # A run recorded before the key existed carries none.
  (eprintln "scenario: a run with no key imports once")
  (let [far (store "keyless-far")
        near (store "keyless-near")]
    (record far "printer.lisp")
    (query! far "UPDATE run SET run_key = NULL")
    (merge-ok near far)
    (merge-ok near far)
    (assert (= (rows-in near "run") 1)
            "a keyless run is identified by the row it is, so it imports once"))

  # Answering zero would report success having merged nothing.
  (eprintln "scenario: an import of a store that is not there fails")
  (let [near (store "missing-near")
        absent (path/join dir "nowhere" "elle-tests.db")
        r (merge-into near absent)]
    (assert (not (= r:exit 0))
            "an import of a missing store must not report success")
    (assert (string/contains? r:err absent)
            (string "the message names the path it could not read: " r:err))
    (assert (or (not (file-exists? near)) (= (rows-in near "run") 0))
            "a failed import leaves no run behind"))

  (eprintln "all import scenarios passed"))
