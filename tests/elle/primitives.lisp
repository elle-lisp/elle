
## === Read primitives ===

(assert (= (read "42") 42) "read integer")
(assert (= (read "\"hello\"") "hello") "read string")
(assert (= (read "true") true) "read boolean true")
(assert (= (read "false") false) "read boolean false")
(assert (pair? (read "(+ 1 2)")) "read list")
(let (([ok? _] (protect ((fn () (read 42)))))) (assert (not ok?) "read type error"))

(assert (= (first (read-all "1 2 3")) 1) "read-all multiple forms")
(assert (= (read-all "") ()) "read-all empty")
(let (([ok? _] (protect ((fn () (read-all 42)))))) (assert (not ok?) "read-all type error"))

## === Conversion primitives ===

(assert (= (integer 42) 42) "integer from int")
(assert (= (integer 3.7) 3) "integer from float")
(assert (= (integer "42") 42) "integer from string")
(let (([ok? _] (protect ((fn () (integer "abc")))))) (assert (not ok?) "integer from bad string"))
(let (([ok? _] (protect ((fn () (integer true)))))) (assert (not ok?) "integer type error"))

(assert (= (float 42) 42.0) "float from int")
(assert (= (float 2.5) 2.5) "float from float")
(assert (= (float "2.5") 2.5) "float from string")
(let (([ok? _] (protect ((fn () (float "abc")))))) (assert (not ok?) "float from bad string"))

(assert (= (string 42) "42") "string from int")
(assert (string? (string 3.14)) "string from float")
(assert (= (string true) "true") "string from bool true")
(assert (= (string false) "false") "string from bool false")
(assert (= (string nil) "nil") "string from nil")
(assert (= (string (list 1 2 3)) "(1 2 3)") "string from list")
(assert (= (string @[1 2 3]) "[1, 2, 3]") "string from array")

(assert (= (number->string 42) "42") "number->string int")
(assert (string? (number->string 3.14)) "number->string float")

(assert (= (integer "42") 42) "integer from string")
(assert (= (integer "-7") -7) "integer from string negative")
(assert (= (float "2.5") 2.5) "float from string")

(assert (= (any->string 42) "42") "any->string int")
(assert (= (any->string true) "true") "any->string bool")

(assert (= (string :foo) "foo") "string keyword")
(assert (= (string 42) "42") "string int")

