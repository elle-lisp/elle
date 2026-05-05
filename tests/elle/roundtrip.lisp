(elle/epoch 10)
# ── literal round-trip: (string x) matches the literal syntax of x ────
#
# Every type's string representation must match its literal input form,
# including the @ mutability sigil.

# ── arrays ───────────────────────────────────────────────────────────

(assert (= (string [1 2 3]) "[1 2 3]") "array round-trips")
(assert (= (string @[1 2 3]) "@[1 2 3]") "@array round-trips")
(assert (= (string []) "[]") "empty array round-trips")
(assert (= (string @[]) "@[]") "empty @array round-trips")

# ── structs ──────────────────────────────────────────────────────────

(assert (= (string {:a 1}) "{:a 1}") "struct round-trips")
(assert (= (string @{:a 1}) "@{:a 1}") "@struct round-trips")
(assert (= (string {}) "{}") "empty struct round-trips")
(assert (= (string @{}) "@{}") "empty @struct round-trips")

# ── sets ─────────────────────────────────────────────────────────────

(assert (= (string |1 2 3|) "|1 2 3|") "set round-trips")
(assert (= (string @|1 2 3|) "@|1 2 3|") "@set round-trips")
(assert (= (string ||) "||") "empty set round-trips")

# ── scalars ──────────────────────────────────────────────────────────

(assert (= (string 42) "42") "int round-trips")
(assert (= (string 3.14) "3.14") "float round-trips")
(assert (= (string true) "true") "true round-trips")
(assert (= (string false) "false") "false round-trips")
(assert (= (string nil) "nil") "nil round-trips")
(assert (= (string :foo) "foo") "keyword string is name")

# ── lists ────────────────────────────────────────────────────────────

(assert (= (string '(1 2 3)) "(1 2 3)") "list round-trips")
(assert (= (string ()) "()") "empty list round-trips")

# ── nested containers preserve sigils ────────────────────────────────

(assert (= (string @[@[1] @[2]]) "@[@[1] @[2]]") "nested @arrays round-trip")
(assert (= (string [@[1] [2]]) "[@[1] [2]]") "mixed array nesting round-trips")

(println "all literal round-trip tests passed")
