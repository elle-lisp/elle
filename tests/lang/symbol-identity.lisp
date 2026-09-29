(elle/epoch 12)
# ── A symbol means the same thing in every symbol table ───────────────
#
# A symbol id is the FNV-1a hash of its name (docs/impl/symbol.md), so a
# symbol built by an OS-thread worker — which compiles against its OWN table
# — is the symbol the main thread wrote. These asserts run on the main
# thread; only the construction happens in the worker.
#
# (epoch 12: sys/spawn is the heavy, stdlib-backed worker. At an earlier
#  epoch the migration would rewrite sys/spawn → sys/spawn-vm.)
#
# The names below are deliberately unusual so that nothing else interns them
# first: the defect these guard against is a table that met the same names in
# a different order.

# ── A symbol value crosses unchanged ──────────────────────────────────

(assert (= 'sigil-alpha (sys/join (sys/spawn (fn [] 'sigil-alpha))))
        "a quoted symbol built in a worker equals the main thread's")

# ── A symbol-keyed struct is readable on the other side ───────────────
#
# Struct keys cross as bare ids with no name beside them. Counterfactual:
# with per-table ids the worker's key `sigil-one` arrived as the main
# thread's `sigil-two` (or as a symbol the file never mentioned), so this
# lookup returned the neighbouring value or nil.

(def from-worker (sys/join (sys/spawn (fn [] {'sigil-one 1 'sigil-two 2}))))

(assert (= 1 (get from-worker 'sigil-one))
        "the main thread's key finds the entry the worker stored")
(assert (= 2 (get from-worker 'sigil-two)) "and so does the second key")

# ── A struct built either side is the same struct ─────────────────────
#
# An immutable struct is a SORTED array, and symbol keys sort by id. Equal
# contents therefore demand equal layouts: the worker's sort and the main
# thread's sort must agree, key for key.

(def here {'sigil-zeta 1 'sigil-mu 2 'sigil-alpha 3})
(def there
  (sys/join (sys/spawn (fn [] {'sigil-zeta 1 'sigil-mu 2 'sigil-alpha 3}))))

(assert (= here there) "the same struct literal is the same struct in a worker")
(assert (= (string here) (string there))
        "and prints identically — same key names in the same sorted order")

# ── The same for an immutable set ─────────────────────────────────────

(def set-here (set 'sigil-zeta 'sigil-mu 'sigil-alpha))
(def set-there
  (sys/join (sys/spawn (fn [] (set 'sigil-zeta 'sigil-mu 'sigil-alpha)))))

(assert (= set-here set-there)
        "the same set literal is the same set in a worker")
(assert (= (string set-here) (string set-there))
        "and prints identically — same elements in the same sorted order")

# ── A worker's eval agrees with the main thread's reader ──────────────
#
# `eval` interns against the worker's table; the quoted datum reaching it was
# read against the main thread's. Both must name one symbol.

(assert (= 5 (sys/join (sys/spawn (fn [] (eval '(get {'sigil-k 5} 'sigil-k))))))
        "a worker's eval reads the key the main thread's reader wrote")
