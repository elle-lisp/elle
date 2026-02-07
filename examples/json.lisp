#!/usr/bin/env elle
;; JSON parsing and serialization examples

;; Example 1: Parse various JSON types
(display "=== Example 1: Parsing JSON ===")
(newline)

(define json-null (json-parse "null"))
(display "Parsed null: ")
(display json-null)
(newline)

(define json-bool (json-parse "true"))
(display "Parsed true: ")
(display json-bool)
(newline)

(define json-int (json-parse "42"))
(display "Parsed 42: ")
(display json-int)
(newline)

(define json-float (json-parse "3.14"))
(display "Parsed 3.14: ")
(display json-float)
(newline)

(define json-string (json-parse "\"hello world\""))
(display "Parsed string: ")
(display json-string)
(newline)

;; Example 2: Parse arrays
(display "\n=== Example 2: Parsing Arrays ===")
(newline)

(define json-array (json-parse "[1, 2, 3, 4, 5]"))
(display "Parsed array: ")
(display json-array)
(newline)

(define mixed-array (json-parse "[1, \"two\", true, null, 3.14]"))
(display "Mixed array: ")
(display mixed-array)
(newline)

;; Example 3: Parse objects
(display "\n=== Example 3: Parsing Objects ===")
(newline)

(define json-obj (json-parse "{\"name\": \"Alice\", \"age\": 30, \"active\": true}"))
(display "Parsed object: ")
(display json-obj)
(newline)

;; Access object fields using get
(define name (get json-obj "name"))
(display "Name from object: ")
(display name)
(newline)

(define age (get json-obj "age"))
(display "Age from object: ")
(display age)
(newline)

;; Example 4: Nested structures
(display "\n=== Example 4: Nested Structures ===")
(newline)

(define nested-json (json-parse "{\"user\": {\"name\": \"Bob\", \"scores\": [95, 87, 92]}, \"active\": true}"))
(display "Nested structure: ")
(display nested-json)
(newline)

;; Example 5: Serialize Elle values to JSON
(display "\n=== Example 5: Serializing to JSON ===")
(newline)

(define serialized-nil (json-serialize nil))
(display "Serialized nil: ")
(display serialized-nil)
(newline)

(define serialized-bool (json-serialize #t))
(display "Serialized #t: ")
(display serialized-bool)
(newline)

(define serialized-int (json-serialize 42))
(display "Serialized 42: ")
(display serialized-int)
(newline)

(define serialized-float (json-serialize 3.14))
(display "Serialized 3.14: ")
(display serialized-float)
(newline)

(define serialized-string (json-serialize "hello"))
(display "Serialized string: ")
(display serialized-string)
(newline)

;; Example 6: Serialize lists as arrays
(display "\n=== Example 6: Serializing Lists ===")
(newline)

(define my-list (list 1 2 3 4 5))
(define serialized-list (json-serialize my-list))
(display "Serialized list: ")
(display serialized-list)
(newline)

(define mixed-list (list 1 "two" #t nil 3.14))
(define serialized-mixed (json-serialize mixed-list))
(display "Serialized mixed list: ")
(display serialized-mixed)
(newline)

;; Example 7: Serialize tables as objects
(display "\n=== Example 7: Serializing Tables ===")
(newline)

(define my-table (table))
(put my-table "name" "Charlie")
(put my-table "age" 25)
(put my-table "active" #t)

(define serialized-table (json-serialize my-table))
(display "Serialized table: ")
(display serialized-table)
(newline)

;; Example 8: Pretty-printing JSON
(display "\n=== Example 8: Pretty-Printing JSON ===")
(newline)

(define pretty-json (json-serialize-pretty my-table))
(display "Pretty-printed table:")
(newline)
(display pretty-json)
(newline)

;; Example 9: Round-trip (parse -> modify -> serialize)
(display "\n=== Example 9: Round-trip Transformation ===")
(newline)

(define original-json "{\"product\": \"Widget\", \"price\": 19.99, \"in_stock\": true}")
(display "Original JSON: ")
(display original-json)
(newline)

(define parsed (json-parse original-json))
(display "Parsed: ")
(display parsed)
(newline)

;; Modify the parsed data
(put parsed "price" 24.99)
(put parsed "discount" 0.1)

(define modified-json (json-serialize parsed))
(display "Modified JSON: ")
(display modified-json)
(newline)

;; Pretty print the modified data
(define pretty-modified (json-serialize-pretty parsed))
(display "Pretty-printed modified:")
(newline)
(display pretty-modified)
(newline)

;; Example 10: Building a config object
(display "\n=== Example 10: Building a Config Object ===")
(newline)

(define config (table))
(put config "app_name" "MyApp")
(put config "version" "1.0.0")
(put config "debug" #f)

(define settings (table))
(put settings "timeout" 30)
(put settings "retries" 3)
(put settings "verbose" #t)

(put config "settings" settings)

(define config-json (json-serialize-pretty config))
(display "Configuration:")
(newline)
(display config-json)
(newline)

;; Example 11: Working with arrays of objects
(display "\n=== Example 11: Arrays of Objects ===")
(newline)

(define users-json "[{\"id\": 1, \"name\": \"Alice\"}, {\"id\": 2, \"name\": \"Bob\"}]")
(define users (json-parse users-json))
(display "Parsed users: ")
(display users)
(newline)

(define pretty-users (json-serialize-pretty users))
(display "Pretty users:")
(newline)
(display pretty-users)
(newline)

(display "\n=== All Examples Complete ===")
(newline)
