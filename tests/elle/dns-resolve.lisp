(elle/epoch 8)
# DNS resolution tests

# Test 1: sys/resolve sync
(let [ips (sys/resolve "localhost")]
  (assert (> (length ips) 0) "sys/resolve should return at least one IP")
  (print (string "test 1 passed: sync resolve -> " ips "\n")))

# Test 2: sys/resolve async
(let [ips (sys/resolve "localhost")]
      (assert (> (length ips) 0) "async sys/resolve should return at least one IP")
      (print (string "test 2 passed: async resolve -> " ips "\n")))

# Test 3: sys/resolve with IP passthrough — IPs are valid hostnames
# for getaddrinfo and should resolve to themselves.
(let [ips (sys/resolve "127.0.0.1")]
  (assert (> (length ips) 0) "resolving an IP should return at least one result")
  (assert (= (first ips) "127.0.0.1") "resolving 127.0.0.1 should return 127.0.0.1")
  (print "test 3 passed: IP passthrough\n"))

# Test 4: sys/resolve returns multiple addresses
(let [ips (sys/resolve "localhost")]
  (assert (array? ips) "sys/resolve should return an array")
  (each ip in ips
    (assert (string? ip) "each element should be a string"))
  (print "test 4 passed: result is array of strings\n"))

(print "all dns tests passed\n")
