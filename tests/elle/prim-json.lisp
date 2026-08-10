(elle/epoch 12)
## tests/elle/prim-json.lisp
## JSON parsing, serialization, roundtrip, error handling

## ── json/parse basics ──────────────────────────────────────────────

(assert (nil? (json/parse "null")) "parse null")
(assert (= (json/parse "true") true) "parse true")
(assert (= (json/parse "false") false) "parse false")
(assert (= (json/parse "0") 0) "parse zero")
(assert (= (json/parse "42") 42) "parse positive int")
(assert (= (json/parse "-17") -17) "parse negative int")
(assert (= (json/parse "\"hello\"") "hello") "parse string")
(assert (= (json/parse "\"\"") "") "parse empty string")
(assert (= (json/parse "\"hello\\nworld\"") "hello\nworld")
        "parse string escape-n")
(assert (= (json/parse "\"\\u0041\"") "A") "parse unicode escape")

## ── json/parse numbers ─────────────────────────────────────────────

(assert (float? (json/parse "3.14")) "parse float is float")
(assert (float? (json/parse "1e10")) "parse scientific is float")
(assert (float? (json/parse "1.0")) "parse 1.0 is float")

## ── json/parse collections ─────────────────────────────────────────

## A JSON array parses to an immutable Elle array, not a list: `()` and
## `[]` are different values and do not compare equal.
(assert (array? (json/parse "[]")) "parse empty array is array")
(assert (= (json/parse "[]") []) "parse empty array")
(assert (not (= (json/parse "[]") ())) "parse empty array is not a list")
(assert (array? (json/parse "[1,2,3]")) "parse array is array")
(assert (= (json/parse "[1,2,3]") [1 2 3]) "parse array elements")
(let [v (json/parse "[1,2,3]")]
  (assert (= (length v) 3) "parse array length")
  (assert (= (first v) 1) "parse array first")
  (assert (= (get v 1) 2) "parse array index"))
(let [v (json/parse "[1,\"two\",true,null]")]
  (assert (= (length v) 4) "parse mixed array length")
  (assert (= v [1 "two" true nil]) "parse mixed array elements"))

## ── json/parse objects ─────────────────────────────────────────────

(let [v (json/parse "{}")]
  (assert (struct? v) "parse empty object is struct"))

(let [v (json/parse "{\"name\":\"Alice\",\"age\":30}")]
  (assert (= (get v "name") "Alice") "parse object string key")
  (assert (= (get v "age") 30) "parse object int value"))

## ── json/parse returns an immutable value ──────────────────────────

(assert (immutable? (json/parse "[]")) "parsed empty array is immutable")
(assert (immutable? (json/parse "[1,2,3]")) "parsed array is immutable")
(assert (not (mutable? (json/parse "[1,2,3]"))) "parsed array is not mutable")
(assert (immutable? (json/parse "{}")) "parsed empty object is immutable")
(assert (immutable? (json/parse "{\"a\":1}")) "parsed object is immutable")
(assert (not (mutable? (json/parse "{\"a\":1}"))) "parsed object is not mutable")

## Immutability reaches every depth, through arrays and objects alike.
(let [v (json/parse "{\"a\": [1, {\"b\": [2]}]}")]
  (assert (immutable? v) "parsed root is immutable")
  (assert (immutable? (get v "a")) "parsed nested array is immutable")
  (assert (immutable? (get (get v "a") 1)) "parsed nested object is immutable")
  (assert (immutable? (get (get (get v "a") 1) "b"))
          "parsed deep nested array is immutable"))

(let [v (json/parse "{\"a\": {\"b\": [1]}}" :keys :keyword)]
  (assert (immutable? v) "keyword-key root is immutable")
  (assert (immutable? (get v :a)) "keyword-key nested object is immutable")
  (assert (immutable? (get (get v :a) :b))
          "keyword-key nested array is immutable"))

## `put` and `del` build a new value and leave the parsed value intact, so
## a caller can remove a key and still read the value it removed.
(let [v (json/parse "{\"a\":1}")]
  (assert (= (get (put v "b" 2) "b") 2)
          "put on parsed object returns a new value")
  (assert (nil? (get v "b")) "put leaves the parsed object unchanged"))

(let [v (json/parse "{\"a\":1,\"b\":2}")]
  (assert (nil? (get (del v "a") "a"))
          "del on parsed object returns a new value")
  (assert (= (get v "a") 1) "del leaves the parsed object unchanged"))

(let [v (json/parse "[1,2,3]")]
  (assert (= (put v 0 99) [99 2 3]) "put on parsed array returns a new value")
  (assert (= v [1 2 3]) "put leaves the parsed array unchanged"))

## `thaw` is the way to a mutable copy.
(let [v (json/parse "{\"a\":1}")
      m (thaw v)]
  (put m "b" 2)
  (assert (mutable? m) "thawed copy is mutable")
  (assert (= (get m "b") 2) "thawed copy takes the put")
  (assert (nil? (get v "b")) "thaw leaves the parsed object unchanged"))

## ── json/parse errors ──────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (json/parse ""))))]
  (assert (not ok?) "parse empty string errors"))