(assert (= (symbol->string 'foo) "foo") "symbol->string")

## === Path primitives ===

(assert (string? (path/cwd)) "path/cwd returns string")
(assert (= (path/join "a" "b" "c") "a/b/c") "path/join multiple")
(assert (= (path/join "hello") "hello") "path/join single")
(let (([ok? _] (protect ((fn () (path/join 42)))))) (assert (not ok?) "path/join type error"))
(assert (= (path/join "a" "/b") "/b") "path/join absolute replaces")

(assert (= (path/parent "/home/user/data.txt") "/home/user") "path/parent")
(assert (= (path/parent "/") nil) "path/parent root")
(assert (= (path/parent "a/b/c") "a/b") "path/parent relative")

(assert (= (path/filename "/home/user/data.txt") "data.txt") "path/filename absolute")
(assert (= (path/filename "data.txt") "data.txt") "path/filename bare")
(assert (= (path/filename "/home/user/") "user") "path/filename trailing slash")

(assert (= (path/stem "data.txt") "data") "path/stem")
(assert (= (path/stem "archive.tar.gz") "archive.tar") "path/stem multiple dots")

(assert (= (path/extension "data.txt") "txt") "path/extension")
(assert (= (path/extension "noext") nil) "path/extension none")
(assert (= (path/extension "archive.tar.gz") "gz") "path/extension multiple dots")

(assert (= (path/with-extension "foo.txt" "rs") "foo.rs") "path/with-extension")
(assert (= (path/normalize "./a/../b") "b") "path/normalize")

(assert (string? (path/absolute "src")) "path/absolute returns string")
(assert (string? (path/canonicalize ".")) "path/canonicalize dot")
(let (([ok? _] (protect ((fn () (path/canonicalize "/nonexistent/path/xyz")))))) (assert (not ok?) "path/canonicalize nonexistent"))

(assert (= (path/relative "/foo/bar/baz" "/foo/bar") "baz") "path/relative")
(assert (= (length (path/components "/a/b/c")) 4) "path/components")

(assert (= (path/absolute? "/foo") true) "path/absolute? true")
(assert (= (path/absolute? "foo") false) "path/absolute? false")

(assert (= (path/relative? "foo") true) "path/relative? true")
(assert (= (path/relative? "/foo") false) "path/relative? false")

(assert (= (path/exists? ".") true) "path/exists? current dir")
(assert (= (path/exists? "/nonexistent/xyz") false) "path/exists? nonexistent")

(assert (= (path/file? "Cargo.toml") true) "path/file? true")
(assert (= (path/file? ".") false) "path/file? false")

(assert (= (path/dir? ".") true) "path/dir? true")
(assert (= (path/dir? "Cargo.toml") false) "path/dir? false")

## === Alias tests for predicates ===

(assert (= (file-exists? ".") true) "file-exists? alias")
(assert (= (directory? ".") true) "directory? alias")
(assert (= (file? "Cargo.toml") true) "file? alias")

## === Read edge cases ===

(assert (= (string (read ":hello")) "hello") "read keyword")
(assert (= (read "2.5") 2.5) "read float")
(assert (= (read "nil") nil) "read nil")
(let (([ok? _] (protect ((fn () (read "(+ 1")))))) (assert (not ok?) "read parse error"))

## === Conversion edge cases ===

(assert (= (integer 0) 0) "integer zero")
(assert (= (integer -42) -42) "integer negative")
(assert (= (float 0) 0.0) "float zero")
(assert (= (string :hello) "hello") "string from keyword")
(assert (string? (string (list))) "string from empty list")

## === Alias tests ===

(assert (= (integer "42") 42) "integer from string")
(assert (= (int 42) 42) "int alias for integer")

## === Type predicates for collections ===

(assert (= (array? @[1 2 3]) true) "array? true for mutable array")
(assert (= (array? [1 2 3]) true) "array? true for immutable array")
(assert (= (array? 42) false) "array? false for other")
(assert (= (array? "hello") false) "array? false for string")

(assert (= (struct? @{:a 1 :b 2}) true) "struct? true for mutable struct")
(assert (= (struct? {:a 1 :b 2}) true) "struct? true for immutable struct")
(assert (= (struct? 42) false) "struct? false for other")
(assert (= (struct? "hello") false) "struct? false for string")
(assert (= (struct? "hello") false) "struct? false string")

(assert (= (empty? []) true) "empty? array true")
(assert (= (empty? [1]) false) "empty? array false")

(assert (= (empty? @[]) true) "empty? array true")
(assert (= (empty? @[1]) false) "empty? array false")

## === fn/errors? introspection ===

(assert (= (fn/errors? (fn (x) x)) false) "fn/errors? pure closure")
(assert (= (fn/errors? 42) false) "fn/errors? non-closure")
(assert (= (fn/errors? "hello") false) "fn/errors? string")

## === take/drop negative count ===

(let (([ok? _] (protect ((fn () (take -1 (list 1 2 3))))))) (assert (not ok?) "take negative count"))
(let (([ok? _] (protect ((fn () (drop -1 (list 1 2 3))))))) (assert (not ok?) "drop negative count"))

(assert (= (take 0 (list 1 2 3)) ()) "take zero")
(assert (= (drop 0 (list 1 2 3)) (list 1 2 3)) "drop zero")

## === Bitwise float truncation ===

(assert (= (bit/and 12.7 10.3) 8) "bit/and float truncation")
(assert (= (bit/or 12.7 10.3) 14) "bit/or float truncation")
(assert (= (bit/xor 12.7 10.3) 6) "bit/xor float truncation")
(assert (= (bit/not 0.9) -1) "bit/not float truncation")
(assert (= (bit/shl 1.9 3) 8) "bit/shl float value")
(assert (= (bit/shr 8.7 2) 2) "bit/shr float value")

(let (([ok? _] (protect ((fn () (bit/and (sqrt -1.0) 1)))))) (assert (not ok?) "bit/and NaN error"))
(let (([ok? _] (protect ((fn () (bit/and (exp 1000.0) 1)))))) (assert (not ok?) "bit/and infinity error"))

(assert (= (bit/and -3.7 255) (bit/and -3 255)) "bit/and negative float")

## === mutable? predicate ===

# Mutable collections
(assert (mutable? @[1 2 3]) "mutable? true for @array")
(assert (mutable? @"hello") "mutable? true for @string")
(assert (mutable? (@bytes 1 2 3)) "mutable? true for @bytes")
(assert (mutable? @{:a 1}) "mutable? true for @struct")
(assert (mutable? @|1 2 3|) "mutable? true for @set")
(assert (mutable? (box 42)) "mutable? true for box")
(assert (mutable? (make-parameter 0)) "mutable? true for parameter")

# Immutable collections
(assert (not (mutable? [1 2 3])) "mutable? false for array")
(assert (not (mutable? "hello")) "mutable? false for string")
(assert (not (mutable? (bytes 1 2 3))) "mutable? false for bytes")
(assert (not (mutable? {:a 1})) "mutable? false for struct")
(assert (not (mutable? |1 2 3|)) "mutable? false for set")

# Other types
(assert (not (mutable? 42)) "mutable? false for integer")
(assert (not (mutable? 3.14)) "mutable? false for float")
(assert (not (mutable? true)) "mutable? false for boolean")
(assert (not (mutable? nil)) "mutable? false for nil")
(assert (not (mutable? :foo)) "mutable? false for keyword")
(assert (not (mutable? (fn (x) x))) "mutable? false for closure")
(assert (not (mutable? +)) "mutable? false for primitive")
(assert (not (mutable? (cons 1 2))) "mutable? false for cons")

## === box? predicate ===

(assert (box? (box 42)) "box? true for box")
(assert (not (box? 42)) "box? false for integer")
(assert (not (box? @[1 2 3])) "box? false for @array")
(assert (not (box? nil)) "box? false for nil")

## === first polymorphism ===

(assert (= (first (list 1 2 3)) 1) "first list")
(assert (= (first (list)) nil) "first empty list")
(assert (= (first [1 2 3]) 1) "first array")
(assert (= (first []) nil) "first empty array")
(assert (= (first @[1 2 3]) 1) "first array")
(assert (= (first @[]) nil) "first empty array")
(assert (= (first "abc") "a") "first string")
(assert (= (first "") nil) "first empty string")
(let (([ok? _] (protect ((fn () (first 42)))))) (assert (not ok?) "first non-sequence error"))

## === rest polymorphism ===

(assert (= (first (rest (list 1 2 3))) 2) "rest list")
(assert (= (rest (list)) ()) "rest empty list")
(assert (= (rest (list 1)) ()) "rest single list")

(assert (= (length (rest [1 2 3])) 2) "rest array length")
(assert (= (array? (rest [1 2 3])) true) "rest array type")

(assert (= (array? (rest [])) true) "rest empty array type")
(assert (= (length (rest [])) 0) "rest empty array length")

(assert (= (length (rest @[1 2 3])) 2) "rest array length")
(assert (= (array? (rest @[1 2 3])) true) "rest array type")

(assert (= (array? (rest @[])) true) "rest empty array type")
(assert (= (length (rest @[])) 0) "rest empty array length")

(assert (= (rest "abc") "bc") "rest string")
(assert (= (rest "") "") "rest empty string")
(assert (= (rest "a") "") "rest single string")

(let (([ok? _] (protect ((fn () (rest 42)))))) (assert (not ok?) "rest non-sequence error"))

## === reverse polymorphism ===

(assert (= (first (reverse (list 1 2 3))) 3) "reverse list")
(assert (= (reverse (list)) ()) "reverse empty list")

(assert (= (array? (reverse [1 2 3])) true) "reverse array type")
(assert (= (get (reverse [1 2 3]) 0) 3) "reverse array first")

(assert (= (array? (reverse [])) true) "reverse empty array type")

(assert (= (array? (reverse @[1 2 3])) true) "reverse array type")
(assert (= (get (reverse @[1 2 3]) 0) 3) "reverse array first")

(assert (= (array? (reverse @[])) true) "reverse empty array type")

(assert (= (reverse "abc") "cba") "reverse string")
(assert (= (reverse "") "") "reverse empty string")

(let (([ok? _] (protect ((fn () (reverse 42)))))) (assert (not ok?) "reverse non-sequence error"))
