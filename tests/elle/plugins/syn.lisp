(elle/epoch 1)

## syn plugin integration tests

(def [ok? plugin] (protect (import-file "target/release/libelle_syn.so")))
(when (not ok?)
  (display "SKIP: syn plugin not built\n")
  (exit 0))

(def parse-file-fn    (get plugin :parse-file))
(def parse-expr-fn    (get plugin :parse-expr))
(def parse-type-fn    (get plugin :parse-type))
(def parse-item-fn    (get plugin :parse-item))
(def items-fn         (get plugin :items))
(def item-kind-fn     (get plugin :item-kind))
(def item-name-fn     (get plugin :item-name))
(def fn-info-fn       (get plugin :fn-info))
(def fn-args-fn       (get plugin :fn-args))
(def fn-return-type-fn (get plugin :fn-return-type))
(def struct-fields-fn (get plugin :struct-fields))
(def enum-variants-fn (get plugin :enum-variants))
(def attributes-fn    (get plugin :attributes))
(def visibility-fn    (get plugin :visibility))
(def to-string-fn     (get plugin :to-string))
(def to-pretty-string-fn (get plugin :to-pretty-string))

## Shared test source (parse once, reuse across tests)
(def source "pub fn add(x: i32, y: i32) -> i32 { x + y }
async fn fetch() {}
unsafe fn danger() {}
#[derive(Debug)]
pub struct Point { x: f64, y: f64 }
struct Unit;
struct Pair(i32, String);
pub(crate) enum Color { Red, Green = 1, Blue }
impl Point { fn new() -> Self { Point { x: 0.0, y: 0.0 } } }
")

(def file (parse-file-fn source))
(def items (items-fn file))

# ── syn/parse-file ─────────────────────────────────────────────────

## Parse a valid Rust source file
(assert (not (nil? file)) "parse-file: returns non-nil for valid source")

## Error: invalid Rust source returns parse-error
(let (([ok? err] (protect ((fn () (parse-file-fn "fn broken( {}")))))) (assert (not ok?) "parse-file: invalid Rust returns parse-error") (assert (= (get err :error) :parse-error) "parse-file: invalid Rust returns parse-error"))

## Error: non-string argument returns type-error
(let (([ok? err] (protect ((fn () (parse-file-fn 42)))))) (assert (not ok?) "parse-file: non-string returns type-error") (assert (= (get err :error) :type-error) "parse-file: non-string returns type-error"))

# ── syn/parse-expr ─────────────────────────────────────────────────

## Parse a valid Rust expression
(assert (not (nil? (parse-expr-fn "1 + 2"))) "parse-expr: returns non-nil for valid expr")

## Error: invalid expression returns parse-error
(let (([ok? err] (protect ((fn () (parse-expr-fn "fn")))))) (assert (not ok?) "parse-expr: invalid expr returns parse-error") (assert (= (get err :error) :parse-error) "parse-expr: invalid expr returns parse-error"))

# ── syn/parse-type ─────────────────────────────────────────────────

## Parse a valid Rust type
(assert (not (nil? (parse-type-fn "Vec<String>"))) "parse-type: returns non-nil for valid type")

# ── syn/parse-item ─────────────────────────────────────────────────

## Parse a valid Rust item
(assert (not (nil? (parse-item-fn "fn foo() {}"))) "parse-item: returns non-nil for valid item")

# ── syn/items ──────────────────────────────────────────────────────

## File with 8 items returns list of length 8
(assert (= (length items) 8) "items: 8-item source yields list of 8")

## Error: non-file argument returns type-error
(let (([ok? err] (protect ((fn () (items-fn "not-a-file")))))) (assert (not ok?) "items: non-file returns type-error") (assert (= (get err :error) :type-error) "items: non-file returns type-error"))

# ── syn/item-kind ──────────────────────────────────────────────────

## item-kind on fn item returns :fn
(assert (= (item-kind-fn (get items 0)) :fn) "item-kind: fn item returns :fn")

## item-kind on struct item returns :struct
(assert (= (item-kind-fn (get items 3)) :struct) "item-kind: struct item returns :struct")

## item-kind on enum item returns :enum
(assert (= (item-kind-fn (get items 6)) :enum) "item-kind: enum item returns :enum")

## item-kind on impl item returns :impl
(assert (= (item-kind-fn (get items 7)) :impl) "item-kind: impl item returns :impl")

## Error: non-item argument returns type-error
(let (([ok? err] (protect ((fn () (item-kind-fn "not-an-item")))))) (assert (not ok?) "item-kind: non-item returns type-error") (assert (= (get err :error) :type-error) "item-kind: non-item returns type-error"))

# ── syn/item-name ──────────────────────────────────────────────────

## item-name on fn item returns "add"
(assert (= (item-name-fn (get items 0)) "add") "item-name: fn item returns name")

## item-name on struct item returns "Point"
(assert (= (item-name-fn (get items 3)) "Point") "item-name: struct item returns name")

## item-name on impl item returns nil (impl blocks have no ident)
(assert (= (item-name-fn (get items 7)) nil) "item-name: impl item returns nil")

# ── syn/fn-info ────────────────────────────────────────────────────

## fn-info on add returns expected struct
(let ((info (fn-info-fn (get items 0))))
  (assert (= (get info :name) "add") "fn-info: :name is add")
  (assert (not (get info :async?)) "fn-info: :async? is false for add")
  (assert (not (get info :unsafe?)) "fn-info: :unsafe? is false for add")
  (assert (not (get info :const?)) "fn-info: :const? is false for add"))

## fn-info on async fn has :async? true
(let ((info (fn-info-fn (get items 1))))
  (assert (get info :async?) "fn-info: :async? is true for async fn"))

## fn-info on unsafe fn has :unsafe? true
(let ((info (fn-info-fn (get items 2))))
  (assert (get info :unsafe?) "fn-info: :unsafe? is true for unsafe fn"))

## Error: fn-info on struct item returns type-error
(let (([ok? err] (protect ((fn () (fn-info-fn (get items 3))))))) (assert (not ok?) "fn-info: struct item returns type-error") (assert (= (get err :error) :type-error) "fn-info: struct item returns type-error"))

# ── syn/fn-args ────────────────────────────────────────────────────

## fn-args on add returns two arguments with correct names and types
(let ((args (fn-args-fn (get items 0))))
  (assert (= (length args) 2) "fn-args: add has 2 args")
  (let ((first-arg (get args 0)))
    (assert (= (get first-arg :name) "x") "fn-args: first arg name is x")
    (assert (= (get first-arg :type) "i32") "fn-args: first arg type is i32")))

## fn-args on fetch returns empty list
(assert (= (length (fn-args-fn (get items 1))) 0) "fn-args: fetch has 0 args")

# ── syn/fn-return-type ─────────────────────────────────────────────

## fn-return-type on add returns "i32"
(assert (= (fn-return-type-fn (get items 0)) "i32") "fn-return-type: add returns i32")

## fn-return-type on fetch (no return type) returns nil
(assert (= (fn-return-type-fn (get items 1)) nil) "fn-return-type: fetch returns nil")

# ── syn/struct-fields ──────────────────────────────────────────────

## Named struct: Point has :kind :named with two named fields
(let ((result (struct-fields-fn (get items 3))))
  (assert (= (get result :kind) :named) "struct-fields: Point kind is :named")
  (assert (= (length (get result :fields)) 2) "struct-fields: Point has 2 fields")
  (let ((field (get (get result :fields) 0)))
    (assert (= (get field :name) "x") "struct-fields: first field name is x")
    (assert (= (get field :type) "f64") "struct-fields: first field type is f64")))

## Unit struct: Unit has :kind :unit with empty fields
(let ((result (struct-fields-fn (get items 4))))
  (assert (= (get result :kind) :unit) "struct-fields: Unit kind is :unit")
  (assert (= (length (get result :fields)) 0) "struct-fields: Unit has 0 fields"))

## Tuple struct: Pair has :kind :tuple with nil names
(let ((result (struct-fields-fn (get items 5))))
  (assert (= (get result :kind) :tuple) "struct-fields: Pair kind is :tuple")
  (assert (= (length (get result :fields)) 2) "struct-fields: Pair has 2 fields")
  (let ((field (get (get result :fields) 0)))
    (assert (= (get field :name) nil) "struct-fields: tuple field name is nil")))

## Error: struct-fields on fn item returns type-error
(let (([ok? err] (protect ((fn () (struct-fields-fn (get items 0))))))) (assert (not ok?) "struct-fields: fn item returns type-error") (assert (= (get err :error) :type-error) "struct-fields: fn item returns type-error"))

# ── syn/enum-variants ──────────────────────────────────────────────

## Color enum has 3 variants; Green has :discriminant "1"
(let ((result (enum-variants-fn (get items 6))))
  (assert (= (get result :name) "Color") "enum-variants: name is Color")
  (let ((variants (get result :variants)))
    (assert (= (length variants) 3) "enum-variants: Color has 3 variants")
    (let ((red (get variants 0)))
      (assert (= (get red :name) "Red") "enum-variants: first variant is Red")
      (assert (= (get red :kind) :unit) "enum-variants: Red kind is :unit"))
    (let ((green (get variants 1)))
      (assert (= (get green :name) "Green") "enum-variants: second variant is Green")
      (assert (not (nil? (get green :discriminant))) "enum-variants: Green has discriminant"))))

## Error: enum-variants on fn item returns type-error
(let (([ok? err] (protect ((fn () (enum-variants-fn (get items 0))))))) (assert (not ok?) "enum-variants: fn item returns type-error") (assert (= (get err :error) :type-error) "enum-variants: fn item returns type-error"))

# ── syn/attributes ─────────────────────────────────────────────────

## Point has one #[derive(Debug)] attribute
(let ((attrs (attributes-fn (get items 3))))
  (assert (= (length attrs) 1) "attributes: Point has 1 attribute")
  (assert (not (nil? (get attrs 0))) "attributes: attribute is non-nil string"))

## add has no attributes
(assert (= (length (attributes-fn (get items 0))) 0) "attributes: add has 0 attributes")

# ── syn/visibility ─────────────────────────────────────────────────

## pub fn add has :public visibility
(assert (= (visibility-fn (get items 0)) :public) "visibility: pub fn returns :public")

## async fn fetch (no vis) has :private visibility
(assert (= (visibility-fn (get items 1)) :private) "visibility: private fn returns :private")

## pub(crate) enum Color has :pub-crate visibility
(assert (= (visibility-fn (get items 6)) :pub-crate) "visibility: pub(crate) returns :pub-crate")

# ── syn/to-string ──────────────────────────────────────────────────

## to-string on a parsed item returns a string
(let ((s (to-string-fn (get items 0))))
  (assert (string? s) "to-string: item returns a string")
  (assert (> (length s) 0) "to-string: string is non-empty"))

## to-string on a parsed expr returns a string
(let ((s (to-string-fn (parse-expr-fn "1 + 2"))))
  (assert (string? s) "to-string: expr returns a string"))

## to-string on a parsed file returns a string
(let ((s (to-string-fn file)))
  (assert (string? s) "to-string: file returns a string"))

## Error: to-string on a non-syn value returns type-error
(let (([ok? err] (protect ((fn () (to-string-fn "not-a-node")))))) (assert (not ok?) "to-string: non-syn value returns type-error") (assert (= (get err :error) :type-error) "to-string: non-syn value returns type-error"))

# ── syn/to-pretty-string ───────────────────────────────────────────

## to-pretty-string on a parsed item returns a formatted string
(let ((s (to-pretty-string-fn (get items 0))))
  (assert (string? s) "to-pretty-string: item returns a string")
  (assert (> (length s) 0) "to-pretty-string: string is non-empty"))

## to-pretty-string on a parsed file returns a formatted string
(let ((s (to-pretty-string-fn file)))
  (assert (string? s) "to-pretty-string: file returns a string"))

## Error: to-pretty-string on an expr returns type-error
(let (([ok? err] (protect ((fn () (to-pretty-string-fn (parse-expr-fn "1 + 2"))))))) (assert (not ok?) "to-pretty-string: expr returns type-error") (assert (= (get err :error) :type-error) "to-pretty-string: expr returns type-error"))
