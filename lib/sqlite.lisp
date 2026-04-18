(elle/epoch 7)
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
  (def SQLITE_OK   0)
  (def SQLITE_ROW  100)
  (def SQLITE_DONE 101)

  ## ── C bindings ───────────────────────────────────────────────────

  (defn cfn [name ret args]
    (let [p (ffi/lookup lib name) s (ffi/signature ret args)]
      (fn [& a] (apply ffi/call p s a))))

  (def c-open      (cfn "sqlite3_open"          :int @[:string :ptr]))
  (def c-close     (cfn "sqlite3_close"         :int @[:ptr]))
  (def c-errmsg    (cfn "sqlite3_errmsg"        :ptr @[:ptr]))
  (def c-prepare   (cfn "sqlite3_prepare_v2"    :int @[:ptr :string :int :ptr :ptr]))
  (def c-step      (cfn "sqlite3_step"          :int @[:ptr]))
  (def c-finalize  (cfn "sqlite3_finalize"      :int @[:ptr]))
  (def c-col-count (cfn "sqlite3_column_count"  :int @[:ptr]))
  (def c-col-name  (cfn "sqlite3_column_name"   :ptr @[:ptr :int]))
  (def c-col-type  (cfn "sqlite3_column_type"   :int @[:ptr :int]))
  (def c-col-int   (cfn "sqlite3_column_int64"  :i64 @[:ptr :int]))
  (def c-col-dbl   (cfn "sqlite3_column_double" :double @[:ptr :int]))
  (def c-col-text  (cfn "sqlite3_column_text"   :ptr @[:ptr :int]))
  (def c-bind-int  (cfn "sqlite3_bind_int64"    :int @[:ptr :int :i64]))
  (def c-bind-dbl  (cfn "sqlite3_bind_double"   :int @[:ptr :int :double]))
  (def c-bind-text (cfn "sqlite3_bind_text"     :int @[:ptr :int :string :int :ptr]))
  (def c-bind-null (cfn "sqlite3_bind_null"     :int @[:ptr :int]))
  (def c-changes   (cfn "sqlite3_changes"       :int @[:ptr]))

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
    (var i 1)
    (each p in params
      (match (type-of p)
        [:nil     (check db (c-bind-null stmt i) "bind")]
        [:integer (check db (c-bind-int stmt i p) "bind")]
        [:float   (check db (c-bind-dbl stmt i p) "bind")]
        [:string  (check db (c-bind-text stmt i p -1 SQLITE_TRANSIENT) "bind")]
        [:boolean (check db (c-bind-int stmt i (if p 1 0)) "bind")]
        [t (error {:error :sqlite-error
                   :message (string "bind: unsupported type " t)})])
      (assign i (inc i))))

  (defn read-row [stmt ncols col-names]
    (let [row @{}]
      (each ci in (range ncols)
        (let [name (keyword (col-names ci))
              val (match (c-col-type stmt ci)
                     [1 (c-col-int stmt ci)]
                     [2 (c-col-dbl stmt ci)]
                     [3 (ffi/string (c-col-text stmt ci))]
                     [_ nil])]
          (put row name val)))
      (freeze row)))

  ## ── Public API ───────────────────────────────────────────────────

  (defn open [path]
    "Open a SQLite database. Use \":memory:\" for in-memory."
    (let* [pp (ffi/malloc 8)
           rc (c-open path pp)
           db (ffi/read pp :ptr)]
      (ffi/free pp)
      (unless (= rc SQLITE_OK)
        (error {:error :sqlite-error
                :message (string "open: " (ffi/string (c-errmsg db)))}))
      db))

  (defn close [db]
    "Close a database connection."
    (c-close db)
    nil)

  (defn exec [db sql & opts]
    "Execute SQL (no result rows). Optional params array. Returns rows affected."
    (let* [params (if (> (length opts) 0) (first opts) [])
           stmt (prepare db sql)]
      (bind-params db stmt params)
      (c-step stmt)
      (let [n (c-changes db)]
        (c-finalize stmt)
        n)))

  (defn query [db sql & opts]
    "Execute a query. Returns list of structs with keyword keys."
    (let* [params (if (> (length opts) 0) (first opts) [])
           stmt (prepare db sql)]
      (bind-params db stmt params)
      (let* [ncols (c-col-count stmt)
             col-names (->array (map (fn [i] (ffi/string (c-col-name stmt i)))
                                      (->list (range ncols))))
             rows @[]]
        (while (= (c-step stmt) SQLITE_ROW)
          (push rows (read-row stmt ncols col-names)))
        (c-finalize stmt)
        (->list rows))))

  {:open open :close close :exec exec :query query})
