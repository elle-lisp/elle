#!/usr/bin/env elle
;; JSON parsing and serialization examples

(import-file "./examples/assertions.lisp")

;; Example 1: Parse various JSON types
(display "=== Example 1: Parsing JSON ===")
(newline)

(var json-null (json-parse "null"))
(display "Parsed null: ")
(display json-null)
(newline)
(assert-eq json-null nil "json-parse null returns nil")

(var json-bool (json-parse "true"))
(display "Parsed true: ")
(display json-bool)
(newline)
(assert-true (eq? json-bool (json-parse "true")) "json-parse true returns true")

(var json-int (json-parse "42"))
(display "Parsed 42: ")
(display json-int)
(newline)
(assert-eq json-int 42 "json-parse 42 returns 42")

(var json-float (json-parse "3.14"))
(display "Parsed 3.14: ")
(display json-float)
(newline)
(assert-eq json-float 3.14 "json-parse 3.14 returns 3.14")

(var json-string (json-parse "\"hello world\""))
(display "Parsed string: ")
(display json-string)
(newline)
(assert-eq json-string "hello world" "json-parse string returns correct value")

;; Example 2: Parse arrays
(display "\n=== Example 2: Parsing Arrays ===")
(newline)

(var json-array (json-parse "[1, 2, 3, 4, 5]"))
(display "Parsed array: ")
(display json-array)
(newline)
(assert-eq (length json-array) 5 "Array has 5 elements")
(assert-eq (get json-array 0) 1 "First array element is 1")
(assert-eq (get json-array 4) 5 "Last array element is 5")

(var mixed-array (json-parse "[1, \"two\", true, null, 3.14]"))
(display "Mixed array: ")
(display mixed-array)
(newline)
(assert-eq (length mixed-array) 5 "Mixed array has 5 elements")
(assert-eq (get mixed-array 1) "two" "Second element is string 'two'")

;; Example 3: Parse objects
(display "\n=== Example 3: Parsing Objects ===")
(newline)

(var json-obj (json-parse "{\"name\": \"Alice\", \"age\": 30, \"active\": true}"))
(display "Parsed object: ")
(display json-obj)
(newline)

;; Access object fields using get
(var name (get json-obj "name"))
(display "Name from object: ")
(display name)
(newline)
(assert-eq name "Alice" "Object field 'name' is 'Alice'")

(var age (get json-obj "age"))
(display "Age from object: ")
(display age)
(newline)
(assert-eq age 30 "Object field 'age' is 30")

;; Example 4: Nested structures
(display "\n=== Example 4: Nested Structures ===")
(newline)

(var nested-json (json-parse "{\"user\": {\"name\": \"Bob\", \"scores\": [95, 87, 92]}, \"active\": true}"))
(display "Nested structure: ")
(display nested-json)
(newline)

;; Example 5: Serialize Elle values to JSON
(display "\n=== Example 5: Serializing to JSON ===")
(newline)

(var serialized-nil (json-serialize nil))
(display "Serialized nil: ")
(display serialized-nil)
(newline)

(var serialized-bool (json-serialize true))
(display "Serialized true: ")
(display serialized-bool)
(newline)
(assert-eq serialized-bool "true" "json-serialize true returns 'true'")

(var serialized-int (json-serialize 42))
(display "Serialized 42: ")
(display serialized-int)
(newline)
(assert-eq serialized-int "42" "json-serialize 42 returns '42'")

(var serialized-float (json-serialize 3.14))
(display "Serialized 3.14: ")
(display serialized-float)
(newline)
(assert-eq serialized-float "3.14" "json-serialize 3.14 returns '3.14'")

(var serialized-string (json-serialize "hello"))
(display "Serialized string: ")
(display serialized-string)
(newline)
(assert-eq serialized-string "\"hello\"" "json-serialize string returns quoted string")

;; Example 5b: Elle Booleans vs JSON Booleans
(display "\n=== Example 5b: Elle Booleans vs JSON Booleans ===")
(newline)

;; Test Elle native booleans with JSON operations
(display "Testing Elle native booleans (#t, false):")
(newline)

(var elle-true true)
(var elle-false false)

(var serialized-elle-true (json-serialize elle-true))
(display "Serialized Elle #t: ")
(display serialized-elle-true)
(newline)
(assert-eq serialized-elle-true "true" "Elle bool true serializes to JSON 'true'")

(var serialized-elle-false (json-serialize elle-false))
(display "Serialized Elle #f: ")
(display serialized-elle-false)
(newline)
(assert-eq serialized-elle-false "false" "Elle bool false serializes to JSON 'false'")

;; Test JSON-parsed booleans
(display "\nTesting JSON-parsed booleans:")
(newline)

(var json-true (json-parse "true"))
(var json-false (json-parse "false"))

(display "Parsed JSON true: ")
(display json-true)
(newline)
(assert-true (eq? json-true (json-parse "true")) "JSON-parsed true values are equal")

(display "Parsed JSON false: ")
(display json-false)
(newline)
(assert-true (eq? json-false (json-parse "false")) "JSON-parsed false values are equal")

