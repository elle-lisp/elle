#!/usr/bin/env elle

# Collections — a contact book application
#
# We build a contact book from scratch, exercising every collection type
# and operation the language offers. Contacts are immutable structs, the
# book is a mutable table, and we query, format, and export it using
# lists, arrays, tuples, strings, and grapheme-aware text processing.
#
# Demonstrates:
#   Literal syntax    — [array]  @[array]  {struct}  @{struct}  "string"  @"string"
#   Mutability        — @ prefix means mutable; bare means immutable
#   Polymorphic ops   — get, length, empty?, append, concat across types
#   put semantics     — mutates mutable types, copies immutable types
#   Iteration         — each ... in ... (prelude macro)
#   Destructuring     — {:key var} and [a b] patterns in let/def/fn
#   Threading macros  — ->  ->>
#   Splice            — ;expr for spreading arrays/tuples into calls
#   Key-value ops     — put, del, keys, values, has?
#   List ops          — cons, first, rest, reverse, take, drop, last, butlast
#   Array mutation    — push, pop, insert, remove
#   String ops        — string/find, string/split, string/join, slice,
#                       string/replace, string/contains?, string/trim,
#                       string/upcase, string/downcase
#   Grapheme clusters — string indexing operates on what humans see as characters

(def {:assert-eq assert-eq :assert-equal assert-equal :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "./examples/assertions.lisp")))


# ========================================
# 1. Creating the contact book
# ========================================
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

# The book is a mutable table — entries come and go.
(def book @{})

# put on a mutable table mutates it in place and returns the same object.
(def same-book (put book "alice" alice))
(assert-eq same-book book "put on table: same object returned")

# put on an immutable struct returns a NEW struct — original unchanged.
(def alice-with-phone (put alice :phone "555-0100"))
(assert-false (has? alice :phone) "put on struct: original unchanged")
(assert-eq (get alice-with-phone :phone) "555-0100" "put on struct: new has key")

# Populate the book
(put book "bob"   bob)
(put book "carol" carol)
(put book "dave"  dave)
(assert-eq (length (keys book)) 4 "book has 4 contacts")
(display "  book has ") (display (length (keys book))) (print " contacts")


# ========================================
# 2. Accessing contacts — polymorphic get
# ========================================
#
# get works on every collection type with the same interface.

# By string key (@struct)
(assert-eq (get (get book "alice") :name) "Alice" "get: table → struct")

# By keyword (struct)
(assert-eq (get alice :email) "alice@example.com" "get: struct by keyword")

# By index (@array)
(assert-eq (get (get alice :tags) 0) :dev "get: tuple by index")

# By index (string — returns a grapheme cluster)
(assert-eq (get "hello" 0) "h" "get: string by index")

# get with a default for missing keys
(assert-eq (get book "eve" :not-found) :not-found "get: missing → default")

# Drill into nested data with thread-first
(def alice-email
  (-> book (get "alice") (get :email)))
(assert-eq alice-email "alice@example.com" "->: nested access")
(display "  alice email: ") (print alice-email)


# ========================================
# 3. Destructuring contacts
# ========================================
#
# Structs destructure by key; tuples by position.

# Unpack a contact's fields
(def {:name aname :email aemail :tags atags}
  (get book "alice"))
(assert-eq aname "Alice" "struct destructure: name")
(assert-eq aemail "alice@example.com" "struct destructure: email")
(display "  alice → ") (display aname) (display " <") (display aemail) (print ">")

# Unpack tags by position
(def [first-tag second-tag] atags)
(assert-eq first-tag :dev "array destructure: first tag")
(assert-eq second-tag :lead "array destructure: second tag")

# Nested destructuring in let: struct containing an array
(let ([{:name n :tags [t & _]} (get book "carol")])
  (assert-eq n "Carol" "let destructure: name")
  (assert-eq t :ops "let destructure: first tag")
  (display "  carol → ") (display n) (display " first-tag=") (print t))

# Destructuring in function params
(defn contact-line [{:name name :email email}]
  "Format a contact as Name <email>."
  (-> name (append " <") (append email) (append ">")))

(assert-eq (contact-line alice) "Alice <alice@example.com>"
  "fn param destructure")


