(elle/epoch 12)
# Runtime reflection (eval) inside the two OS-thread worker environments.
# (epoch 11: sys/spawn is the heavy, stdlib-backed worker; sys/spawn-vm is light.
#  This file targets the current epoch so its sys/spawn stays heavy — at an
#  earlier epoch the migration would rewrite sys/spawn → sys/spawn-vm.)
# See docs/threads.md § Two worker environments.
#
# A spawned worker compiles eval'd code against its OWN symbol table + globals.
# `sys/spawn-vm` materializes only primitives + %-intrinsics; `sys/spawn` also
# loads the standard library. The asserts run on the main thread (which has
# stdlib); only the eval happens in the worker.

# ── sys/spawn-vm (light): primitives + intrinsics, NOT stdlib ──────────
# A real primitive resolves (was :eval-error "symbol table not available"
# before workers installed a symbol table at all).
(assert (integer? (sys/join (sys/spawn-vm (fn [] (eval (quote (sys/thread-id)))))))
        "spawn-vm: eval of a primitive resolves")

# A %-intrinsic resolves and computes.
(assert (= 3 (sys/join (sys/spawn-vm (fn [] (eval (quote (%add 1 2)))))))
        "spawn-vm: eval of an intrinsic resolves")

# Special forms (begin/def/quote/if) are recognized by the analyzer by NAME,
# so they resolve in a light worker too — they are not stdlib. This only works
# because a quoted symbol crosses the spawn boundary by name and re-interns in
# the worker's own table; a raw sender-table id would not name `begin`/`def`
# there. (Regression guard: before symbols carried their name, this was a loud
# :eval-error "Unknown symbol".)
(assert (= 7
           (sys/join (sys/spawn-vm (fn []
                                     (eval (quote (begin
                                       (def x 7)
                                       x)))))))
        "spawn-vm: eval of special forms (begin/def) resolves")
(assert (= 5 (sys/join (sys/spawn-vm (fn [] (eval (quote (if true 5 6)))))))
        "spawn-vm: eval of the if special form resolves")

# A primitive reached through a special form (def + a primitive call).
(assert (= 9
           (sys/join (sys/spawn-vm (fn []
                                     (eval (quote (begin
                                       (def b (%add 4 5))
                                       b)))))))
        "spawn-vm: special form binding a primitive result resolves")

# A stdlib name (+) is NOT available in a light worker — eval fails. This pins
# the boundary that motivates the heavy spawn.
(let [[ok? _] (protect (sys/join (sys/spawn-vm (fn [] (eval (quote (+ 1 2)))))))]
  (assert (not ok?) "spawn-vm: eval of a stdlib fn (+) fails (no stdlib)"))

# ── sys/spawn (heavy): stdlib is loaded, so eval sees the full vocabulary ─
(assert (= 3 (sys/join (sys/spawn (fn [] (eval (quote (+ 1 2)))))))
        "spawn: eval of a stdlib fn (+) resolves")

(assert (= [2 3 4]
           (sys/join (sys/spawn (fn [] (eval (quote (map inc [1 2 3])))))))
        "spawn: eval of a higher-order stdlib fn (map) resolves")

# Both still run an ordinary shipped closure (no eval) and join its value.
(assert (= 7 (sys/join (sys/spawn-vm (fn [] (+ 3 4)))))
        "spawn-vm: plain closure")
(assert (= 7 (sys/join (sys/spawn (fn [] (+ 3 4))))) "spawn: plain closure")
