(elle/epoch 12)
## tests/elle/prim-net.lisp
## TCP/UDP network primitives

## ── tcp/listen ─────────────────────────────────────────────────────

(let [p (tcp/listen "127.0.0.1" 0)]
  (assert (port? p) "tcp/listen returns port")
  (port/close p))

(let [[ok? _] (protect ((fn [] (tcp/listen "not-a-valid-addr" 0))))]
  (assert (not ok?) "tcp/listen bad addr errors"))

(let [[ok? _] (protect ((fn [] (tcp/listen "127.0.0.1" 99999))))]
  (assert (not ok?) "tcp/listen bad port errors"))

(let [[ok? _] (protect ((fn [] (tcp/listen 42 0))))]
  (assert (not ok?) "tcp/listen non-string addr errors"))

## ── tcp/accept ─────────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (tcp/accept 42))))]
  (assert (not ok?) "tcp/accept non-port errors"))

## ── tcp/connect ────────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (tcp/connect "127.0.0.1" 99999))))]
  (assert (not ok?) "tcp/connect bad port errors"))

## ── udp/bind ───────────────────────────────────────────────────────

(let [p (udp/bind "127.0.0.1" 0)]
  (assert (port? p) "udp/bind returns port")
  (port/close p))

(println "prim-net: all tests passed")
