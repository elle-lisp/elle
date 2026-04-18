(elle/epoch 8)
# Network I/O tests — TCP, UDP, and Unix domain sockets
#
# Tests that don't require concurrent peers (pure validation).
# Tests requiring concurrent Rust threads stay in tests/integration/net.rs:
#   - test_tcp_echo_roundtrip
#   - test_udp_roundtrip
#   - test_unix_echo_roundtrip
#   - test_tcp_graceful_shutdown
#   - test_stream_write_via_scheduled


# === TCP/listen ===

(assert (port? (tcp/listen "127.0.0.1" 0)) "tcp/listen returns a port")

(let [[ok? _] (protect ((fn () (tcp/listen "127.0.0.1" 99999))))] (assert (not ok?) "tcp/listen with invalid port (99999) signals error"))

(let [[ok? _] (protect ((fn () (tcp/listen 42 0))))] (assert (not ok?) "tcp/listen with non-string addr signals error"))

# === TCP/accept ===

(let [[ok? _] (protect ((fn () (tcp/accept 42))))] (assert (not ok?) "tcp/accept with non-port arg signals error"))

# === UDP/bind ===

(assert (port? (udp/bind "0.0.0.0" 0)) "udp/bind returns a port")

# === Unix/listen ===

# Create a listener, verify it's a port, then clean up
(let [p (unix/listen "/tmp/elle-test-net-unix-listen.sock")]
  (assert (port? p) "unix/listen returns a port")
  (port/close p))

# === port/set-options ===

# Create a listener, set timeout option, verify no error, then clean up
(let [p (tcp/listen "127.0.0.1" 0)]
  (port/set-options p :timeout 5000)
  (port/close p))

# === TCP/shutdown ===

(let [[ok? _] (protect ((fn () (tcp/shutdown 42 :foo))))] (assert (not ok?) "tcp/shutdown with bad keyword signals error"))
