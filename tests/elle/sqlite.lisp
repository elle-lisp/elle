(elle/epoch 9)
## SQLite module tests (FFI to libsqlite3)

(def [ok? _] (protect ((fn [] (ffi/native "libsqlite3.so")))))
(unless ok? (println "SKIP: libsqlite3.so not available") (exit 0))

(def db ((import "std/sqlite")))

(def conn (db:open ":memory:"))

## Create, insert, query
(db:exec conn "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, score REAL)")
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
(assert (= (db:exec conn "UPDATE users SET score = 100 WHERE name = ?1" ["alice"]) 1)
        "rows affected")

## Boolean binding (stored as integer)
(db:exec conn "CREATE TABLE flags (active INTEGER)")
(db:exec conn "INSERT INTO flags VALUES (?1)" [true])
(let* [rows (db:query conn "SELECT * FROM flags")
       r (first rows)]
  (assert (= r:active 1) "bool stored as 1"))

## Empty result
(let [rows (db:query conn "SELECT * FROM users WHERE id = 999")]
  (assert (= (length rows) 0) "empty result"))

## Error on bad SQL
(let [[ok? err] (protect ((fn [] (db:exec conn "NOT VALID SQL"))))]
  (assert (not ok?) "bad sql errors")
  (assert (= err:error :sqlite-error) "sqlite error type"))

(db:close conn)

(println "sqlite: all tests passed")