# ========================================
# 4. Lists — collecting keys and contacts
# ========================================
#
# Lists are cons cells. Ideal for accumulation and recursion.

(def names (keys book))
(assert-true (list? names) "keys returns a list")
(assert-eq (length names) 4 "four names")

# cons prepends, first/rest decompose
(def with-eve (cons "eve" names))
(assert-eq (first with-eve) "eve" "cons prepends")
(assert-eq (length with-eve) 5 "cons adds one")

# last, reverse, take, drop, butlast
(assert-eq (last names) (first (reverse names)) "last = first of reverse")
(assert-eq (length (take 2 names)) 2 "take 2")
(assert-eq (length (drop 2 names)) 2 "drop 2")
(assert-eq (length (butlast names)) 3 "butlast drops one")


# ========================================
# 5. each — iterating the book
# ========================================

# Collect all contacts into an array
(var all-contacts @[])
(each k in (keys book)
  (push all-contacts (get book k)))
(assert-eq (length all-contacts) 4 "each: collected all contacts")

# Count total tags across all contacts
(var total-tags 0)
(each c in all-contacts
  (assign total-tags (+ total-tags (length (get c :tags)))))
(display "  total tags across all contacts: ") (print total-tags)
(assert-true (> total-tags 0) "each: summed tag counts")

# each over a tuple (alice's tags)
(var tag-count 0)
(each t in (get alice :tags)
  (assign tag-count (+ tag-count 1)))
(assert-eq tag-count 2 "each over tuple")

# each over a string (by grapheme cluster)
(var char-count 0)
(each ch in "hello"
  (assign char-count (+ char-count 1)))
(assert-eq char-count 5 "each over string")


# ========================================
# 6. Querying — finding contacts by tag
# ========================================

(defn has-tag? [contact tag]
  "Check whether a contact has a given tag."
  (var found false)
  (each t in (get contact :tags)
    (when (= t tag)
      (assign found true)))
  found)

(assert-true (has-tag? alice :lead) "alice is a lead")
(assert-false (has-tag? bob :lead) "bob is not a lead")

# Collect leads
(var leads @[])
(each k in (keys book)
  (when (has-tag? (get book k) :lead)
    (push leads k)))
(assert-eq (length leads) 2 "two leads found")
(display "  leads: ") (print leads)

# Collect devs
(var devs @[])
(each k in (keys book)
  (when (has-tag? (get book k) :dev)
    (push devs k)))
(assert-eq (length devs) 3 "three devs found")
(display "  devs: ") (print devs)


# ========================================
# 7. Formatting contacts for display
# ========================================

