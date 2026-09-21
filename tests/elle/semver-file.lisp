(elle/epoch 12)
# audited: 2026-09-21
# std/semver/file — the .surface format: render, parse, and the roundtrip.
#
# The counter-factual: the file is the diff baseline reviewers read, so
# the byte layout is contract. A renderer that reorders, drops a field,
# or emits an unsorted line would churn every PR diff over it.

(def sfile ((import "std/semver/file")))

(def run-rec
  {:kind :fn
   :required 1
   :optional 0
   :rest :none
   :named-keys []
   :params ["job"]
   :signals {:bits [:error] :propagates []}
   :doc "12ab34cd"})

(def surface
  {:format 1
   :module "std/demo"
   :version "1.2.3"
   :mode :hybrid
   :released {:commit "abc123" :date "2026-09-21"}
   :tests "tests/elle/demo*.lisp"
   :constructor {:required 1
                 :optional 0
                 :rest :none
                 :named-keys []
                 :params ["dep"]}
   :exports {:run run-rec
             :limit {:kind :value :type :integer :hash "0000002a"}
             :open {:kind :fn
                    :required 1
                    :optional 0
                    :rest :named
                    :named-keys [:create? :mode]
                    :params ["path"]
                    :signals {:bits [] :propagates []}}
             :spread {:kind :fn
                      :required 1
                      :optional 2
                      :rest :list
                      :named-keys []
                      :params ["a" "b" "c"]
                      :signals {:bits [] :propagates [0 2]}}}})

(def expected
  (string "(elle-surface 1)\n" "(module \"std/demo\")\n" "(version \"1.2.3\")\n"
          "(mode :hybrid)\n"
          "(released :commit \"abc123\" :date \"2026-09-21\")\n"
          "(tests \"tests/elle/demo*.lisp\")\n" "(constructor [dep])\n"
          "(export limit :value :type :integer :hash \"0000002a\")\n"
          "(export open :fn [path &named :create? :mode] :signals [])\n"
          "(export run :fn [job] :signals [:error] :doc \"12ab34cd\")\n"
          "(export spread :fn [a &opt b c & rest] :signals []"
          " :propagates [0 2])\n"))

# Rendering is the pinned byte layout: sorted exports, fixed field order.
(assert (= (sfile:render surface) expected) "render matches the format")

# Parse inverts render, field for field.
(assert (= (sfile:parse expected) surface) "parse inverts render")

# Text roundtrips byte-identically.
(assert (= (sfile:render (sfile:parse expected)) expected)
        "render after parse reproduces the bytes")

# A minimal surface: no provenance, no tests, no constructor.
(def bare
  {:format 1
   :module "std/tiny"
   :version "0.1.0"
   :mode :static
   :exports {:f {:kind :fn
                 :required 0
                 :optional 0
                 :rest :none
                 :named-keys []
                 :params []
                 :signals {:bits [] :propagates []}}}})
(assert (= (sfile:parse (sfile:render bare)) bare) "bare surface roundtrips")

# A trait-carrying value export keeps its methods, sorted.
(def bag
  {:format 1
   :module "std/bag"
   :version "1.0.0"
   :mode :hybrid
   :exports {:empty-bag {:kind :value
                         :type :struct
                         :hash "deadbeef"
                         :traits {:Collection [:conj :empty]}}}})
(assert (= (sfile:parse (sfile:render bag)) bag) "traits roundtrip")

# Malformed text refuses loudly.
(let [[ok? _] (protect (sfile:parse "(not-a-surface 1)\n"))]
  (assert (not ok?) "wrong header"))
(let [[ok? _] (protect (sfile:parse "(elle-surface 2)\n"))]
  (assert (not ok?) "unknown format version"))
(let [[ok? _] (protect (sfile:parse "(elle-surface 1)\n(export f :gadget)\n"))]
  (assert (not ok?) "unknown export kind"))

(println "semver-file: all tests passed")