;; Test round-trip: parse -> serialize
(display "\nTesting round-trip (parse -> serialize):")
(newline)

(var roundtrip-true (json-serialize (json-parse "true")))
(display "Round-trip true: ")
(display roundtrip-true)
(newline)
(assert-eq roundtrip-true "true" "JSON true round-trips correctly")

(var roundtrip-false (json-serialize (json-parse "false")))
(display "Round-trip false: ")
(display roundtrip-false)
(newline)
(assert-eq roundtrip-false "false" "JSON false round-trips correctly")

;; Test interoperability: Elle bools and JSON-parsed bools serialize identically
(display "\nTesting interoperability (Elle bools vs JSON-parsed bools):")
(newline)

(var mixed-list-bools (list elle-true json-false elle-false json-true))
(var serialized-mixed-bools (json-serialize mixed-list-bools))
(display "Mixed list (Elle and JSON bools): ")
(display serialized-mixed-bools)
(newline)
(assert-eq serialized-mixed-bools "[true,false,false,true]" "Mixed Elle and JSON bools serialize identically")

;; Test that Elle true and JSON-parsed true serialize the same way
(var elle-true-serialized (json-serialize true))
(var json-true-serialized (json-serialize (json-parse "true")))
(display "Elle true serialized: ")
(display elle-true-serialized)
(newline)
(display "JSON true serialized: ")
(display json-true-serialized)
(newline)
(assert-eq elle-true-serialized json-true-serialized "Elle true and JSON true serialize identically")

;; Test that Elle false and JSON-parsed false serialize the same way
(var elle-false-serialized (json-serialize false))
(var json-false-serialized (json-serialize (json-parse "false")))
(display "Elle false serialized: ")
(display elle-false-serialized)
(newline)
(display "JSON false serialized: ")
(display json-false-serialized)
(newline)
(assert-eq elle-false-serialized json-false-serialized "Elle false and JSON false serialize identically")

;; Example 6: Serialize lists as arrays
(display "\n=== Example 6: Serializing Lists ===")
(newline)

(var my-list (list 1 2 3 4 5))
(var serialized-list (json-serialize my-list))
(display "Serialized list: ")
(display serialized-list)
(newline)
(assert-eq serialized-list "[1,2,3,4,5]" "json-serialize list returns JSON array")

(var mixed-list (list 1 "two" true nil 3.14))
(var serialized-mixed (json-serialize mixed-list))
(display "Serialized mixed list: ")
(display serialized-mixed)
(newline)
(assert-eq serialized-mixed "[1,\"two\",true,null,3.14]" "json-serialize mixed list returns correct JSON")

;; Example 7: Serialize tables as objects
(display "\n=== Example 7: Serializing Tables ===")
(newline)

(var my-table (table))
(put my-table "name" "Charlie")
(put my-table "age" 25)
(put my-table "active" true)

(var serialized-table (json-serialize my-table))
(display "Serialized table: ")
(display serialized-table)
(newline)

;; Example 8: Pretty-printing JSON
(display "\n=== Example 8: Pretty-Printing JSON ===")
(newline)

(var pretty-json (json-serialize-pretty my-table))
(display "Pretty-printed table:")
(newline)
(display pretty-json)
(newline)

;; Example 9: Round-trip (parse -> modify -> serialize)
(display "\n=== Example 9: Round-trip Transformation ===")
(newline)

(var original-json "{\"product\": \"Widget\", \"price\": 19.99, \"in_stock\": true}")
(display "Original JSON: ")
(display original-json)
(newline)

(var parsed (json-parse original-json))
(display "Parsed: ")
(display parsed)
(newline)

;; Modify the parsed data
(put parsed "price" 24.99)
(put parsed "discount" 0.1)

(var modified-json (json-serialize parsed))
(display "Modified JSON: ")
(display modified-json)
(newline)

;; Pretty print the modified data
(var pretty-modified (json-serialize-pretty parsed))
(display "Pretty-printed modified:")
(newline)
(display pretty-modified)
(newline)

;; Example 10: Building a config object
(display "\n=== Example 10: Building a Config Object ===")
(newline)

(var config (table))
(put config "app_name" "MyApp")
(put config "version" "1.0.0")
(put config "debug" false)

(var settings (table))
(put settings "timeout" 30)
(put settings "retries" 3)
(put settings "verbose" true)

(put config "settings" settings)

(var config-json (json-serialize-pretty config))
(display "Configuration:")
(newline)
(display config-json)
(newline)

;; Example 11: Working with arrays of objects
(display "\n=== Example 11: Arrays of Objects ===")
(newline)

(var users-json "[{\"id\": 1, \"name\": \"Alice\"}, {\"id\": 2, \"name\": \"Bob\"}]")
(var users (json-parse users-json))
(display "Parsed users: ")
(display users)
(newline)

(var pretty-users (json-serialize-pretty users))
(display "Pretty users:")
(newline)
(display pretty-users)
(newline)

(display "\n=== All Examples Complete ===")
(newline)
