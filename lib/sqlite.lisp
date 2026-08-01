(elle/epoch 12)
## lib/sqlite.lisp — SQLite database access via FFI to libsqlite3
##
## Usage:
##   (def db ((import "std/sqlite")))
##   (def conn (db:open ":memory:"))
##   (db:exec conn "CREATE TABLE t (id INTEGER, name TEXT)")
##   (db:exec conn "INSERT INTO t VALUES (?1, ?2)" [1 "alice"])
##   (db:query conn "SELECT * FROM t")  => ({:id 1 :name "alice"})
##   (db:close conn)

(fn []
  (def lib (ffi/native "libsqlite3.so"))
  (def null-ptr (ptr/from-int 0))
  (def SQLITE_TRANSIENT (ptr/from-int -1))
  (def SQLITE_OK 0)
  (def SQLITE_ROW 100)
  (def SQLITE_DONE 101)

  ## ── C bindings ───────────────────────────────────────────────────

  (defn cfn [name ret args]
    (let [p (ffi/lookup lib name)
          s (ffi/signature ret args)]
      (fn [& a] (apply ffi/call p s a))))

  (def c-open (cfn "sqlite3_open" :int @[:string :ptr]))
  (def c-close (cfn "sqlite3_close" :int @[:ptr]))
  (def c-errmsg (cfn "sqlite3_errmsg" :ptr @[:ptr]))
  (def c-prepare (cfn "sqlite3_prepare_v2" :int @[:ptr :string :int :ptr :ptr]))
  (def c-step (cfn "sqlite3_step" :int @[:ptr]))
  (def c-finalize (cfn "sqlite3_finalize" :int @[:ptr]))
  (def c-col-count (cfn "sqlite3_column_count" :int @[:ptr]))
  (def c-col-name (cfn "sqlite3_column_name" :ptr @[:ptr :int]))
  (def c-col-type (cfn "sqlite3_column_type" :int @[:ptr :int]))
  (def c-col-int (cfn "sqlite3_column_int64" :i64 @[:ptr :int]))
  (def c-col-dbl (cfn "sqlite3_column_double" :double @[:ptr :int]))
  (def c-col-text (cfn "sqlite3_column_text" :ptr @[:ptr :int]))
  (def c-bind-int (cfn "sqlite3_bind_int64" :int @[:ptr :int :i64]))
  (def c-bind-dbl (cfn "sqlite3_bind_double" :int @[:ptr :int :double]))
  (def c-bind-text (cfn "sqlite3_bind_text" :int @[:ptr :int :string :int :ptr]))
  (def c-bind-blob (cfn "sqlite3_bind_blob" :int @[:ptr :int :ptr :int :ptr]))
  (def c-bind-null (cfn "sqlite3_bind_null" :int @[:ptr :int]))
  (def c-col-blob (cfn "sqlite3_column_blob" :ptr @[:ptr :int]))
  (def c-col-bytes (cfn "sqlite3_column_bytes" :int @[:ptr :int]))
  (def c-changes (cfn "sqlite3_changes" :int @[:ptr]))
  (def c-busy-timeout (cfn "sqlite3_busy_timeout" :int @[:ptr :int]))

  ## How long a writer waits for a busy database before raising. The default
  ## covers a concurrent test-runner pass over the shared session DB
  ## (docs/test-runner.md § Concurrent runs wait); past it the holder is wedged
  ## rather than slow, and failing is the honest answer.
  (def DEFAULT-BUSY-MS 30000)

  ## ── Bytes helpers ──────────────────────────────────────────────────

  (defn bytes->ptr [b]
    "Copy bytes into C memory. Caller must ffi/free the result."
    (let [n (length b)
          ptr (ffi/malloc n)]
      (ffi/write ptr (ffi/array :u8 n) b)
      ptr))

  (defn ptr->bytes [ptr n]
    "Read n bytes from a C pointer into a bytes value."
    (ffi/read ptr (ffi/array :u8 n)))

  ## ── Helpers ──────────────────────────────────────────────────────

  (defn check [db rc ctx]
    (unless (= rc SQLITE_OK)
      (error {:error :sqlite-error
              :message (string ctx ": " (ffi/string (c-errmsg db)))})))

  (defn prepare [db sql]
    "Prepare a statement. Returns stmt pointer. Caller must finalize."
    (let [pp (ffi/malloc 8)]
      (check db (c-prepare db sql -1 pp null-ptr) "prepare")
      (let [stmt (ffi/read pp :ptr)]
        (ffi/free pp)
        stmt)))

  (defn bind-params [db stmt params]
    (def @i 1)
    (each p in params
      (match (type-of p)
        :nil (check db (c-bind-null stmt i) "bind")
        :integer (check db (c-bind-int stmt i p) "bind")
        :float (check db (c-bind-dbl stmt i p) "bind")
        :string (check db (c-bind-text stmt i p -1 SQLITE_TRANSIENT) "bind")
        :bytes
          (let [n (length p)]
            (if (= n 0)  ## Empty blob: use a 1-byte alloc so sqlite sees type BLOB not NULL
              (let [ptr (ffi/malloc 1)]
                (check db (c-bind-blob stmt i ptr 0 SQLITE_TRANSIENT) "bind")
                (ffi/free ptr))
              (let [ptr (bytes->ptr p)]
                (check db (c-bind-blob stmt i ptr n SQLITE_TRANSIENT) "bind")
                (ffi/free ptr))))
        :boolean
          (check db (c-bind-int stmt i (if p 1 0)) "bind")

        ## A keyword binds as its bare name text (`(string :pass)` → "pass"),
        ## so
        ## enum-valued columns (result.status, result.tier) can be keyword-typed
        ## in callers while the column stays plain TEXT and SQL like
        ## `WHERE status = 'pass'` is unchanged.
        :keyword
          (check db (c-bind-text stmt i (string p) -1 SQLITE_TRANSIENT) "bind")
        t
          (error {:error :sqlite-error
                  :message (string "bind: unsupported type " t)}))
      (assign i (inc i))))

  (defn read-row [stmt ncols col-names]
    (let [row @{}]
      (each ci in (range ncols)
        (let [name (keyword (col-names ci))
              val (match (c-col-type stmt ci)
                    1 (c-col-int stmt ci)
                    2 (c-col-dbl stmt ci)
                    3 (ffi/string (c-col-text stmt ci))
                    4
                      (let [n (c-col-bytes stmt ci)
                            ptr (c-col-blob stmt ci)]
                        (if (> n 0) (ptr->bytes ptr n) (bytes)))
                    _ nil)]
          (put row name val)))
      (freeze row)))

  ## ── Public API ───────────────────────────────────────────────────

  (defn busy-ms []
    "The configured busy-wait, in milliseconds."
    (let [v (get (sys/env) "ELLE_SQLITE_BUSY_MS")]
      (if (nil? v)
        DEFAULT-BUSY-MS
        (let [[ok? n] (protect (parse-int v))]
          (if (and ok? (> n 0)) n DEFAULT-BUSY-MS)))))

  (defn open [path]
    "Open a SQLite database. Use \":memory:\" for in-memory.

   Every connection waits on a busy database rather than raising at once, and
   an on-disk one uses WAL journaling so a reader and a writer can share the
   file. Two processes writing one database is the normal case here — the test
   runner's session DB is a single path per user — so they must queue
   (docs/test-runner.md § Concurrent runs wait)."
    (let* [pp (ffi/malloc 8)
           rc (c-open path pp)
           db (ffi/read pp :ptr)]
      (ffi/free pp)
      (unless (= rc SQLITE_OK)
        (error {:error :sqlite-error
                :message (string "open: " (ffi/string (c-errmsg db)))}))
      (c-busy-timeout db (busy-ms))
      ## WAL is a property of the FILE, so `:memory:` answers `memory` and the
      ## request is a no-op there. A read-only directory can refuse the change;
      ## the connection still works journaled the old way, so do not fail the
      ## open over it. Driven through the raw statement calls rather than
      ## `exec`, which is defined below.
      (let [[ok? stmt] (protect (prepare db "PRAGMA journal_mode = WAL"))]
        (when ok?
          (c-step stmt)
          (c-finalize stmt)))
      db))

  (defn close [db]
    "Close a database connection."
    (c-close db)
    nil)

  (defn step-error [db stmt ctx]
    "Finalize stmt and raise the current sqlite error."
    (let [msg (ffi/string (c-errmsg db))]
      (c-finalize stmt)
      (error {:error :sqlite-error :message (string ctx ": " msg)})))

  (defn exec [db sql & opts]
    "Execute SQL (no result rows). Optional params array. Returns rows
   affected.  Raises :sqlite-error when the statement fails — including
   constraint violations and busy/locked errors.  (sqlite3_step's
   return code was once discarded here, silently swallowing failed
   writes.)"
    (let* [params (if (> (length opts) 0) (first opts) [])
           stmt (prepare db sql)]
      (bind-params db stmt params)
      (let [rc (c-step stmt)]
        (unless (or (= rc SQLITE_DONE) (= rc SQLITE_ROW))
          (step-error db stmt "step")))
      (let [n (c-changes db)]
        (c-finalize stmt)
        n)))

  (defn query [db sql & opts]
    "Execute a query. Returns list of structs with keyword keys.
   Raises :sqlite-error when a step fails mid-iteration — a failed
   step once ended the loop silently, returning truncated results."
    (let* [params (if (> (length opts) 0) (first opts) [])
           stmt (prepare db sql)]
      (bind-params db stmt params)
      (let* [ncols (c-col-count stmt)
             col-names (->array (map (fn [i] (ffi/string (c-col-name stmt i)))
                                     (->list (range ncols))))
             rows @[]]
        (def @rc (c-step stmt))
        (while (= rc SQLITE_ROW)
          (push rows (read-row stmt ncols col-names))
          (assign rc (c-step stmt)))
        (unless (= rc SQLITE_DONE) (step-error db stmt "step"))
        (c-finalize stmt)
        (->list rows))))

  {:open open :close close :exec exec :query query})
