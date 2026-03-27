#!/usr/bin/env elle

# Collections — a contact book application
#
# We build a contact book from scratch, exercising every collection type
# and operation the language offers. Contacts are immutable structs, the
# book is a mutable @struct, and we query, format, and export it using
# lists, arrays, arrays, strings, and grapheme-aware text processing.
#
# Demonstrates:
#   Literal syntax    — [array]  @[array]  {struct}  @{struct}  "string"  @"string"
#   Mutability        — @ prefix means mutable; bare means immutable
#   Polymorphic ops   — get, length, empty?, append, concat across types
#   put semantics     — mutates mutable types, copies immutable types
#   Iteration         — each ... in ... (prelude macro)
#   Destructuring     — {:key var} and [a b] patterns in let/def/fn
#   Threading macros  — ->  ->>
#   Splice            — ;expr for spreading arrays into calls
#   Key-value ops     — put, del, keys, values, has?
#   List ops          — cons, first, rest, reverse, take, drop, last, butlast
#   Array mutation    — push, pop, insert, remove
#   String ops        — string/find, string/split, string/join, slice,
#                       string/replace, string/contains?, string/trim,
#                       string/upcase, string/downcase
#   Grapheme clusters — string indexing operates on what humans see as characters
## ── Creating the contact book ──────────────────────────────────────

#
# Bare delimiters are immutable; @-prefixed are mutable.
#   [...]  array      @[...]  @array
#   {...}  struct     @{...}  @struct
#   "..."  string     @"..."  @string

# A contact is an immutable struct — once created, it never changes.
(def alice {:name "Alice" :email "alice@example.com" :tags [:dev :lead]})
(def bob   {:name "Bob"   :email "bob@example.com"   :tags [:dev]})
(def carol {:name "Carol" :email "carol@example.com" :tags [:ops :lead]})
(def dave  {:name "Dave"  :email "dave@example.com"  :tags [:ops :dev]})

# The book is a mutable @struct — entries come and go.
(def book @{})

# put on a mutable @struct mutates it in place and returns the same object.
(def same-book (put book "alice" alice))
(assert (= same-book book) "put on @struct: same object returned")

# put on an immutable struct returns a NEW struct — original unchanged.
(def alice-with-phone (put alice :phone "555-0100"))
(assert (not (has? alice :phone)) "put on struct: original unchanged")
(assert (= (get alice-with-phone :phone) "555-0100") "put on struct: new has key")

# Populate the book
(put book "bob"   bob)
(put book "carol" carol)
(put book "dave"  dave)
(assert (= (length (keys book)) 4) "book has 4 contacts")
(println "  book has " (length (keys book)) " contacts")
## ── Accessing contacts — polymorphic get ───────────────────────────

#
# get works on every collection type with the same interface.

# By string key (@struct)
(assert (= (get (get book "alice") :name) "Alice") "get: @struct → struct")

# By keyword (struct)
(assert (= (get alice :email) "alice@example.com") "get: struct by keyword")

# By index (@array)
(assert (= (get (get alice :tags) 0) :dev) "get: array by index")

# By index (string — returns a grapheme cluster)
(assert (= (get "hello" 0) "h") "get: string by index")

# get with a default for missing keys
(assert (= (get book "eve" :not-found) :not-found) "get: missing → default")

# Drill into nested data with thread-first
(def alice-email
  (-> book (get "alice") (get :email)))
(assert (= alice-email "alice@example.com") "->: nested access")
(println "  alice email: " alice-email)
## ── Destructuring contacts ─────────────────────────────────────────

#
# Structs destructure by key; arrays by position.

# Unpack a contact's fields
(def {:name aname :email aemail :tags atags}
  (get book "alice"))
(assert (= aname "Alice") "struct destructure: name")
(assert (= aemail "alice@example.com") "struct destructure: email")
(println "  alice → " aname " <" aemail ">")

# Unpack tags by position
(def [first-tag second-tag] atags)
(assert (= first-tag :dev) "array destructure: first tag")
(assert (= second-tag :lead) "array destructure: second tag")

# Nested destructuring in let: struct containing an array
(let ([{:name n :tags [t & _]} (get book "carol")])
  (assert (= n "Carol") "let destructure: name")
  (assert (= t :ops) "let destructure: first tag")
  (println "  carol →" n "first-tag=" t))

