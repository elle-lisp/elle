(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## === Read primitives ===

(assert-eq (read "42") 42 "read integer")
(assert-eq (read "\"hello\"") "hello" "read string")
(assert-eq (read "true") true "read boolean true")
(assert-eq (read "false") false "read boolean false")
(assert-true (pair? (read "(+ 1 2)")) "read list")
(assert-err (fn () (read 42)) "read type error")

(assert-eq (first (read-all "1 2 3")) 1 "read-all multiple forms")
(assert-eq (read-all "") () "read-all empty")
(assert-err (fn () (read-all 42)) "read-all type error")

## === Conversion primitives ===

(assert-eq (integer 42) 42 "integer from int")
(assert-eq (integer 3.7) 3 "integer from float")
(assert-eq (integer "42") 42 "integer from string")
(assert-err (fn () (integer "abc")) "integer from bad string")
(assert-err (fn () (integer true)) "integer type error")

(assert-eq (float 42) 42.0 "float from int")
(assert-eq (float 2.5) 2.5 "float from float")
(assert-eq (float "2.5") 2.5 "float from string")
(assert-err (fn () (float "abc")) "float from bad string")

(assert-eq (string 42) "42" "string from int")
(assert-true (string? (string 3.14)) "string from float")
(assert-eq (string true) "true" "string from bool true")
(assert-eq (string false) "false" "string from bool false")
(assert-eq (string nil) "nil" "string from nil")
(assert-eq (string (list 1 2 3)) "(1 2 3)" "string from list")
(assert-eq (string @[1 2 3]) "[1, 2, 3]" "string from array")

(assert-eq (number->string 42) "42" "number->string int")
(assert-true (string? (number->string 3.14)) "number->string float")

(assert-eq (integer "42") 42 "integer from string")
(assert-eq (integer "-7") -7 "integer from string negative")
(assert-eq (float "2.5") 2.5 "float from string")

(assert-eq (any->string 42) "42" "any->string int")
(assert-eq (any->string true) "true" "any->string bool")

(assert-eq (string :foo) "foo" "string keyword")
(assert-eq (string 42) "42" "string int")

(assert-eq (symbol->string 'foo) "foo" "symbol->string")

## === Path primitives ===

(assert-true (string? (path/cwd)) "path/cwd returns string")
(assert-eq (path/join "a" "b" "c") "a/b/c" "path/join multiple")
(assert-eq (path/join "hello") "hello" "path/join single")
(assert-err (fn () (path/join 42)) "path/join type error")
(assert-eq (path/join "a" "/b") "/b" "path/join absolute replaces")

(assert-eq (path/parent "/home/user/data.txt") "/home/user" "path/parent")
(assert-eq (path/parent "/") nil "path/parent root")
(assert-eq (path/parent "a/b/c") "a/b" "path/parent relative")

(assert-eq (path/filename "/home/user/data.txt") "data.txt" "path/filename absolute")
(assert-eq (path/filename "data.txt") "data.txt" "path/filename bare")
(assert-eq (path/filename "/home/user/") "user" "path/filename trailing slash")

(assert-eq (path/stem "data.txt") "data" "path/stem")
(assert-eq (path/stem "archive.tar.gz") "archive.tar" "path/stem multiple dots")

(assert-eq (path/extension "data.txt") "txt" "path/extension")
(assert-eq (path/extension "noext") nil "path/extension none")
(assert-eq (path/extension "archive.tar.gz") "gz" "path/extension multiple dots")

(assert-eq (path/with-extension "foo.txt" "rs") "foo.rs" "path/with-extension")
(assert-eq (path/normalize "./a/../b") "b" "path/normalize")

(assert-true (string? (path/absolute "src")) "path/absolute returns string")
(assert-true (string? (path/canonicalize ".")) "path/canonicalize dot")
(assert-err (fn () (path/canonicalize "/nonexistent/path/xyz")) "path/canonicalize nonexistent")

(assert-eq (path/relative "/foo/bar/baz" "/foo/bar") "baz" "path/relative")
(assert-eq (length (path/components "/a/b/c")) 4 "path/components")

(assert-eq (path/absolute? "/foo") true "path/absolute? true")
(assert-eq (path/absolute? "foo") false "path/absolute? false")

(assert-eq (path/relative? "foo") true "path/relative? true")
(assert-eq (path/relative? "/foo") false "path/relative? false")

(assert-eq (path/exists? ".") true "path/exists? current dir")
(assert-eq (path/exists? "/nonexistent/xyz") false "path/exists? nonexistent")

(assert-eq (path/file? "Cargo.toml") true "path/file? true")
(assert-eq (path/file? ".") false "path/file? false")

(assert-eq (path/dir? ".") true "path/dir? true")
(assert-eq (path/dir? "Cargo.toml") false "path/dir? false")

## === Alias tests for predicates ===

(assert-eq (file-exists? ".") true "file-exists? alias")
(assert-eq (directory? ".") true "directory? alias")
(assert-eq (file? "Cargo.toml") true "file? alias")

## === Read edge cases ===

(assert-eq (string (read ":hello")) "hello" "read keyword")
(assert-eq (read "2.5") 2.5 "read float")
(assert-eq (read "nil") nil "read nil")
(assert-err (fn () (read "(+ 1")) "read parse error")

## === Conversion edge cases ===

(assert-eq (integer 0) 0 "integer zero")
(assert-eq (integer -42) -42 "integer negative")
(assert-eq (float 0) 0.0 "float zero")
(assert-eq (string :hello) "hello" "string from keyword")
(assert-true (string? (string (list))) "string from empty list")

## === Alias tests ===

(assert-eq (string->int "42") 42 "string->int alias")
(assert-eq (int 42) 42 "int alias")

## === Type predicates for collections ===

(assert-eq (array? @[1 2 3]) true "array? true for mutable array")
(assert-eq (array? [1 2 3]) true "array? true for immutable array")
(assert-eq (array? 42) false "array? false for other")
(assert-eq (array? "hello") false "array? false for string")

(assert-eq (struct? @{:a 1 :b 2}) true "struct? true for mutable struct")
(assert-eq (struct? {:a 1 :b 2}) true "struct? true for immutable struct")
(assert-eq (struct? 42) false "struct? false for other")
(assert-eq (struct? "hello") false "struct? false for string")
(assert-eq (struct? "hello") false "struct? false string")

(assert-eq (empty? []) true "empty? array true")
(assert-eq (empty? [1]) false "empty? array false")

(assert-eq (empty? @[]) true "empty? array true")
(assert-eq (empty? @[1]) false "empty? array false")

## === fn/errors? introspection ===

(assert-eq (fn/errors? (fn (x) x)) false "fn/errors? pure closure")
(assert-eq (fn/errors? 42) false "fn/errors? non-closure")
(assert-eq (fn/errors? "hello") false "fn/errors? string")

## === take/drop negative count ===

(assert-err (fn () (take -1 (list 1 2 3))) "take negative count")
(assert-err (fn () (drop -1 (list 1 2 3))) "drop negative count")

(assert-eq (take 0 (list 1 2 3)) () "take zero")
(assert-eq (drop 0 (list 1 2 3)) (list 1 2 3) "drop zero")

## === Bitwise float truncation ===

(assert-eq (bit/and 12.7 10.3) 8 "bit/and float truncation")
(assert-eq (bit/or 12.7 10.3) 14 "bit/or float truncation")
(assert-eq (bit/xor 12.7 10.3) 6 "bit/xor float truncation")
(assert-eq (bit/not 0.9) -1 "bit/not float truncation")
(assert-eq (bit/shl 1.9 3) 8 "bit/shl float value")
(assert-eq (bit/shr 8.7 2) 2 "bit/shr float value")

(assert-err (fn () (bit/and (sqrt -1.0) 1)) "bit/and NaN error")
(assert-err (fn () (bit/and (exp 1000.0) 1)) "bit/and infinity error")

(assert-eq (bit/and -3.7 255) (bit/and -3 255) "bit/and negative float")

## === first polymorphism ===

(assert-eq (first (list 1 2 3)) 1 "first list")
(assert-eq (first (list)) nil "first empty list")
(assert-eq (first [1 2 3]) 1 "first array")
(assert-eq (first []) nil "first empty array")
(assert-eq (first @[1 2 3]) 1 "first array")
(assert-eq (first @[]) nil "first empty array")
(assert-eq (first "abc") "a" "first string")
(assert-eq (first "") nil "first empty string")
(assert-err (fn () (first 42)) "first non-sequence error")

## === rest polymorphism ===

(assert-eq (first (rest (list 1 2 3))) 2 "rest list")
(assert-eq (rest (list)) () "rest empty list")
(assert-eq (rest (list 1)) () "rest single list")

(assert-eq (length (rest [1 2 3])) 2 "rest array length")
(assert-eq (array? (rest [1 2 3])) true "rest array type")

(assert-eq (array? (rest [])) true "rest empty array type")
(assert-eq (length (rest [])) 0 "rest empty array length")

(assert-eq (length (rest @[1 2 3])) 2 "rest array length")
(assert-eq (array? (rest @[1 2 3])) true "rest array type")

(assert-eq (array? (rest @[])) true "rest empty array type")
(assert-eq (length (rest @[])) 0 "rest empty array length")

(assert-eq (rest "abc") "bc" "rest string")
(assert-eq (rest "") "" "rest empty string")
(assert-eq (rest "a") "" "rest single string")

(assert-err (fn () (rest 42)) "rest non-sequence error")

## === reverse polymorphism ===

(assert-eq (first (reverse (list 1 2 3))) 3 "reverse list")
(assert-eq (reverse (list)) () "reverse empty list")

(assert-eq (array? (reverse [1 2 3])) true "reverse array type")
(assert-eq (get (reverse [1 2 3]) 0) 3 "reverse array first")

(assert-eq (array? (reverse [])) true "reverse empty array type")

(assert-eq (array? (reverse @[1 2 3])) true "reverse array type")
(assert-eq (get (reverse @[1 2 3]) 0) 3 "reverse array first")

(assert-eq (array? (reverse @[])) true "reverse empty array type")

(assert-eq (reverse "abc") "cba" "reverse string")
(assert-eq (reverse "") "" "reverse empty string")

(assert-err (fn () (reverse 42)) "reverse non-sequence error")