(let [[ok? _] (protect ((fn [] (json/parse "42 extra"))))]
  (assert (not ok?) "parse trailing content errors"))
(let [[ok? _] (protect ((fn [] (json/parse "invalid"))))]
  (assert (not ok?) "parse invalid token errors"))
(let [[ok? _] (protect ((fn [] (json/parse 42))))]
  (assert (not ok?) "parse non-string arg errors"))

## ── json/parse leading zeros ───────────────────────────────────────

(let [[ok? _] (protect ((fn [] (json/parse "01"))))]
  (assert (not ok?) "parse leading zero errors"))
(assert (= (json/parse "0") 0) "parse lone zero ok")

## ── json/parse trailing comma ──────────────────────────────────────

(let [[ok? _] (protect ((fn [] (json/parse "[1,2,]"))))]
  (assert (not ok?) "parse trailing comma in array errors"))
(let [[ok? _] (protect ((fn [] (json/parse "{\"a\":1,}"))))]
  (assert (not ok?) "parse trailing comma in object errors"))

## ── json/parse surrogates ──────────────────────────────────────────

(assert (= (json/parse "\"\\uD83D\\uDE00\"") "😀") "parse surrogate pair")
(let [[ok? _] (protect ((fn [] (json/parse "\"\\uD800\""))))]
  (assert (not ok?) "parse lone high surrogate errors"))

## ── json/serialize ─────────────────────────────────────────────────

(assert (= (json/serialize nil) "null") "serialize null")
(assert (= (json/serialize true) "true") "serialize true")
(assert (= (json/serialize false) "false") "serialize false")
(assert (= (json/serialize 42) "42") "serialize int")
(assert (= (json/serialize "hello") "\"hello\"") "serialize string")

## ── json/serialize string escaping ─────────────────────────────────

(assert (= (json/serialize "hello\"world") "\"hello\\\"world\"")
        "serialize quote escape")
(assert (= (json/serialize "hello\\world") "\"hello\\\\world\"")
        "serialize backslash escape")
(assert (= (json/serialize "hello\nworld") "\"hello\\nworld\"")
        "serialize newline escape")
(assert (= (json/serialize "hello\tworld") "\"hello\\tworld\"")
        "serialize tab escape")

## ── json/serialize collections ─────────────────────────────────────

(assert (= (json/serialize (list 1 2 3)) "[1,2,3]") "serialize list")
(assert (= (json/serialize [1 2 3]) "[1,2,3]") "serialize immutable array")
(assert (= (json/serialize @[1 2 3]) "[1,2,3]") "serialize mutable array")
(assert (= (json/serialize (json/parse "[1,[2],{\"a\":3}]")) "[1,[2],{\"a\":3}]")
        "serialize a parsed value")

## ── json/serialize NaN/Infinity ────────────────────────────────────

(let [[ok? _] (protect ((fn [] (json/serialize (/ 0.0 0.0)))))]
  (assert (not ok?) "serialize NaN errors"))
(let [[ok? _] (protect ((fn [] (json/serialize (/ 1.0 0.0)))))]
  (assert (not ok?) "serialize Infinity errors"))

## ── json/serialize keyword ─────────────────────────────────────────

(assert (= (json/serialize :hello) "\"hello\"") "serialize keyword as string")

## ── json roundtrip ─────────────────────────────────────────────────

## A list serializes to a JSON array, and that array parses back to an
## immutable Elle array — so the roundtrip lands on `[...]`, not on the
## list it started from.
(let [original (list 1 "test" true nil)]
  (assert (= (json/parse (json/serialize original)) [1 "test" true nil])
          "json list roundtrip lands on an array"))

(let [original [1 "test" true nil]]
  (assert (= (json/parse (json/serialize original)) original)
          "json array roundtrip"))

(let [original {"a" 1 "b" [2 3] "c" {"d" true}}]
  (assert (= (json/parse (json/serialize original)) original)
          "json object roundtrip"))

## ── json/parse :keys :keyword ──────────────────────────────────────

(let [v (json/parse "{\"a\": 1}" :keys :keyword)]
  (assert (= (get v :a) 1) "parse keyword keys simple"))

(let [v (json/parse "{\"a\": {\"b\": 2}}" :keys :keyword)]
  (assert (= (get (get v :a) :b) 2) "parse keyword keys nested"))

(let [v (json/parse "{\"a\": 1}")]
  (assert (= (get v "a") 1) "parse default string keys"))

(let [[ok? _] (protect ((fn [] (json/parse "{}" :keys :wrong))))]
  (assert (not ok?) "parse wrong keys option errors"))

(let [[ok? _] (protect ((fn [] (json/parse "{}" :wrong :keyword))))]
  (assert (not ok?) "parse wrong option key errors"))

(let [[ok? _] (protect ((fn [] (json/parse "{}" :keys))))]
  (assert (not ok?) "parse 2 args arity error"))

(let [v (json/parse "[]" :keys :keyword)]
  (assert (= v []) "parse array keyword keys unaffected"))

(println "prim-json: all tests passed")
