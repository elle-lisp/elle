#!/usr/bin/env elle

# Files, JSON, and modules
#
# Demonstrates:
#   Module loading    — import-file loads .lisp files
#   File read/write   — slurp, spit, append-file, read-lines
#   File info         — file-exists?, file?, directory?, file-size
#   File ops          — delete-file, rename-file, copy-file
#   Directory ops     — create-directory, create-directory-all,
#                       delete-directory, list-directory
#   Path ops          — path/filename, path/extension, path/parent,
#                       path/join, path/cwd
#   JSON parse        — json-parse for null, bool, int, float, string,
#                       array, object, nested
#   JSON serialize    — json-serialize, json-serialize-pretty, round-trip

# import-file loads another .lisp file and returns its last expression's
# value. assertions.lisp is loaded at the top of every example — this
# line IS the module-loading demonstration.
(import-file "./examples/assertions.lisp")


# All temp files live under a unique directory relative to the working dir.
(def tmp-dir
  (string/join (list ".elle-test-"
                     (string (integer (* (clock/monotonic) 1000000000)))) ""))
(create-directory-all tmp-dir)

(defn tmp [name]
  "Build a path inside the temp directory."
  (path/join tmp-dir name))


# ========================================
# 1. Read/write files
# ========================================

(spit (tmp "hello.txt") "Hello, Elle!")
(def content (slurp (tmp "hello.txt")))
(display "  slurp: ") (print content)
(assert-eq content "Hello, Elle!" "spit then slurp round-trips")

(append-file (tmp "hello.txt") "\nSecond line.")
(def appended (slurp (tmp "hello.txt")))
(assert-eq appended "Hello, Elle!\nSecond line." "append-file adds content")

(spit (tmp "lines.txt") "alpha\nbeta\ngamma\n")
(def lines (read-lines (tmp "lines.txt")))
(display "  lines: ") (print lines)
(assert-eq (length lines) 3 "read-lines splits on newlines")


# ========================================
# 2. File info
# ========================================

(assert-true (file-exists? (tmp "hello.txt")) "file-exists? on existing file")
(assert-false (file-exists? (tmp "nope.txt")) "file-exists? on missing file")
(assert-true (file? (tmp "hello.txt")) "file? on file")
(assert-true (directory? tmp-dir) "directory? on directory")

(def size (file-size (tmp "hello.txt")))
(display "  file-size: ") (print size)
(assert-eq size 25 "file-size returns byte count")


# ========================================
# 3. File operations
# ========================================

(copy-file (tmp "hello.txt") (tmp "copy.txt"))
(assert-true (file-exists? (tmp "copy.txt")) "copy-file creates target")
(assert-eq (slurp (tmp "copy.txt")) (slurp (tmp "hello.txt"))
  "copy-file preserves content")

(rename-file (tmp "copy.txt") (tmp "renamed.txt"))
(assert-false (file-exists? (tmp "copy.txt")) "rename-file removes source")
(assert-true (file-exists? (tmp "renamed.txt")) "rename-file creates target")


# ========================================
# 4. Directory operations
# ========================================

(def sub (tmp "sub"))
(create-directory sub)
(assert-true (directory? sub) "create-directory works")

(def deep (path/join tmp-dir "a" "b" "c"))
(create-directory-all deep)
(assert-true (directory? deep) "create-directory-all creates nested dirs")

# Populate a directory and list it.
(spit (path/join sub "one.txt") "1")
(spit (path/join sub "two.txt") "2")
(def entries (list-directory sub))
(display "  list-directory: ") (print entries)
(assert-eq (length entries) 2 "list-directory returns all entries")


# ========================================
# 5. Path operations
# ========================================

(def p "/home/user/docs/report.pdf")
(assert-eq (path/filename p) "report.pdf" "path/filename")
(assert-eq (path/extension p) "pdf" "path/extension")
(assert-eq (path/parent p) "/home/user/docs" "path/parent")

(def joined (path/join "a" "b" "c.txt"))
(display "  path/join: ") (print joined)
(assert-eq joined "a/b/c.txt" "path/join composes segments")

