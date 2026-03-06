# Ports — lifecycle, predicates, display, and standard port parameters

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# === Type predicate ===

(assert-true (port? (port/stdin)) "port? on stdin")
(assert-true (port? (port/stdout)) "port? on stdout")
(assert-true (port? (port/stderr)) "port? on stderr")
(assert-false (port? 42) "port? on int")
(assert-false (port? "hello") "port? on string")
(assert-false (port? nil) "port? on nil")

# === Open predicate ===

(assert-true (port/open? (port/stdin)) "port/open? on fresh stdin")
(assert-true (port/open? (port/stdout)) "port/open? on fresh stdout")

# === Close and open? ===

(let ((p (port/stdin)))
  (assert-true (port/open? p) "port open before close")
  (port/close p)
  (assert-false (port/open? p) "port closed after close"))

# === Idempotent close ===

(let ((p (port/stdout)))
  (port/close p)
  (port/close p)
  (assert-false (port/open? p) "double close is idempotent"))

# === with macro for resource management ===

(with p (port/open "/tmp/elle-test-ports-with-474" :write) port/close
  (assert-true (port/open? p) "port open inside with"))

# === File port open/close lifecycle ===

(let ((p (port/open "/tmp/elle-test-ports-lifecycle-474" :write)))
  (assert-true (port? p) "file port is a port")
  (assert-true (port/open? p) "file port is open after open")
  (port/close p)
  (assert-false (port/open? p) "file port is closed after close"))

# === port/open-bytes ===

(let ((p (port/open-bytes "/tmp/elle-test-ports-bytes-474" :write)))
  (assert-true (port? p) "bytes port is a port")
  (port/close p))

# === Error cases ===

(assert-err
  (fn () (port/open "/tmp/elle-nonexistent-dir-474/file" :read))
  "port/open on nonexistent path errors")

(assert-err
  (fn () (port/open "/tmp/elle-test-474" :badmode))
  "port/open with bad mode errors")

(assert-err
  (fn () (port/close 42))
  "port/close on non-port errors")

(assert-err
  (fn () (port/open? 42))
  "port/open? on non-port errors")

# === Display format ===

(assert-string-eq (string (port/stdin)) "#<port:stdin>" "stdin display")
(assert-string-eq (string (port/stdout)) "#<port:stdout>" "stdout display")
(assert-string-eq (string (port/stderr)) "#<port:stderr>" "stderr display")

# === Standard port parameters ===

(assert-true (parameter? *stdin*) "*stdin* is a parameter")
(assert-true (parameter? *stdout*) "*stdout* is a parameter")
(assert-true (parameter? *stderr*) "*stderr* is a parameter")

(assert-true (port? (*stdin*)) "*stdin* default is a port")
(assert-true (port? (*stdout*)) "*stdout* default is a port")
(assert-true (port? (*stderr*)) "*stderr* default is a port")

# === Parameterize standard ports ===

(let ((custom-port (port/open "/tmp/elle-test-ports-param-474" :write)))
  (parameterize ((*stdout* custom-port))
    (assert-true (port? (*stdout*)) "parameterized *stdout* is a port")
    (assert-true (port/open? (*stdout*)) "parameterized *stdout* is open"))
  (port/close custom-port))

# === Additional error cases ===

(assert-err
  (fn () (port/open 42 :read))
  "port/open with non-string path errors")

# === Read-write and append modes ===

(let ((p (port/open "/tmp/elle-test-ports-readwrite-474" :read-write)))
  (assert-true (port? p) "read-write port is a port")
  (assert-true (port/open? p) "read-write port is open")
  (port/close p))

(let ((p (port/open "/tmp/elle-test-ports-append-474" :append)))
  (assert-true (port? p) "append port is a port")
  (assert-true (port/open? p) "append port is open")
  (port/close p))
