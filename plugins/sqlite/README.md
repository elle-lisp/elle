# elle-sqlite

A SQLite plugin for Elle, wrapping the Rust `rusqlite` crate with bundled SQLite.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_sqlite.so` (or `target/release/libelle_sqlite.so`).

## Usage

```lisp
(import-file "path/to/libelle_sqlite.so")

(def db (sqlite/open ":memory:"))
(sqlite/execute db "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)")
(sqlite/execute db "INSERT INTO users (name, age) VALUES (?1, ?2)" (list "Alice" 30))
(sqlite/execute db "INSERT INTO users (name, age) VALUES (?1, ?2)" (list "Bob" 25))

(def rows (sqlite/query db "SELECT * FROM users"))
;; => ({:id 1 :name "Alice" :age 30} {:id 2 :name "Bob" :age 25})

(def older (sqlite/query db "SELECT name FROM users WHERE age > ?1" (list 28)))
;; => ({:name "Alice"})

(sqlite/close db)
```

## Primitives

| Name | Args | Returns |
|------|------|---------|
| `sqlite/open` | path | database connection |
| `sqlite/close` | db | nil |
| `sqlite/execute` | db, sql, params? | rows affected (integer) |
| `sqlite/query` | db, sql, params? | list of structs |