(def cwd (path/cwd))
(display "  path/cwd: ") (print cwd)
(assert-true (> (length cwd) 0) "path/cwd returns non-empty string")


# ========================================
# 6. JSON: parsing scalars
# ========================================

(assert-eq (json-parse "null") nil "json-parse null")
(assert-eq (json-parse "true") true "json-parse true")
(assert-eq (json-parse "false") false "json-parse false")
(assert-eq (json-parse "42") 42 "json-parse integer")
(assert-eq (json-parse "3.14") 3.14 "json-parse float")
(assert-eq (json-parse "\"hello\"") "hello" "json-parse string")


# ========================================
# 7. JSON: serializing scalars
# ========================================

(assert-eq (json-serialize nil) "null" "json-serialize nil")
(assert-eq (json-serialize true) "true" "json-serialize true")
(assert-eq (json-serialize false) "false" "json-serialize false")
(assert-eq (json-serialize 42) "42" "json-serialize int")
(assert-eq (json-serialize 3.14) "3.14" "json-serialize float")
(assert-eq (json-serialize "hello") "\"hello\"" "json-serialize string")


# ========================================
# 8. JSON: collections and nesting
# ========================================

(def arr (json-parse "[1, \"two\", true, null]"))
(display "  parsed array: ") (print arr)
(assert-eq (length arr) 4 "json-parse array length")
(assert-eq (get arr 1) "two" "json-parse array element")

(def obj (json-parse "{\"name\": \"Alice\", \"age\": 30}"))
(display "  parsed object: ") (print obj)
(assert-eq (get obj "name") "Alice" "json-parse object field")
(assert-eq (get obj "age") 30 "json-parse object field int")

(def nested (json-parse "{\"user\": {\"name\": \"Bob\", \"scores\": [95, 87]}}"))
(def user (get nested "user"))
(assert-eq (get user "name") "Bob" "nested object access")
(assert-eq (get (get user "scores") 0) 95 "nested array access")


# ========================================
# 9. JSON: round-trip
# ========================================

# Serialize a list as a JSON array.
(assert-eq (json-serialize (list 1 2 3)) "[1,2,3]" "list serializes as array")

# Parse → modify → serialize.
(def product (json-parse "{\"name\": \"Widget\", \"price\": 19.99}"))
(put product "price" 24.99)
(put product "sale" true)
(def updated-json (json-serialize product))
(display "  round-trip: ") (print updated-json)

# Pretty-print for readability.
(def pretty (json-serialize-pretty product))
(display "  pretty:\n") (display pretty) (print "")

# Verify the modified value survived the round-trip.
(def reparsed (json-parse updated-json))
(assert-eq (get reparsed "price") 24.99 "round-trip preserves modified value")
(assert-eq (get reparsed "sale") true "round-trip preserves added field")


# ========================================
# 10. JSON: file I/O integration
# ========================================

# Write JSON to a file and read it back — the natural use case.
(def config (table))
(put config "app" "elle-test")
(put config "version" 1)
(put config "debug" false)

(spit (tmp "config.json") (json-serialize-pretty config))
(def loaded (json-parse (slurp (tmp "config.json"))))
(display "  config from file: ") (print loaded)
(assert-eq (get loaded "app") "elle-test" "JSON config round-trips through file")
(assert-eq (get loaded "version") 1 "JSON config preserves int")


# ========================================
# Cleanup
# ========================================

# Remove all files and directories we created.
(delete-file (tmp "hello.txt"))
(delete-file (tmp "lines.txt"))
(delete-file (tmp "renamed.txt"))
(delete-file (tmp "config.json"))
(delete-file (path/join sub "one.txt"))
(delete-file (path/join sub "two.txt"))
(delete-directory sub)
(delete-directory deep)
(delete-directory (path/join tmp-dir "a" "b"))
(delete-directory (path/join tmp-dir "a"))
(delete-directory tmp-dir)

# Verify cleanup.
(assert-false (file-exists? tmp-dir) "temp directory removed")

(print "")
(print "all io passed.")
