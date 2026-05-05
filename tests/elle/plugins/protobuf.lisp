(elle/epoch 10)

## protobuf plugin integration tests

## Try to load the protobuf plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_protobuf.so")))
(when (not ok?)
  (print "SKIP: protobuf plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def schema-fn (get plugin :schema))
(def encode-fn (get plugin :encode))
(def decode-fn (get plugin :decode))
(def messages-fn (get plugin :messages))
(def fields-fn (get plugin :fields))
(def enums-fn (get plugin :enums))

## ── Schema definition ────────────────────────────────────────────────

## Full schema with enum, simple message, nested message, map field
(def test-proto
  "
syntax = \"proto3\";

enum Status {
  UNKNOWN = 0;
  OK = 1;
  ERROR = 2;
}

message Person {
  string name = 1;
  int32 age = 2;
  repeated string tags = 3;
  Status status = 4;
  map<string, int32> scores = 5;
}

message Team {
  string team_name = 1;
  repeated Person members = 2;
}
")

(def pool (schema-fn test-proto))

## ── protobuf/messages ────────────────────────────────────────────────

(def msgs (messages-fn pool))

(assert (array? msgs) "protobuf/messages returns array")

## The pool should contain Person and Team (Status is an enum, not a message)
(assert (> (length msgs) 0) "protobuf/messages returns non-empty array")

## Check that Person and Team are present
(def has-person
  (let [found false]
    (letrec [check (fn (i)
                     (if (>= i (length msgs))
                       found
                       (if (= (get msgs i) "Person") true (check (+ i 1)))))]
      (check 0))))

(def has-team
  (let [found false]
    (letrec [check (fn (i)
                     (if (>= i (length msgs))
                       found
                       (if (= (get msgs i) "Team") true (check (+ i 1)))))]
      (check 0))))

(assert has-person "protobuf/messages includes Person")
(assert has-team "protobuf/messages includes Team")

## ── protobuf/fields ─────────────────────────────────────────────────

(def person-fields (fields-fn pool "Person"))

(assert (array? person-fields) "protobuf/fields returns array")

(assert (= (length person-fields) 5) "Person has 5 fields")

## Find a field by name in the fields array
(def find-field
  (fn (fields name)
    (letrec [search (fn (i)
                      (if (>= i (length fields))
                        nil
                        (if (= (get (get fields i) :name) name)
                          (get fields i)
                          (search (+ i 1)))))]
      (search 0))))

(def f-name (find-field person-fields "name"))
(def f-age (find-field person-fields "age"))
(def f-tags (find-field person-fields "tags"))
(def f-status (find-field person-fields "status"))
(def f-scores (find-field person-fields "scores"))

(assert (not (nil? f-name)) "Person has field 'name'")
(assert (not (nil? f-age)) "Person has field 'age'")
(assert (not (nil? f-tags)) "Person has field 'tags'")
(assert (not (nil? f-status)) "Person has field 'status'")
(assert (not (nil? f-scores)) "Person has field 'scores'")

(assert (= (get f-name :type) :string) "name field type is string")
(assert (= (get f-age :type) :int32) "age field type is int32")
(assert (= (get f-tags :label) :repeated) "tags field label is repeated")
(assert (= (get f-status :type) :enum) "status field type is enum")
(assert (= (get f-scores :type) :message)
        "scores field type is message (map entry)")

(assert (= (get f-name :number) 1) "name field number is 1")
(assert (= (get f-age :number) 2) "age field number is 2")
(assert (= (get f-tags :number) 3) "tags field number is 3")

## ── protobuf/enums ──────────────────────────────────────────────────

(def enums (enums-fn pool))

(assert (array? enums) "protobuf/enums returns array")

(assert (> (length enums) 0) "protobuf/enums returns non-empty result")

## Find the Status enum
(def find-enum
  (fn (enums name)
    (letrec [search (fn (i)
                      (if (>= i (length enums))
                        nil
                        (if (= (get (get enums i) :name) name)
                          (get enums i)
                          (search (+ i 1)))))]
      (search 0))))

(def status-enum (find-enum enums "Status"))

(assert (not (nil? status-enum)) "protobuf/enums includes Status")

(def status-values (get status-enum :values))

(assert (= (length status-values) 3) "Status has 3 values")

## Find enum value by name
(def find-enum-val
  (fn (values name)
    (letrec [search (fn (i)
                      (if (>= i (length values))
                        nil
                        (if (= (get (get values i) :name) name)
                          (get values i)
                          (search (+ i 1)))))]
      (search 0))))

(def v-unknown (find-enum-val status-values "UNKNOWN"))
(def v-ok (find-enum-val status-values "OK"))
(def v-error (find-enum-val status-values "ERROR"))

(assert (not (nil? v-unknown)) "Status has UNKNOWN value")
(assert (not (nil? v-ok)) "Status has OK value")
(assert (not (nil? v-error)) "Status has ERROR value")

(assert (= (get v-unknown :number) 0) "UNKNOWN = 0")
(assert (= (get v-ok :number) 1) "OK = 1")
(assert (= (get v-error :number) 2) "ERROR = 2")

## ── Round-trip: simple Person ───────────────────────────────────────

(def alice {:name "Alice" :age 30 :tags ["dev" "lisp"]})
(def alice-buf (encode-fn pool "Person" alice))

(assert (bytes? alice-buf) "protobuf/encode returns bytes")

(assert (> (length alice-buf) 0) "encoded bytes are non-empty")

(def alice-decoded (decode-fn pool "Person" alice-buf))

(assert (struct? alice-decoded) "protobuf/decode returns struct")

(assert (= (get alice-decoded :name) "Alice") "Person round-trip: name")

(assert (= (get alice-decoded :age) 30) "Person round-trip: age")

(assert (= (length (get alice-decoded :tags)) 2)
        "Person round-trip: tags length")

(assert (= (get (get alice-decoded :tags) 0) "dev") "Person round-trip: tags[0]")

(assert (= (get (get alice-decoded :tags) 1) "lisp")
        "Person round-trip: tags[1]")

## ── Round-trip: Team with nested Persons ────────────────────────────

(def bob {:name "Bob" :age 25 :tags ["ops"]})
(def carol {:name "Carol" :age 28 :tags ["ml" "python"]})

(def team {:team_name "Alpha" :members [alice bob carol]})
(def team-buf (encode-fn pool "Team" team))

(assert (bytes? team-buf) "Team encode returns bytes")

(def team-decoded (decode-fn pool "Team" team-buf))

(assert (= (get team-decoded :team_name) "Alpha") "Team round-trip: team_name")

(def members (get team-decoded :members))

(assert (= (length members) 3) "Team round-trip: 3 members")

(assert (= (get (get members 0) :name) "Alice")
        "Team round-trip: member[0].name")

(assert (= (get (get members 1) :name) "Bob") "Team round-trip: member[1].name")

(assert (= (get (get members 2) :name) "Carol")
        "Team round-trip: member[2].name")

(assert (= (length (get (get members 2) :tags)) 2)
        "Team round-trip: member[2].tags length")

## ── Enum fields round-trip as keywords ──────────────────────────────

(def person-ok {:name "Dave" :status :OK})
(def person-ok-decoded
  (decode-fn pool "Person" (encode-fn pool "Person" person-ok)))

(assert (= (get person-ok-decoded :status) :OK)
        "enum field :OK round-trips as keyword")

(def person-error {:name "Eve" :status :ERROR})
(def person-error-decoded
  (decode-fn pool "Person" (encode-fn pool "Person" person-error)))

(assert (= (get person-error-decoded :status) :ERROR)
        "enum field :ERROR round-trips as keyword")

## ── Map fields round-trip correctly ─────────────────────────────────

(def person-scores {:name "Frank" :scores {:math 95 :science 88 :history 72}})
(def scores-buf (encode-fn pool "Person" person-scores))
(def scores-decoded (decode-fn pool "Person" scores-buf))

(def scores (get scores-decoded :scores))

(assert (struct? scores) "map field decodes as struct")

(assert (= (get scores :math) 95) "map field round-trip: math = 95")

(assert (= (get scores :science) 88) "map field round-trip: science = 88")

(assert (= (get scores :history) 72) "map field round-trip: history = 72")

## ── Error: unknown message name ─────────────────────────────────────

(let [[ok? err] (protect ((fn () (encode-fn pool "NoSuchMessage" {:x 1}))))]
  (assert (not ok?) "encode with unknown message name gives protobuf-error")
  (assert (= (get err :error) :protobuf-error)
          "encode with unknown message name gives protobuf-error"))

(let [[ok? err] (protect ((fn () (decode-fn pool "NoSuchMessage" (bytes 0)))))]
  (assert (not ok?) "decode with unknown message name gives protobuf-error")
  (assert (= (get err :error) :protobuf-error)
          "decode with unknown message name gives protobuf-error"))

(let [[ok? err] (protect ((fn () (fields-fn pool "NoSuchMessage"))))]
  (assert (not ok?) "fields with unknown message name gives protobuf-error")
  (assert (= (get err :error) :protobuf-error)
          "fields with unknown message name gives protobuf-error"))

## ── Error: wrong types ───────────────────────────────────────────────

(let [[ok? err] (protect ((fn () (schema-fn 42))))]
  (assert (not ok?) "protobuf/schema non-string gives type-error")
  (assert (= (get err :error) :type-error)
          "protobuf/schema non-string gives type-error"))

(let [[ok? err] (protect ((fn () (encode-fn pool "Person" "not a struct"))))]
  (assert (not ok?) "encode non-struct gives type-error")
  (assert (= (get err :error) :type-error) "encode non-struct gives type-error"))

(let [[ok? err] (protect ((fn () (decode-fn pool "Person" "not bytes"))))]
  (assert (not ok?) "decode non-bytes gives type-error")
  (assert (= (get err :error) :type-error) "decode non-bytes gives type-error"))

(let [[ok? err] (protect ((fn () (messages-fn 42))))]
  (assert (not ok?) "messages with non-pool gives type-error")
  (assert (= (get err :error) :type-error)
          "messages with non-pool gives type-error"))
