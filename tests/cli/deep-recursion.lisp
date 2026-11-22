(elle/epoch 18)
## audited: 2026-11-22
## A recursion 100,000 deep completes on the build as shipped, and each limit on depth halts the process with a message.
## tests/cli/overview.md
## docs/impl/vm.md
##
## The same recursions with the JIT off and with every function compiled on its
## first call run on the rig, in rig/tests/deep_recursion.rs.

(include-file "../runner/lib/harness.lisp")

(defn halts [program needles]
  "Run PROGRAM, and require a halt: exit 1 with a message holding each of
   NEEDLES, never a signal. A negative exit is the signal that killed it, and
   -11 or -6 is the Rust stack overflowing."
  (let [r (elle-run ["-e" program])]
    (assert (= r:exit 1)
            (string "a halt exits 1, got " r:exit "; stderr: " r:err))
    (each needle needles
      (assert (string/contains? r:err needle)
              (string "stderr lacks " needle ": " r:err)))))

# The build decides where each call runs: the recursion starts interpreted and
# its callee compiles once it is hot, part of the way down.
(eprintln "scenario: a non-tail recursion 100,000 deep completes")
(let [r (elle-run ["-e"
                   (string "(defn sum-to [n] (if (= n 0) 0 (+ n (sum-to (- n 1))))) "
                           "(println (sum-to 100000))")])]
  (assert (= r:exit 0)
          (string "a non-tail recursion 100,000 deep did not complete: " r:exit
                  "; stderr: " r:err))
  (assert (= (string/trim r:out) "5000050000") (string "printed " r:out)))

(eprintln "scenario: a runaway recursion halts at the depth cap")
(halts (string "(vm/config-set :max-depth 50000) "
               "(defn down [n] (pair n (down (+ n 1)))) (length (down 0))")
       ["stack-overflow" "50000"])

# `arena/allocs` runs its thunk by re-entering the VM from Rust, so this
# recursion nests Rust frames at every level whatever the tier, and the depth
# cap is far away. The counter-factual: with no native-stack check at the
# re-entry, the thread overflows and the process dies of SIGSEGV.
(eprintln "scenario: a reentrant recursion halts before the native stack overflows")
(halts "(defn nest [n] (+ 1 (first (arena/allocs (fn [] (nest (+ n 1))))))) (nest 0)"
       ["stack-overflow" "native stack"])

(eprintln "all deep-recursion scenarios passed")