(defn format-tags [tags]
  "Format a tag tuple as [dev, lead]."
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
(assert-true (string/contains? alice-str "Alice") "format: name")
(assert-true (string/contains? alice-str "dev") "format: tag")
(display "  ") (print alice-str)

# Format every contact
(var formatted @[])
(each k in (keys book)
  (push formatted (format-contact (get book k))))
(assert-eq (length formatted) 4 "formatted all contacts")
(each line in formatted
  (display "  ") (print line))


# ========================================
# 8. String processing — cleaning imported data
# ========================================
#
# Imagine importing raw contact names from a CSV file.

(def raw-input "  Alice, Bob , Carol , Dave  ")

(def split-names (string/split raw-input ","))
(assert-eq (length split-names) 4 "split into 4 parts")

# Clean up whitespace
(var clean @[])
(each n in split-names
  (push clean (string/trim n)))
(assert-eq (get clean 0) "Alice" "trimmed first name")
(assert-eq (get clean 3) "Dave" "trimmed last name")
(display "  cleaned import: ") (print clean)

# String operations on email addresses
(assert-true (string/starts-with? alice-email "alice") "starts-with?")
(assert-true (string/ends-with? alice-email ".com") "ends-with?")
(assert-eq (string/find alice-email "@") 5 "find: @ position")
(assert-eq (string/find "abcabc" "bc" 2) 4 "find: with offset")
(assert-eq (string/find "hello" "xyz") nil "find: not found → nil")

# Extract domain from an email
(defn email-domain [email]
  "Extract the domain part of an email address."
  (let* ([at-pos (string/find email "@")]
         [domain (slice email (+ at-pos 1) (length email))])
    domain))

(assert-eq (email-domain "alice@example.com") "example.com" "email-domain")
(display "  domain: ") (print (email-domain "alice@example.com"))

# Case and replace
(assert-eq (string/upcase "hello") "HELLO" "upcase")
(assert-eq (string/downcase "HELLO") "hello" "downcase")
(assert-eq (string/replace "foo-bar-baz" "-" "_") "foo_bar_baz" "replace")


# ========================================
# 9. Grapheme clusters — strings are human-readable units
# ========================================
#
# String indexing, length, and iteration operate on grapheme clusters.
# An emoji with a skin-tone modifier is one element, not two codepoints.

(assert-eq (length "hello") 5 "ASCII: one grapheme per byte")
(assert-eq (length "héllo") 5 "precomposed é: one grapheme")
(assert-eq (length "👋🏽") 1 "wave + skin tone: one grapheme")
(assert-eq (get "👋🏽" 0) "👋🏽" "get: whole cluster")
(display "  length(\"hello\")=") (display (length "hello"))
(display "  length(\"👋🏽\")=") (print (length "👋🏽"))

# Iterating yields grapheme clusters
(var graphemes @[])
(each g in "aé👋🏽"
  (push graphemes g))
(assert-eq (length graphemes) 3 "three grapheme clusters")
(assert-eq (get graphemes 2) "👋🏽" "third: wave emoji")

# Flag emoji: two regional indicators, one grapheme
(assert-eq (length "🇫🇷") 1 "flag: one grapheme")

# Slicing and finding respect grapheme boundaries
(assert-eq (slice "héllo" 1 4) "éll" "slice: grapheme indices")
(assert-eq (string/find "aé👋🏽bc" "👋🏽") 2 "find: grapheme index of emoji")


# ========================================
# 10. Array mutation — managing an invite list
# ========================================

(var invites @[:alice :carol])

# insert at a position
(insert invites 1 :bob)
(assert-eq (get invites 1) :bob "insert at index 1")
(assert-eq (length invites) 3 "insert grew the array")

# remove by index
(remove invites 1)
(assert-eq (length invites) 2 "remove shrunk the array")
(assert-eq (get invites 1) :carol "remove shifted elements")

# push / pop (stack-style, on the end)
(push invites :dave)
(assert-eq (length invites) 3 "push extends")
(def popped (pop invites))
(assert-eq popped :dave "pop returns last")
(assert-eq (length invites) 2 "pop shrinks")


# ========================================
# 11. Updating and removing contacts
# ========================================

(assert-true (has? book "dave") "dave exists")
(del book "dave")
(assert-false (has? book "dave") "del removes entry")
(assert-eq (length (keys book)) 3 "three contacts remain")

# Update alice's tags — structs are immutable, so replace the entry
(def updated-alice (put alice :tags [:dev :lead :admin]))
(put book "alice" updated-alice)
(def {:tags new-tags} (get book "alice"))
(assert-eq (length new-tags) 3 "alice now has 3 tags")
(display "  updated alice tags: ") (print new-tags)

# del creates a new struct without a key
(def point {:x 1 :y 2 :z 3})
(def point2d (del point :z))
(assert-eq (get point :z) 3 "original struct unchanged")
(assert-false (has? point2d :z) "new struct lacks :z")


# ========================================
# 12. concat vs append
# ========================================
#
# concat always returns a new value. Neither argument is mutated.
# append on mutable types mutates the first argument in place.

# Immutable: concat creates new
(def t1 [1 2])
(def t2 [3 4])
(def t3 (concat t1 t2))
(assert-eq (length t1) 2 "concat: original unchanged")
(assert-eq (length t3) 4 "concat: new has all elements")

# Mutable: append mutates
(def a1 @[1 2])
(def a2 @[3 4])
(append a1 a2)
(assert-eq (length a1) 4 "append: mutated in place")

# Strings are immutable — concat creates new
(def s1 "hello")
(def s3 (concat s1 " world"))
(assert-eq s1 "hello" "concat: original string unchanged")
(assert-eq s3 "hello world" "concat: new string")


# ========================================
# 13. Splice — spreading into calls and constructors
# ========================================
#
# ;expr spreads an array or tuple into a function call's arguments.

(def nums @[1 2 3])
(assert-eq (+ ;nums) 6 "splice: spread array into +")

(def more [10 20])
(assert-eq (+ ;more) 30 "splice: spread tuple into +")

# Splice in data constructors
(def base @[1 2])
(def extended-arr @[;base 3 4])
(assert-eq (length extended-arr) 4 "splice in array literal")
(assert-eq (get extended-arr 2) 3 "splice: element order")


# ========================================
# 14. Export — putting it all together
# ========================================
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
(assert-eq (get csv 0) "name,email,tags" "csv: header")
(assert-eq (length csv) 4 "csv: header + 3 data lines")

(display "  csv output:") (print "")
(each line in csv
  (display "    ") (print line))

# Every data line should contain an @
(each line in (rest (list ;csv))
  (assert-true (string/contains? line "@") "csv: data has email"))

(assert-true (string/contains? (get csv 1) "Alice") "csv: alice in first line")


(print "")

# ========================================
# 15. Sets — immutable and mutable
# ========================================
#
# Sets are unordered collections of unique values.
# Immutable sets: |1 2 3|
# Mutable sets: @|1 2 3|

# Literal syntax
(assert-eq || || "empty immutable set")
(assert-eq @|| @|| "empty mutable set")
(assert-eq (set 1 2 3) (set 1 2 3) "immutable set")
(assert-eq @|1 2 3| @|1 2 3| "mutable set")

# Order doesn't matter in sets
(assert-eq (set 3 1 2) (set 1 2 3) "set order independence")

# Deduplication
(assert-eq (set 1 1 2 2 3) (set 1 2 3) "set deduplication")

# Constructors
(assert-eq (set 1 2 3) (set 1 2 3) "set constructor")
(assert-eq (@set 1 2 3) @|1 2 3| "mutable-set constructor")

# Predicates
(assert-true (set? (set 1 2 3)) "set? on immutable set")
(assert-true (set? @|1 2 3|) "set? on mutable set")
(assert-false (set? [1 2 3]) "set? on tuple")

# Type discrimination
(assert-eq (type-of (set 1 2 3)) :set "type-of immutable set")
(assert-eq (type-of @|1 2 3|) :@set "type-of mutable set")

# Membership
(assert-true (contains? (set 1 2 3) 2) "contains? true")
(assert-false (contains? (set 1 2 3) 4) "contains? false")

# Element operations
(assert-eq (add (set 1 2) 3) (set 1 2 3) "add to immutable set")
(assert-eq (del (set 1 2 3) 2) (set 1 3) "del from immutable set")

# Set algebra
(assert-eq (union (set 1 2) (set 2 3)) (set 1 2 3) "union")
(assert-eq (intersection (set 1 2 3) (set 2 3 4)) (set 2 3) "intersection")
(assert-eq (difference (set 1 2 3) (set 2 3)) (set 1) "difference")

# Length and empty?
(assert-eq (length (set 1 2 3)) 3 "set length")
(assert-true (empty? ||) "empty? on empty set")
(assert-false (empty? (set 1)) "empty? on non-empty set")

# Conversion
(assert-eq (length (set->array (set 3 1 2))) 3 "set->array conversion")

# Freeze/thaw
(assert-eq (freeze @|1 2 3|) (set 1 2 3) "freeze mutable set")
(assert-eq (type-of (freeze @|1 2 3|)) :set "freeze returns immutable")
(assert-eq (thaw (set 1 2 3)) @|1 2 3| "thaw immutable set")
(assert-eq (type-of (thaw (set 1 2 3))) :@set "thaw returns mutable")

# Iteration with each
(var set-sum 0)
(each x (set 1 2 3)
  (assign set-sum (+ set-sum x)))
(assert-eq set-sum 6 "each on set")

# Mapping over sets
(def doubled (map (fn (x) (* x 2)) (set 1 2 3)))
(assert-true (set? doubled) "map returns set")
(assert-true (contains? doubled 2) "map: 1*2=2")
(assert-true (contains? doubled 4) "map: 2*2=4")
(assert-true (contains? doubled 6) "map: 3*2=6")

(print "all collections passed.")