# Destructuring in function params
(defn contact-line [{:name name :email email}]
  "Format a contact as Name <email>."
  (-> name (append " <") (append email) (append ">")))

(assert (= (contact-line alice) "Alice <alice@example.com>") "fn param destructure")
## ── Lists — collecting keys and contacts ───────────────────────────

#
# Lists are cons cells. Ideal for accumulation and recursion.

(def names (keys book))
(assert (list? names) "keys returns a list")
(assert (= (length names) 4) "four names")

# cons prepends, first/rest decompose
(def with-eve (cons "eve" names))
(assert (= (first with-eve) "eve") "cons prepends")
(assert (= (length with-eve) 5) "cons adds one")

# last, reverse, take, drop, butlast
(assert (= (last names) (first (reverse names))) "last = first of reverse")
(assert (= (length (take 2 names)) 2) "take 2")
(assert (= (length (drop 2 names)) 2) "drop 2")
(assert (= (length (butlast names)) 3) "butlast drops one")
## ── each — iterating the book ──────────────────────────────────────

# Collect all contacts into an array
(var all-contacts @[])
(each k in (keys book)
  (push all-contacts (get book k)))
(assert (= (length all-contacts) 4) "each: collected all contacts")

# Count total tags across all contacts
(var total-tags 0)
(each c in all-contacts
  (assign total-tags (+ total-tags (length (get c :tags)))))
(println "  total tags across all contacts: " total-tags)
(assert (> total-tags 0) "each: summed tag counts")

# each over an array (alice's tags)
(var tag-count 0)
(each t in (get alice :tags)
  (assign tag-count (+ tag-count 1)))
(assert (= tag-count 2) "each over array")

# each over a string (by grapheme cluster)
(var char-count 0)
(each ch in "hello"
  (assign char-count (+ char-count 1)))
(assert (= char-count 5) "each over string")
## ── Querying — finding contacts by tag ─────────────────────────────

(defn has-tag? [contact tag]
  "Check whether a contact has a given tag."
  (var found false)
  (each t in (get contact :tags)
    (when (= t tag)
      (assign found true)))
  found)

(assert (has-tag? alice :lead) "alice is a lead")
(assert (not (has-tag? bob :lead)) "bob is not a lead")

# Collect leads
(var leads @[])
(each k in (keys book)
  (when (has-tag? (get book k) :lead)
    (push leads k)))
(assert (= (length leads) 2) "two leads found")
(println "  leads: " leads)

# Collect devs
(var devs @[])
(each k in (keys book)
  (when (has-tag? (get book k) :dev)
    (push devs k)))
(assert (= (length devs) 3) "three devs found")
(println "  devs: " devs)
## ── Formatting contacts for display ────────────────────────────────

(defn format-tags [tags]
  "Format a tag array as [dev, lead]."
  (var parts (list))
  (each t in tags
    (assign parts (append parts (list (string t)))))
  (-> "[" (append (string/join parts ", ")) (append "]")))

(defn format-contact [{:name name :email email :tags tags}]
  "Full contact display line."
  (-> name
      (append " <")
      (append email)
      (append "> ")
      (append (format-tags tags))))

(def alice-str (format-contact alice))
(assert (string/contains? alice-str "Alice") "format: name")
(assert (string/contains? alice-str "dev") "format: tag")
(println "  " alice-str)

# Format every contact
(var formatted @[])
(each k in (keys book)
  (push formatted (format-contact (get book k))))
(assert (= (length formatted) 4) "formatted all contacts")
(each line in formatted
  (println "  " line))
## ── String processing — cleaning imported data ─────────────────────

#
# Imagine importing raw contact names from a CSV file.

(def raw-input "  Alice, Bob , Carol , Dave  ")

(def split-names (string/split raw-input ","))
(assert (= (length split-names) 4) "split into 4 parts")

# Clean up whitespace
(var clean @[])
(each n in split-names
  (push clean (string/trim n)))
(assert (= (get clean 0) "Alice") "trimmed first name")
(assert (= (get clean 3) "Dave") "trimmed last name")
(println "  cleaned import: " clean)

# String operations on email addresses
(assert (string/starts-with? alice-email "alice") "starts-with?")
(assert (string/ends-with? alice-email ".com") "ends-with?")
(assert (= (string/find alice-email "@") 5) "find: @ position")
(assert (= (string/find "abcabc" "bc" 2) 4) "find: with offset")
(assert (= (string/find "hello" "xyz") nil) "find: not found → nil")

