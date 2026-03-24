## Isolate the subprocess stdout pipe hang

# Test 1: echo (non-elle child, multiple lines)
(eprintln "test 1: echo multi-line")
(def r1 (subprocess/system "sh" ["-c" "echo line1; echo line2; echo line3"]))
(eprintln "  stdout=" (string/size-of r1:stdout) " bytes: '" r1:stdout "'")

# Test 2: elle child with single println
(eprintln "test 2: elle single println")
(def r2 (subprocess/system "sh" ["-c" "echo '(println 42)' | ./target/debug/elle"]))
(eprintln "  stdout=" (string/size-of r2:stdout) " bytes: '" r2:stdout "'")

# Test 3: elle child with two printlns (via sh pipe)
(eprintln "test 3: elle two printlns via sh pipe")
(def r3 (subprocess/system "sh" ["-c" "echo '(begin (println 1) (println 2))' | ./target/debug/elle"]))
(eprintln "  stdout=" (string/size-of r3:stdout) " bytes: '" r3:stdout "'")

# Test 4: elle child script with two printlns (via file)
(eprintln "test 4: elle script with two printlns")
(def r4 (subprocess/system "./target/debug/elle" ["tests/elle/subprocess-child.lisp"]))
(eprintln "  stdout=" (string/size-of r4:stdout) " bytes: '" r4:stdout "'")

(eprintln "all subprocess pipe tests done")
