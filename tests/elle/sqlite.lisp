(elle/epoch 12)
## SQLite module tests (FFI to libsqlite3)

# Gate the whole file on libsqlite3: if it can't load, re-raise as a loud :gated
# so `elle test` records a file-level SKIP with a reason (docs/test-runner.md
# § Gating). This is an eager (def …), so it runs during the barrier-module
# setup and gates before any test thunk. Never (exit 0): under the runner that
# would terminate the process mid-run and silently drop every later form.
(def _libsqlite3
  (let [r (protect (ffi/native "libsqlite3.so"))]
    (if (get r 0)
      true
      (error (struct :error :gated :reason "libsqlite3.so not installed")))))

(def db ((import "std/sqlite")))

(def conn (db:open ":memory:"))

## Create, insert, query
(db:exec conn
         "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, score REAL)")
(db:exec conn "INSERT INTO users VALUES (?1, ?2, ?3)" [1 "alice" 95.5])
(db:exec conn "INSERT INTO users VALUES (?1, ?2, ?3)" [2 "bob" 87.0])
(db:exec conn "INSERT INTO users VALUES (?1, ?2, ?3)" [3 "charlie" nil])

(let* [rows (db:query conn "SELECT * FROM users")
       alice (first rows)
       bob (nth 1 rows)
       charlie (nth 2 rows)]
  (assert (= (length rows) 3) "row count")
  (assert (= alice:name "alice") "text value")
  (assert (= alice:id 1) "integer value")
  (assert (= alice:score 95.5) "float value")
  (assert (= bob:name "bob") "second row")
  (assert (nil? charlie:score) "null value"))

## Parameterized query
(let* [rows (db:query conn "SELECT * FROM users WHERE score > ?1" [90.0])
       r (first rows)]
  (assert (= (length rows) 1) "filtered count")
  (assert (= r:name "alice") "filtered name"))

## Exec returns rows affected
(assert (= (db:exec conn "UPDATE users SET score = 100 WHERE name = ?1"
                    ["alice"]) 1) "rows affected")

## Boolean binding (stored as integer)
(db:exec conn "CREATE TABLE flags (active INTEGER)")
(db:exec conn "INSERT INTO flags VALUES (?1)" [true])
(let* [rows (db:query conn "SELECT * FROM flags")
       r (first rows)]
  (assert (= r:active 1) "bool stored as 1"))

## Empty result
(let [rows (db:query conn "SELECT * FROM users WHERE id = 999")]
  (assert (= (length rows) 0) "empty result"))

## Blob binding and reading
(db:exec conn "CREATE TABLE blobs (id INTEGER, data BLOB)")
(def test-blob (bytes 0 1 127 128 200 255))
(db:exec conn "INSERT INTO blobs VALUES (?1, ?2)" [1 test-blob])
(db:exec conn "INSERT INTO blobs VALUES (?1, ?2)" [2 (bytes)])
(db:exec conn "INSERT INTO blobs VALUES (?1, ?2)" [3 (bytes 42)])

(let* [rows (db:query conn "SELECT * FROM blobs ORDER BY id")
       r1 (first rows)
       r2 (nth 1 rows)
       r3 (nth 2 rows)]
  (assert (= (type-of r1:data) :bytes) "blob type is bytes")
  (assert (= r1:data test-blob) "blob round-trip")
  (assert (= (length r1:data) 6) "blob length")
  (assert (= r2:data (bytes)) "empty blob")
  (assert (= r3:data (bytes 42)) "single-byte blob"))

## Error on bad SQL
(let [[ok? err] (protect ((fn [] (db:exec conn "NOT VALID SQL"))))]
  (assert (not ok?) "bad sql errors")
  (assert (= err:error :sqlite-error) "sqlite error type"))

## Constraint violation raises from exec.  sqlite3_step's return code
## was once discarded, so PK/UNIQUE/NOT NULL violations silently
## swallowed the write and returned 0 — callers had no way to tell a
## dropped row from a no-op.
(db:exec conn "CREATE TABLE pk (a INTEGER, b INTEGER, PRIMARY KEY (a, b))")
(db:exec conn "INSERT INTO pk VALUES (1, 2)")
(let [[ok? err] (protect ((fn [] (db:exec conn "INSERT INTO pk VALUES (1, 2)"))))]
  (assert (not ok?) "pk violation errors")
  (assert (= err:error :sqlite-error) "pk violation error type"))
(let [rows (db:query conn "SELECT COUNT(*) AS n FROM pk")]
  (assert (= (let [r (first rows)]
               r:n) 1) "violating insert wrote nothing"))
(db:exec conn "CREATE TABLE nn (a INTEGER NOT NULL)")
(let [[ok? _] (protect ((fn [] (db:exec conn "INSERT INTO nn VALUES (NULL)"))))]
  (assert (not ok?) "not-null violation errors"))  ## The connection stays usable after a constraint error.
(db:exec conn "INSERT INTO pk VALUES (1, 3)")
(let [rows (db:query conn "SELECT COUNT(*) AS n FROM pk")]
  (assert (= (let [r (first rows)]
               r:n) 2) "connection usable after violation"))

(db:close conn)

(println "sqlite: all tests passed")