# Extract domain from an email
(defn email-domain [email]
  "Extract the domain part of an email address."
  (let* ([at-pos (string/find email "@")]
         [domain (slice email (+ at-pos 1) (length email))])
    domain))

(assert (= (email-domain "alice@example.com") "example.com") "email-domain")
(println "  domain: " (email-domain "alice@example.com"))

# Case and replace
(assert (= (string/upcase "hello") "HELLO") "upcase")
(assert (= (string/downcase "HELLO") "hello") "downcase")
(assert (= (string/replace "foo-bar-baz" "-" "_") "foo_bar_baz") "replace")
## ── Grapheme clusters — strings are human-readable units ───────────

#
# String indexing, length, and iteration operate on grapheme clusters.
# An emoji with a skin-tone modifier is one element, not two codepoints.

(assert (= (length "hello") 5) "ASCII: one grapheme per byte")
(assert (= (length "héllo") 5) "precomposed é: one grapheme")
(assert (= (length "👋🏽") 1) "wave + skin tone: one grapheme")
(assert (= (get "👋🏽" 0) "👋🏽") "get: whole cluster")
(println "  length(\"hello\")=" (length "hello") "  length(\"👋🏽\")=" (length "👋🏽"))

# Iterating yields grapheme clusters
(var graphemes @[])
(each g in "aé👋🏽"
  (push graphemes g))
(assert (= (length graphemes) 3) "three grapheme clusters")
(assert (= (get graphemes 2) "👋🏽") "third: wave emoji")

# Flag emoji: two regional indicators, one grapheme
(assert (= (length "🇫🇷") 1) "flag: one grapheme")

# Slicing and finding respect grapheme boundaries
(assert (= (slice "héllo" 1 4) "éll") "slice: grapheme indices")
(assert (= (string/find "aé👋🏽bc" "👋🏽") 2) "find: grapheme index of emoji")
## ── Array mutation — managing an invite list ───────────────────────

(var invites @[:alice :carol])

# insert at a position
(insert invites 1 :bob)
(assert (= (get invites 1) :bob) "insert at index 1")
(assert (= (length invites) 3) "insert grew the array")

# remove by index
(remove invites 1)
(assert (= (length invites) 2) "remove shrunk the array")
(assert (= (get invites 1) :carol) "remove shifted elements")

# push / pop (stack-style, on the end)
(push invites :dave)
(assert (= (length invites) 3) "push extends")
(def popped (pop invites))
(assert (= popped :dave) "pop returns last")
(assert (= (length invites) 2) "pop shrinks")
## ── Updating and removing contacts ─────────────────────────────────

(assert (has? book "dave") "dave exists")
(del book "dave")
(assert (not (has? book "dave")) "del removes entry")
(assert (= (length (keys book)) 3) "three contacts remain")

# Update alice's tags — structs are immutable, so replace the entry
(def updated-alice (put alice :tags [:dev :lead :admin]))
(put book "alice" updated-alice)
(def {:tags new-tags} (get book "alice"))
(assert (= (length new-tags) 3) "alice now has 3 tags")
(println "  updated alice tags: " new-tags)

# del creates a new struct without a key
(def point {:x 1 :y 2 :z 3})
(def point2d (del point :z))
(assert (= (get point :z) 3) "original struct unchanged")
(assert (not (has? point2d :z)) "new struct lacks :z")
## ── concat vs append ───────────────────────────────────────────────

#
# concat always returns a new value. Neither argument is mutated.
# append on mutable types mutates the first argument in place.

# Immutable: concat creates new
(def t1 [1 2])
(def t2 [3 4])
(def t3 (concat t1 t2))
(assert (= (length t1) 2) "concat: original unchanged")
(assert (= (length t3) 4) "concat: new has all elements")

# Mutable: append mutates
(def a1 @[1 2])
(def a2 @[3 4])
(append a1 a2)
(assert (= (length a1) 4) "append: mutated in place")

# Strings are immutable — concat creates new
(def s1 "hello")
(def s3 (concat s1 " world"))
(assert (= s1 "hello") "concat: original string unchanged")
(assert (= s3 "hello world") "concat: new string")
## ── Splice — spreading into calls and constructors ─────────────────

#
# ;expr spreads an array into a function call's arguments.

(def nums @[1 2 3])
(assert (= (+ ;nums) 6) "splice: spread array into +")

