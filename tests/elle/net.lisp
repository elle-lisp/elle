# Network I/O tests — TCP, UDP, and Unix domain sockets
#
# Tests that don't require concurrent peers (pure validation).
# Tests requiring concurrent Rust threads stay in tests/integration/net.rs:
#   - test_tcp_echo_roundtrip
#   - test_udp_roundtrip
#   - test_unix_echo_roundtrip
#   - test_tcp_graceful_shutdown
#   - test_stream_write_via_scheduled

(import-file "tests/elle/assert.lisp")

# === TCP/listen ===

(assert-true (port? (tcp/listen "127.0.0.1" 0))
  "tcp/listen returns a port")

(assert-err (fn () (tcp/listen "127.0.0.1" 99999))
  "tcp/listen with invalid port (99999) signals error")

(assert-err (fn () (tcp/listen 42 0))
  "tcp/listen with non-string addr signals error")

# === TCP/accept ===

(assert-err (fn () (tcp/accept 42))
  "tcp/accept with non-port arg signals error")

# === UDP/bind ===

(assert-true (port? (udp/bind "0.0.0.0" 0))
  "udp/bind returns a port")

# === Unix/listen ===

# Create a listener, verify it's a port, then clean up
(let ((p (unix/listen "/tmp/elle-test-net-unix-listen.sock")))
  (assert-true (port? p) "unix/listen returns a port")
  (port/close p))

# === port/set-options ===

# Create a listener, set timeout option, verify no error, then clean up
(let ((p (tcp/listen "127.0.0.1" 0)))
  (port/set-options p :timeout 5000)
  (port/close p))

# === TCP/shutdown ===

(assert-err (fn () (tcp/shutdown 42 :foo))
  "tcp/shutdown with bad keyword signals error")