(def more [10 20])
(assert (= (+ ;more) 30) "splice: spread array into +")

# Splice in data constructors
(def base @[1 2])
(def extended-arr @[;base 3 4])
(assert (= (length extended-arr) 4) "splice in array literal")
(assert (= (get extended-arr 2) 3) "splice: element order")
## ── Export — putting it all together ───────────────────────────────

#
# Generate a CSV export of the contact book, using destructuring,
# threading, each, splice, and string operations.

(defn export-csv [the-book]
  "Export the contact book as CSV lines."
  (var lines @["name,email,tags"])
  (each k in (keys the-book)
    (let ([{:name name :email email :tags tags} (get the-book k)])
      (push lines
        (-> name
            (append ",")
            (append email)
            (append ",")
            (append (format-tags tags))))))
  lines)

(def csv (export-csv book))
(assert (= (get csv 0) "name,email,tags") "csv: header")
(assert (= (length csv) 4) "csv: header + 3 data lines")

(println "  csv output:")
(each line in csv
  (println "    " line))

# Every data line should contain an @
(each line in (rest (list ;csv))
  (assert (string/contains? line "@") "csv: data has email"))

(assert (string/contains? (get csv 1) "Alice") "csv: alice in first line")


(println "")
## ── Sets — immutable and mutable ───────────────────────────────────

#
# Sets are unordered collections of unique values.
# Immutable sets: |1 2 3|
# Mutable sets: @|1 2 3|

# Literal syntax
(assert (= || ||) "empty immutable set")
(assert (= @|| @||) "empty mutable set")
(assert (= (set 1 2 3) (set 1 2 3)) "immutable set")
(assert (= @|1 2 3| @|1 2 3|) "mutable set")

# Order doesn't matter in sets
(assert (= (set 3 1 2) (set 1 2 3)) "set order independence")

# Deduplication
(assert (= (set 1 1 2 2 3) (set 1 2 3)) "set deduplication")

# Constructors
(assert (= (set 1 2 3) (set 1 2 3)) "set constructor")
(assert (= (@set 1 2 3) @|1 2 3|) "mutable-set constructor")

# Predicates
(assert (set? (set 1 2 3)) "set? on immutable set")
(assert (set? @|1 2 3|) "set? on mutable set")
(assert (not (set? [1 2 3])) "set? on array")

# Type discrimination
(assert (= (type-of (set 1 2 3)) :set) "type-of immutable set")
(assert (= (type-of @|1 2 3|) :@set) "type-of mutable set")

# Membership
(assert (contains? (set 1 2 3) 2) "contains? true")
(assert (not (contains? (set 1 2 3) 4)) "contains? false")

# Element operations
(assert (= (add (set 1 2) 3) (set 1 2 3)) "add to immutable set")
(assert (= (del (set 1 2 3) 2) (set 1 3)) "del from immutable set")

# Set algebra
(assert (= (union (set 1 2) (set 2 3)) (set 1 2 3)) "union")
(assert (= (intersection (set 1 2 3) (set 2 3 4)) (set 2 3)) "intersection")
(assert (= (difference (set 1 2 3) (set 2 3)) (set 1)) "difference")

# Length and empty?
(assert (= (length (set 1 2 3)) 3) "set length")
(assert (empty? ||) "empty? on empty set")
(assert (not (empty? (set 1))) "empty? on non-empty set")

# Conversion
(assert (= (length (set->array (set 3 1 2))) 3) "set->array conversion")

# Freeze/thaw
(assert (= (freeze @|1 2 3|) (set 1 2 3)) "freeze mutable set")
(assert (= (type-of (freeze @|1 2 3|)) :set) "freeze returns immutable")
(assert (= (thaw (set 1 2 3)) @|1 2 3|) "thaw immutable set")
(assert (= (type-of (thaw (set 1 2 3))) :@set) "thaw returns mutable")

# Iteration with each
(var set-sum 0)
(each x (set 1 2 3)
  (assign set-sum (+ set-sum x)))
(assert (= set-sum 6) "each on set")

# Mapping over sets
(def doubled (map (fn (x) (* x 2)) (set 1 2 3)))
(assert (set? doubled) "map returns set")
(assert (contains? doubled 2) "map: 1*2=2")
(assert (contains? doubled 4) "map: 2*2=4")
(assert (contains? doubled 6) "map: 3*2=6")

(println "all collections passed.")
